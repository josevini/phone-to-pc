//! Following a real daemon's status through its control socket, as the app does.

mod support;

use std::path::Path;
use std::time::Duration;

use clipsync_desktop::daemon::{rename, request, unpair, watch};
use clipsyncd::ipc::{Reply, Request, StatusView};
use support::Daemon;
use tokio::sync::mpsc;

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
    let daemon = Daemon::start(tmp.path(), "alpha").await;
    let socket = daemon.socket.clone();
    let mut watched = Watched::start(&socket);

    let first = watched.next().await.expect("the daemon is running");
    assert_eq!(first.name, "alpha");
    assert!(!first.paused);

    assert_eq!(request(&socket, &Request::Pause).await.unwrap(), Reply::Ok);
    assert!(watched.next().await.unwrap().paused);

    daemon.handle.shutdown().await;
    assert_eq!(watched.next().await, None, "a stopped daemon is reported as unreachable");
}

#[tokio::test]
async fn a_daemon_started_later_is_found() {
    let tmp = tempfile::tempdir().unwrap();
    let socket = tmp.path().join("clipsync.sock");
    let mut watched = Watched::start(&socket);
    assert_eq!(watched.next().await, None);

    let _daemon = Daemon::start(tmp.path(), "late").await;
    assert_eq!(watched.next().await.expect("found once it runs").name, "late");
}

#[tokio::test]
async fn a_request_without_a_daemon_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let err = request(&tmp.path().join("clipsync.sock"), &Request::Resume).await.unwrap_err();
    assert!(format!("{err:#}").contains("is it running?"), "{err:#}");
}

#[tokio::test]
async fn unpairing_forgets_the_device_on_both_sides() {
    let tmp = tempfile::tempdir().unwrap();
    let a = Daemon::start(&tmp.path().join("a"), "alpha").await;
    let b = Daemon::start(&tmp.path().join("b"), "beta").await;
    let uri = b.handle.start_pairing().await.unwrap().uri;
    a.handle.pair_with_uri(&uri).await.unwrap();
    let b_id = b.handle.status().await.id;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !a.handle.status().await.devices.iter().any(|d| d.connected) {
        assert!(tokio::time::Instant::now() < deadline, "never connected");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(unpair(&a.socket, &b_id.to_string()).await.unwrap(), "beta");
    assert!(a.handle.status().await.devices.is_empty());
    let err = unpair(&a.socket, &b_id.to_string()).await.unwrap_err();
    assert!(format!("{err:#}").contains("no paired device"), "{err:#}");
}

#[tokio::test]
async fn renaming_changes_the_name_or_says_why_not() {
    let tmp = tempfile::tempdir().unwrap();
    let a = Daemon::start(tmp.path(), "alpha").await;
    rename(&a.socket, "Meu PC").await.unwrap();
    assert_eq!(a.handle.status().await.name, "Meu PC");
    let err = rename(&a.socket, "").await.unwrap_err();
    assert!(format!("{err:#}").contains("1 to 64 bytes"), "{err:#}");
}
