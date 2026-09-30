//! Two real daemons in one process, talking TLS over loopback, with in-memory clipboards.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use clipsync_core::DeviceId;
use clipsync_core::engine::{Event, LocalChange};
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
        Self::launch_with(dirs, name, peers, false).await
    }

    async fn launch_with(
        dirs: &Dirs,
        name: &str,
        peers: Vec<SocketAddr>,
        advertise: bool,
    ) -> (DaemonHandle, MemoryClipboard) {
        let identity = Identity::load_or_create(&dirs.data).unwrap();
        let config = Config { name: name.into(), port: 0, peers };
        let (clipboard, events) = MemoryClipboard::new();
        let cfg = DaemonConfig {
            dirs: dirs.clone(),
            config,
            identity,
            platform: "linux".into(),
            listen: if advertise { "0.0.0.0:0".parse().unwrap() } else { "127.0.0.1:0".parse().unwrap() },
            advertise,
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
    assert!(matches!(outcome, LocalChange::Sent { peers: 1, .. }), "{outcome:?}");
    eventually("b receives", || async { b.clipboard.contents().as_deref() == Some("sent from the cli") }).await;
}

#[tokio::test]
async fn a_paused_daemon_stays_connected_but_neither_sends_nor_applies() {
    let (a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    pair_with_token(&a, &b).await;
    a.handle.set_paused(true).await;
    assert!(a.handle.status().await.paused);

    a.clipboard.copy("private");
    b.clipboard.copy("from beta");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(a.clipboard.contents().as_deref(), Some("private"));
    assert_eq!(b.clipboard.contents().as_deref(), Some("from beta"));
    assert_eq!(a.handle.send_text("sent while paused".into()).await, LocalChange::Paused);
    assert!(a.connected_to(&b.id().await).await);

    a.handle.set_paused(false).await;
    assert!(!a.handle.status().await.paused);
    a.clipboard.copy("shared again");
    eventually("b receives", || async { b.clipboard.contents().as_deref() == Some("shared again") }).await;
}

#[tokio::test]
async fn status_changes_are_reported_once_each() {
    let a = Node::start("alpha", vec![]).await;
    let mut events = a.handle.subscribe();
    a.handle.set_paused(true).await;
    a.handle.set_paused(true).await;
    a.handle.start_pairing().await.unwrap();
    a.handle.stop_pairing();
    a.handle.stop_pairing();
    a.handle.set_paused(false).await;

    let mut changes = vec![];
    while changes.len() < 4 {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv()).await.expect("no status change");
        if let DaemonEvent::StatusChanged(status) = event.unwrap() {
            changes.push((status.paused, status.pairing));
        }
    }
    assert_eq!(changes, vec![(true, false), (true, true), (true, false), (false, false)]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).all(|e| !matches!(e, DaemonEvent::StatusChanged(_))),
        "a change was reported twice"
    );
}

#[tokio::test]
async fn renaming_is_saved_and_reaches_connected_peers() {
    // a knows where b is, as mDNS would tell it, so it reconnects after the rename.
    let b = Node::start("beta", vec![]).await;
    let a = Node::start("alpha", vec![b.addr().await]).await;
    pair_with_token(&a, &b).await;
    std::fs::create_dir_all(&a.dirs.config).unwrap();
    std::fs::write(a.dirs.config_file(), "port = 0\n").unwrap();

    a.handle.rename("alpha 2".into()).await.unwrap();
    assert_eq!(a.handle.status().await.name, "alpha 2");
    let config = std::fs::read_to_string(a.dirs.config_file()).unwrap();
    assert_eq!(config, "port = 0\nname = \"alpha 2\"\n");
    let a_id = a.id().await;
    // Reconnecting can take a retry or two of the backoff: allow more than `eventually` does.
    let deadline = Instant::now() + Duration::from_secs(15);
    while !b.handle.status().await.devices.iter().any(|d| d.id == a_id && d.name == "alpha 2" && d.connected) {
        assert!(Instant::now() < deadline, "b never learnt the new name");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(b.saved_state().paired[0].name, "alpha 2");
}

#[tokio::test]
async fn an_invalid_name_changes_nothing() {
    let a = Node::start("alpha", vec![]).await;
    assert!(a.handle.rename(String::new()).await.is_err());
    assert!(a.handle.rename("a".repeat(65)).await.is_err());
    assert_eq!(a.handle.status().await.name, "alpha");
    assert!(!a.dirs.config_file().exists());
}

#[tokio::test]
async fn pausing_is_kept_across_a_restart() {
    let mut a = Node::start("alpha", vec![]).await;
    a.handle.set_paused(true).await;
    assert!(a.saved_state().paused);
    a.restart("alpha", vec![]).await;
    assert!(a.handle.status().await.paused);
    a.handle.set_paused(false).await;
    assert!(!a.saved_state().paused);
}

#[tokio::test]
async fn the_daemon_stops_when_the_clipboard_backend_stops() {
    let a = Node::start("alpha", vec![]).await;
    a.clipboard.close("compositor went away");
    tokio::time::timeout(Duration::from_secs(5), a.handle.stopped()).await.expect("daemon still running");
}

#[tokio::test]
async fn a_paired_device_found_on_the_network_is_dialed() {
    let (mut a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    pair_with_token(&a, &b).await;
    a.restart("alpha", vec![]).await;
    let (a_id, b_id) = (a.id().await, b.id().await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!a.connected_to(&b_id).await, "nothing tells a where b is yet");

    a.handle.discovered(b_id, vec![b.addr().await]);
    eventually("reconnected", || async { a.connected_to(&b_id).await && b.connected_to(&a_id).await }).await;
}

#[tokio::test]
async fn an_unpaired_device_found_on_the_network_is_not_dialed() {
    let (a, b) = (Node::start("alpha", vec![]).await, Node::start("beta", vec![]).await);
    let mut b_events = b.handle.subscribe();
    a.handle.discovered(b.id().await, vec![b.addr().await]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut saw_connection = false;
    while let Ok(event) = b_events.try_recv() {
        saw_connection |= matches!(event, DaemonEvent::Engine(Event::ConnectionClosed { .. }));
    }
    assert!(!saw_connection, "a must not even try");
}

/// Uses real multicast on the local network, which CI runners may not allow.
#[tokio::test]
#[ignore = "needs mDNS multicast on the local network"]
async fn paired_devices_find_each_other_with_mdns() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = |n: &str| Dirs { data: tmp.path().join(n), config: tmp.path().join(n), runtime: tmp.path().into() };
    let (a_dirs, b_dirs) = (dirs("a"), dirs("b"));
    let (a, _a_clip) = Node::launch_with(&a_dirs, "alpha", vec![], true).await;
    let (b, _b_clip) = Node::launch_with(&b_dirs, "beta", vec![], true).await;
    let invite = b.start_pairing().await.unwrap();
    a.pair_with_uri(&invite.uri).await.unwrap();
    let (a_id, b_id) = (a.status().await.id, b.status().await.id);
    let connected = |h: &DaemonHandle, peer: DeviceId| {
        let h = h.clone();
        async move { h.status().await.devices.iter().any(|d| d.id == peer && d.connected) }
    };
    eventually("paired", || async { connected(&a, b_id).await && connected(&b, a_id).await }).await;

    // Restart a with no configured peers: only mDNS can tell it where b is.
    a.shutdown().await;
    let (a, _a_clip) = Node::launch_with(&a_dirs, "alpha", vec![], true).await;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !(connected(&a, b_id).await && connected(&b, a_id).await) {
        assert!(Instant::now() < deadline, "not reconnected through mDNS");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
