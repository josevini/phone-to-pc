//! The control socket, driven through its client, against real daemons.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clipsyncd::clipboard::MemoryClipboard;
use clipsyncd::daemon::{self, DaemonConfig, DaemonHandle};
use clipsyncd::ipc::{self, Client, Reply, Request, StatusView};
use clipsyncd::storage::{Config, Dirs, Identity};

struct Node {
    handle: DaemonHandle,
    clipboard: MemoryClipboard,
    socket: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Node {
    async fn start(name: &str) -> Node {
        let tmp = tempfile::tempdir().unwrap();
        let dirs =
            Dirs { data: tmp.path().join("data"), config: tmp.path().join("config"), runtime: tmp.path().into() };
        let identity = Identity::load_or_create(&dirs.data).unwrap();
        let (clipboard, events) = MemoryClipboard::new();
        let cfg = DaemonConfig {
            dirs: dirs.clone(),
            config: Config { name: name.into(), port: 0, peers: vec![] },
            identity,
            platform: "linux".into(),
            listen: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
        };
        let handle = daemon::spawn(cfg, Arc::new(clipboard.clone()), events).await.unwrap();
        let socket = dirs.socket();
        ipc::serve(&socket, handle.clone()).await.unwrap();
        Node { handle, clipboard, socket, _tmp: tmp }
    }

    async fn client(&self) -> Client {
        Client::connect(&self.socket).await.unwrap()
    }

    async fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.handle.status().await.port))
    }
}

/// Reads replies until `pick` accepts one, failing after five seconds.
async fn wait_for<T>(client: &mut Client, mut pick: impl FnMut(&Reply) -> Option<T>) -> T {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let reply = tokio::time::timeout_at(deadline, client.next())
            .await
            .expect("timed out waiting for a reply")
            .unwrap()
            .expect("connection closed");
        if let Some(found) = pick(&reply) {
            return found;
        }
    }
}

async fn pair(a: &Node, b: &Node) -> (Client, Client) {
    let mut b_cli = b.client().await;
    let uri = match b_cli.request(&Request::PairStart).await.unwrap() {
        Reply::PairingStarted { uri, .. } => uri,
        other => panic!("unexpected {other:?}"),
    };
    let mut a_cli = a.client().await;
    assert_eq!(a_cli.request(&Request::PairUri { uri }).await.unwrap(), Reply::Ok);
    wait_for(&mut a_cli, |r| matches!(r, Reply::Paired { .. }).then_some(())).await;
    wait_for(&mut b_cli, |r| matches!(r, Reply::Paired { .. }).then_some(())).await;
    (a_cli, b_cli)
}

#[tokio::test]
async fn status_reports_the_device_and_its_paired_devices() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    pair(&a, &b).await;
    let reply = a.client().await.request(&Request::Status).await.unwrap();
    let Reply::Status { status } = reply else { panic!("unexpected {reply:?}") };
    assert_eq!(status.name, "alpha");
    assert!(!status.addrs.is_empty());
    assert_eq!(status.devices.len(), 1);
    assert_eq!(status.devices[0].name, "beta");
}

#[tokio::test]
async fn pairing_with_a_qr_uri_reports_the_paired_device_on_both_sides() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    let _clients = pair(&a, &b).await;
    let status = a.handle.status().await;
    assert!(status.devices.iter().any(|d| d.name == "beta" && d.connected));
}

#[tokio::test]
async fn pairing_by_comparing_codes_asks_both_users() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    let mut b_cli = b.client().await;
    assert!(matches!(b_cli.request(&Request::PairStart).await.unwrap(), Reply::PairingStarted { .. }));
    let mut a_cli = a.client().await;
    let addr = b.addr().await.to_string();
    assert_eq!(a_cli.request(&Request::PairAddress { addr }).await.unwrap(), Reply::Ok);

    let code_on = |r: &Reply| match r {
        Reply::PairingCode { conn, code, name, .. } => Some((*conn, code.clone(), name.clone())),
        _ => None,
    };
    let (a_conn, a_code, a_sees) = wait_for(&mut a_cli, code_on).await;
    let (b_conn, b_code, b_sees) = wait_for(&mut b_cli, code_on).await;
    assert_eq!(a_code, b_code);
    assert_eq!(a_code.len(), 6);
    assert_eq!((a_sees.as_str(), b_sees.as_str()), ("beta", "alpha"));

    a_cli.send(&Request::Confirm { conn: a_conn, accept: true }).await.unwrap();
    b_cli.send(&Request::Confirm { conn: b_conn, accept: true }).await.unwrap();
    wait_for(&mut a_cli, |r| matches!(r, Reply::Paired { .. }).then_some(())).await;
    wait_for(&mut b_cli, |r| matches!(r, Reply::Paired { .. }).then_some(())).await;
}

#[tokio::test]
async fn a_rejected_code_fails_the_pairing() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    let mut b_cli = b.client().await;
    b_cli.request(&Request::PairStart).await.unwrap();
    let mut a_cli = a.client().await;
    a_cli.request(&Request::PairAddress { addr: b.addr().await.to_string() }).await.unwrap();
    let (b_conn, ..) = wait_for(&mut b_cli, |r| match r {
        Reply::PairingCode { conn, .. } => Some((*conn, ())),
        _ => None,
    })
    .await;
    b_cli.send(&Request::Confirm { conn: b_conn, accept: false }).await.unwrap();
    let reason = wait_for(&mut a_cli, |r| match r {
        Reply::PairingFailed { reason } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert!(reason.contains("rejected"), "{reason}");
    assert!(a.handle.status().await.devices.is_empty());
}

#[tokio::test]
async fn an_invalid_pairing_uri_is_an_error() {
    let a = Node::start("alpha").await;
    let reply = a.client().await.request(&Request::PairUri { uri: "https://example.com".into() }).await.unwrap();
    assert!(matches!(reply, Reply::Error { .. }), "{reply:?}");
}

#[tokio::test]
async fn text_sent_through_the_socket_reaches_paired_devices() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    pair(&a, &b).await;
    let reply = a.client().await.request(&Request::Send { text: "via cli".into() }).await.unwrap();
    assert_eq!(reply, Reply::Sent { outcome: "sent".into(), peers: 1 });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while b.clipboard.contents().as_deref() != Some("via cli") {
        assert!(tokio::time::Instant::now() < deadline, "b never received the text");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn sharing_is_paused_and_resumed_through_the_socket() {
    let a = Node::start("alpha").await;
    let mut client = a.client().await;
    let paused = async |client: &mut Client| match client.request(&Request::Status).await.unwrap() {
        Reply::Status { status } => status.paused,
        other => panic!("unexpected {other:?}"),
    };
    assert!(!paused(&mut client).await);

    assert_eq!(client.request(&Request::Pause).await.unwrap(), Reply::Ok);
    assert!(paused(&mut client).await);
    let reply = client.request(&Request::Send { text: "while paused".into() }).await.unwrap();
    assert_eq!(reply, Reply::Sent { outcome: "paused".into(), peers: 0 });

    assert_eq!(client.request(&Request::Resume).await.unwrap(), Reply::Ok);
    assert!(!paused(&mut client).await);
}

#[tokio::test]
async fn a_subscriber_gets_the_status_then_each_change() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    let mut watcher = a.client().await;
    watcher.send(&Request::Subscribe).await.unwrap();
    let status = |pick: fn(&StatusView) -> bool| {
        move |r: &Reply| match r {
            Reply::Status { status } if pick(status) => Some(()),
            _ => None,
        }
    };
    let first = wait_for(&mut watcher, |r| match r {
        Reply::Status { status } => Some(status.clone()),
        _ => None,
    })
    .await;
    assert!(first.devices.is_empty() && !first.paused && !first.pairing);

    a.client().await.request(&Request::Pause).await.unwrap();
    wait_for(&mut watcher, status(|s| s.paused)).await;
    a.client().await.request(&Request::Resume).await.unwrap();
    wait_for(&mut watcher, status(|s| !s.paused)).await;

    let _clients = pair(&b, &a).await;
    wait_for(&mut watcher, status(|s| s.pairing)).await;
    wait_for(&mut watcher, status(|s| s.devices.iter().any(|d| d.name == "beta" && d.connected))).await;

    b.handle.shutdown().await;
    wait_for(&mut watcher, status(|s| s.devices.iter().any(|d| d.name == "beta" && !d.connected))).await;
}

#[tokio::test]
async fn devices_are_unpaired_by_name_or_id_prefix() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    pair(&a, &b).await;
    let mut cli = a.client().await;
    assert!(matches!(cli.request(&Request::Unpair { device: "nobody".into() }).await.unwrap(), Reply::Error { .. }));
    let reply = cli.request(&Request::Unpair { device: "beta".into() }).await.unwrap();
    assert!(matches!(&reply, Reply::Unpaired { name, .. } if name == "beta"), "{reply:?}");
    assert!(a.handle.status().await.devices.is_empty());

    let c = Node::start("gamma").await;
    pair(&a, &c).await;
    let prefix = c.handle.status().await.id.to_string()[..6].to_owned();
    let reply = cli.request(&Request::Unpair { device: prefix }).await.unwrap();
    assert!(matches!(&reply, Reply::Unpaired { name, .. } if name == "gamma"), "{reply:?}");
}

#[tokio::test]
async fn closing_the_client_that_opened_pairing_mode_closes_it() {
    let a = Node::start("alpha").await;
    let mut cli = a.client().await;
    cli.request(&Request::PairStart).await.unwrap();
    assert!(a.handle.status().await.pairing);
    drop(cli);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while a.handle.status().await.pairing {
        assert!(tokio::time::Instant::now() < deadline, "pairing mode still open");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_second_daemon_cannot_take_a_live_socket_but_a_stale_one_is_replaced() {
    let a = Node::start("alpha").await;
    let b = Node::start("beta").await;
    assert!(ipc::serve(&a.socket, b.handle.clone()).await.is_err(), "a's socket is live");

    let stale = a.socket.with_file_name("stale.sock");
    drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());
    assert!(stale.exists());
    ipc::serve(&stale, b.handle.clone()).await.unwrap();
    let reply = Client::connect(&stale).await.unwrap().request(&Request::Status).await.unwrap();
    assert!(matches!(reply, Reply::Status { status } if status.name == "beta"));
}

#[tokio::test]
async fn only_the_user_can_use_the_socket() {
    use std::os::unix::fs::PermissionsExt;
    let a = Node::start("alpha").await;
    let mode = std::fs::metadata(&a.socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
