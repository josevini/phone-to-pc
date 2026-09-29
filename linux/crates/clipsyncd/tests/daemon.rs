//! Two real daemons in one process, talking TLS over loopback, with in-memory clipboards.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use clipsync_core::DeviceId;
use clipsync_core::engine::Event;
use clipsyncd::clipboard::MemoryClipboard;
use clipsyncd::daemon::{self, DaemonConfig, DaemonEvent, DaemonHandle};
use clipsyncd::storage::{Config, Dirs, Identity, State};
use tokio::sync::broadcast;

struct Node {
    handle: DaemonHandle,
    clipboard: MemoryClipboard,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Node {
    async fn start(name: &str, peers: Vec<SocketAddr>) -> Node {
        let tmp = tempfile::tempdir().unwrap();
        let dirs =
            Dirs { data: tmp.path().join("data"), config: tmp.path().join("config"), runtime: tmp.path().into() };
        let (handle, clipboard) = Self::launch(&dirs, name, peers).await;
        Node { handle, clipboard, dirs, _tmp: tmp }
    }

    async fn launch(dirs: &Dirs, name: &str, peers: Vec<SocketAddr>) -> (DaemonHandle, MemoryClipboard) {
        let identity = Identity::load_or_create(&dirs.data).unwrap();
        let config = Config { name: name.into(), port: 0, peers };
        let (clipboard, events) = MemoryClipboard::new();
        let cfg = DaemonConfig {
            dirs: dirs.clone(),
            config,
            identity,
            platform: "linux".into(),
            listen: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
        };
        let handle = daemon::spawn(cfg, std::sync::Arc::new(clipboard.clone()), events).await.unwrap();
        (handle, clipboard)
    }

    /// Stops the daemon and starts a new one on the same files.
    async fn restart(&mut self, name: &str, peers: Vec<SocketAddr>) {
        self.handle.shutdown().await;
        let (handle, clipboard) = Self::launch(&self.dirs, name, peers).await;
        self.handle = handle;
        self.clipboard = clipboard;
    }

    async fn id(&self) -> DeviceId {
        self.handle.status().await.id
    }

    async fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.handle.status().await.port))
    }

    async fn connected_to(&self, peer: &DeviceId) -> bool {
        self.handle.status().await.devices.iter().any(|d| d.id == *peer && d.connected)
    }

    fn saved_state(&self) -> State {
        State::load(&self.dirs.state_file()).unwrap()
    }
}

/// Polls `condition` until it holds, failing the test after five seconds.
async fn eventually<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition().await {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn next_pairing_code(events: &mut broadcast::Receiver<DaemonEvent>) -> (u64, u32) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await.expect("no pairing code").unwrap() {
            DaemonEvent::Engine(Event::PairingCode { conn, code, .. }) => return (conn, code),
            _ => continue,
        }
    }
}

async fn pair_with_token(a: &Node, b: &Node) {
    let invite = b.handle.start_pairing().await.unwrap();
    a.handle.pair_with_uri(&invite.uri).await.unwrap();
    let (a_id, b_id) = (a.id().await, b.id().await);
    eventually("both devices connected", || async { a.connected_to(&b_id).await && b.connected_to(&a_id).await }).await;
}

#[tokio::test]
async fn token_pairing_then_copies_flow_both_ways() {
    let (a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    pair_with_token(&a, &b).await;

    a.clipboard.copy("olá do alpha");
    eventually("b receives", || async { b.clipboard.contents().as_deref() == Some("olá do alpha") }).await;
    b.clipboard.copy("back from beta");
    eventually("a receives", || async { a.clipboard.contents().as_deref() == Some("back from beta") }).await;

    let b_id = b.id().await;
    assert_eq!(a.saved_state().paired.iter().map(|p| p.id).collect::<Vec<_>>(), vec![b_id]);
    assert!(a.saved_state().lamport >= 2, "the Lamport counter is persisted");
}

#[tokio::test]
async fn sas_pairing_needs_both_confirmations() {
    let (a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    let (mut a_events, mut b_events) = (a.handle.subscribe(), b.handle.subscribe());
    b.handle.start_pairing().await.unwrap();
    a.handle.pair_with_address(b.addr().await).await.unwrap();

    let (a_conn, a_code) = next_pairing_code(&mut a_events).await;
    let (b_conn, b_code) = next_pairing_code(&mut b_events).await;
    assert_eq!(a_code, b_code);
    a.handle.confirm_pairing(a_conn, true).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(a.saved_state().paired.is_empty(), "a waits for b");
    b.handle.confirm_pairing(b_conn, true).await;

    let (a_id, b_id) = (a.id().await, b.id().await);
    eventually("both devices connected", || async { a.connected_to(&b_id).await && b.connected_to(&a_id).await }).await;
    assert_eq!(b.saved_state().paired[0].name, "alpha");
}

#[tokio::test]
async fn paired_devices_reconnect_after_a_restart() {
    let (mut a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    pair_with_token(&a, &b).await;
    let b_addr = b.addr().await;
    a.restart("alpha", vec![b_addr]).await;

    let (a_id, b_id) = (a.id().await, b.id().await);
    eventually("reconnected", || async { a.connected_to(&b_id).await && b.connected_to(&a_id).await }).await;
    a.clipboard.copy("after restart");
    eventually("b receives", || async { b.clipboard.contents().as_deref() == Some("after restart") }).await;
}

#[tokio::test]
async fn unpairing_is_persisted_on_both_sides() {
    let (a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    pair_with_token(&a, &b).await;
    assert!(a.handle.unpair(b.id().await).await);
    eventually("b forgets a", || async { b.handle.status().await.devices.is_empty() }).await;
    assert!(a.handle.status().await.devices.is_empty());
    assert!(a.saved_state().paired.is_empty() && b.saved_state().paired.is_empty());
}

#[tokio::test]
async fn the_clipboard_at_startup_is_not_sent() {
    let (a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    a.clipboard.preload("already there");
    pair_with_token(&a, &b).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(b.clipboard.contents(), None);
    // Copying the same text again is a new copy, not an echo of the startup content.
    a.clipboard.copy("already there");
    eventually("b receives", || async { b.clipboard.contents().as_deref() == Some("already there") }).await;
}

#[tokio::test]
async fn an_unpaired_device_cannot_connect() {
    let b = Node::start("beta", vec![]).await;
    let c = Node::start("stranger", vec![b.addr().await]).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(b.handle.status().await.devices.is_empty());
    assert!(c.handle.status().await.devices.is_empty());
}

#[tokio::test]
async fn sending_text_reaches_connected_peers() {
    let (a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    pair_with_token(&a, &b).await;
    let outcome = a.handle.send_text("sent from the cli".into()).await;
    assert!(matches!(outcome, clipsync_core::engine::LocalChange::Sent { peers: 1, .. }), "{outcome:?}");
    eventually("b receives", || async { b.clipboard.contents().as_deref() == Some("sent from the cli") }).await;
}

#[tokio::test]
async fn the_daemon_stops_when_the_clipboard_backend_stops() {
    let a = Node::start("alpha", vec![]).await;
    a.clipboard.close("compositor went away");
    tokio::time::timeout(Duration::from_secs(5), a.handle.stopped()).await.expect("daemon still running");
}
