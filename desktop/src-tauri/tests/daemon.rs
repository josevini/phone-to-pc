//! Following a real daemon's status through its control socket, as the app does.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clipsync_desktop::daemon::{request, watch};
use clipsyncd::clipboard::MemoryClipboard;
use clipsyncd::daemon::{self, DaemonConfig, DaemonHandle};
use clipsyncd::ipc::{self, Reply, Request, StatusView};
use clipsyncd::storage::{Config, Dirs, Identity};
use tokio::sync::mpsc;

/// A daemon with an in-memory clipboard, serving its control socket in `dir`.
async fn start_daemon(dir: &Path, name: &str) -> DaemonHandle {
    let dirs = Dirs { data: dir.join("data"), config: dir.join("config"), runtime: dir.into() };
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
    let handle = daemon::spawn(cfg, Arc::new(clipboard), events).await.unwrap();
    ipc::serve(&dirs.socket(), handle.clone()).await.unwrap();
    handle
}

/// What `watch` reported, in order.
struct Watched(mpsc::UnboundedReceiver<Option<StatusView>>);

impl Watched {
    fn start(socket: &Path) -> Watched {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(watch(socket.to_owned(), Duration::from_millis(50), move |status| {
            let _ = tx.send(status);
        }));
        Watched(rx)
    }

    async fn next(&mut self) -> Option<StatusView> {
        tokio::time::timeout(Duration::from_secs(5), self.0.recv()).await.expect("nothing reported").unwrap()
    }
}

#[tokio::test]
async fn the_status_is_followed_and_requests_reach_the_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    let daemon = start_daemon(tmp.path(), "alpha").await;
    let socket = tmp.path().join("clipsync.sock");
    let mut watched = Watched::start(&socket);

    let first = watched.next().await.expect("the daemon is running");
    assert_eq!(first.name, "alpha");
    assert!(!first.paused);

    assert_eq!(request(&socket, &Request::Pause).await.unwrap(), Reply::Ok);
    assert!(watched.next().await.unwrap().paused);

    daemon.shutdown().await;
    assert_eq!(watched.next().await, None, "a stopped daemon is reported as unreachable");
}

#[tokio::test]
async fn a_daemon_started_later_is_found() {
    let tmp = tempfile::tempdir().unwrap();
    let socket = tmp.path().join("clipsync.sock");
    let mut watched = Watched::start(&socket);
    assert_eq!(watched.next().await, None);

    let _daemon = start_daemon(tmp.path(), "late").await;
    assert_eq!(watched.next().await.expect("found once it runs").name, "late");
}

#[tokio::test]
async fn a_request_without_a_daemon_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let err = request(&tmp.path().join("clipsync.sock"), &Request::Resume).await.unwrap_err();
    assert!(format!("{err:#}").contains("is it running?"), "{err:#}");
}
