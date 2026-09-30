//! Pairing from the app against real daemons: showing this device's code, pasting another's link, comparing codes.

mod support;

use std::time::Duration;

use clipsync_core::engine::Event;
use clipsync_desktop::pairing::{Pairing, PairingEvent};
use clipsyncd::daemon::DaemonEvent;
use support::Daemon;
use tokio::sync::mpsc;

/// What a pairing reported, in order.
struct Events(mpsc::UnboundedReceiver<PairingEvent>);

impl Events {
    fn new() -> (impl FnMut(PairingEvent) + Send + 'static, Events) {
        let (tx, rx) = mpsc::unbounded_channel();
        (move |event| drop(tx.send(event)), Events(rx))
    }

    async fn next(&mut self) -> PairingEvent {
        tokio::time::timeout(Duration::from_secs(5), self.0.recv()).await.expect("nothing reported").unwrap()
    }
}

async fn two_daemons() -> (tempfile::TempDir, Daemon, Daemon) {
    let tmp = tempfile::tempdir().unwrap();
    let a = Daemon::start(&tmp.path().join("a"), "alpha").await;
    let b = Daemon::start(&tmp.path().join("b"), "beta").await;
    (tmp, a, b)
}

async fn eventually(what: &str, mut condition: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition().await {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_device_that_scans_the_code_is_paired() {
    let (_tmp, a, b) = two_daemons().await;
    let (on_event, mut events) = Events::new();
    let (invite, _pairing) = Pairing::show_code(&a.socket, on_event).await.unwrap();
    assert!(invite.uri.starts_with("clipsync://pair?"));
    assert_eq!(invite.expires_in_s, 120);
    assert_eq!(invite.qr.modules.len(), invite.qr.size * invite.qr.size);

    b.handle.pair_with_uri(&invite.uri).await.unwrap();
    assert_eq!(events.next().await, PairingEvent::Paired { name: "beta".into() });
}

#[tokio::test]
async fn a_pasted_link_pairs_with_the_device_that_shows_it() {
    let (_tmp, a, b) = two_daemons().await;
    let uri = b.handle.start_pairing().await.unwrap().uri;
    let (on_event, mut events) = Events::new();
    let _pairing = Pairing::with_link(&a.socket, &uri, on_event).await.unwrap();
    assert_eq!(events.next().await, PairingEvent::Paired { name: "beta".into() });
}

#[tokio::test]
async fn a_device_pairing_by_address_needs_the_code_confirmed_here() {
    let (_tmp, a, b) = two_daemons().await;
    let (on_event, mut events) = Events::new();
    let (_invite, pairing) = Pairing::show_code(&a.socket, on_event).await.unwrap();
    let mut b_events = b.handle.subscribe();
    b.handle.pair_with_address(a.addr().await).await.unwrap();

    let PairingEvent::Code { name, code } = events.next().await else { panic!("no code to confirm") };
    assert_eq!(name, "beta");
    let b_code = loop {
        if let DaemonEvent::Engine(Event::PairingCode { conn, code, .. }) = b_events.recv().await.unwrap() {
            b.handle.confirm_pairing(conn, true).await;
            break code;
        }
    };
    assert_eq!(code, format!("{:03} {:03}", b_code / 1000, b_code % 1000));
    pairing.confirm(true);
    assert_eq!(events.next().await, PairingEvent::Paired { name: "beta".into() });
}

#[tokio::test]
async fn a_rejected_code_pairs_nothing() {
    let (_tmp, a, b) = two_daemons().await;
    let (on_event, mut events) = Events::new();
    let (_invite, pairing) = Pairing::show_code(&a.socket, on_event).await.unwrap();
    b.handle.pair_with_address(a.addr().await).await.unwrap();
    assert!(matches!(events.next().await, PairingEvent::Code { .. }));
    pairing.confirm(false);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(a.handle.status().await.devices.is_empty());
    assert!(b.handle.status().await.devices.is_empty());
}

#[tokio::test]
async fn closing_the_pairing_closes_pairing_mode() {
    let (_tmp, a, _b) = two_daemons().await;
    let (on_event, _events) = Events::new();
    let (_invite, pairing) = Pairing::show_code(&a.socket, on_event).await.unwrap();
    assert!(a.handle.status().await.pairing);
    drop(pairing);
    eventually("pairing mode closed", async || !a.handle.status().await.pairing).await;
}

#[tokio::test]
async fn pairing_mode_that_ends_without_a_device_is_reported() {
    let (_tmp, a, _b) = two_daemons().await;
    let (on_event, mut events) = Events::new();
    let (_invite, _pairing) = Pairing::show_code(&a.socket, on_event).await.unwrap();
    a.handle.stop_pairing();
    assert_eq!(events.next().await, PairingEvent::Ended);
}

#[tokio::test]
async fn a_link_that_no_longer_works_fails_with_a_reason() {
    let (_tmp, a, b) = two_daemons().await;
    let uri = b.handle.start_pairing().await.unwrap().uri;
    let token = uri.split("token=").nth(1).unwrap();
    let forged = uri.replace(token, &"0".repeat(32));
    let (on_event, mut events) = Events::new();
    let _pairing = Pairing::with_link(&a.socket, &forged, on_event).await.unwrap();
    let PairingEvent::Failed { reason } = events.next().await else { panic!("not a failure") };
    assert!(reason.contains("no longer valid"), "{reason}");
}

#[tokio::test]
async fn something_that_is_not_a_pairing_link_is_an_error() {
    let (_tmp, a, _b) = two_daemons().await;
    let (on_event, _events) = Events::new();
    assert!(Pairing::with_link(&a.socket, "https://example.com", on_event).await.is_err());
}
