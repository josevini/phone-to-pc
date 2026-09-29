//! The Wayland clipboard backend against a real, headless compositor.

mod support;

use std::sync::mpsc;
use std::time::Duration;

use clipsyncd::clipboard::{ClipboardEvent, SkipReason, WaylandClipboard};
use support::sway::Sway;

struct Backend {
    clipboard: WaylandClipboard,
    events: mpsc::Receiver<ClipboardEvent>,
}

impl Backend {
    fn on(sway: &Sway) -> Backend {
        let (tx, events) = mpsc::channel();
        let clipboard = WaylandClipboard::spawn_on(Some(sway.socket()), move |event| {
            let _ = tx.send(event);
        })
        .unwrap();
        Backend { clipboard, events }
    }

    /// The next event, skipping the report of what was there at startup unless asked for.
    fn next(&self) -> ClipboardEvent {
        loop {
            let event = self.events.recv_timeout(Duration::from_secs(5)).expect("no clipboard event");
            match event {
                ClipboardEvent::Text { initial: true, .. } | ClipboardEvent::Skipped { initial: true, .. } => continue,
                other => return other,
            }
        }
    }

    fn quiet_for(&self, time: Duration) -> Vec<ClipboardEvent> {
        std::iter::from_fn(|| self.events.recv_timeout(time).ok()).collect()
    }
}

fn skipped(reason: SkipReason) -> ClipboardEvent {
    ClipboardEvent::Skipped { reason, initial: false }
}

#[test]
fn reports_text_copied_by_another_client() {
    let sway = Sway::start();
    let backend = Backend::on(&sway);
    sway.copy(&[], "olá 👋".as_bytes());
    assert_eq!(backend.next(), ClipboardEvent::Text { text: "olá 👋".into(), initial: false });
}

#[test]
fn reports_what_was_already_copied_as_initial() {
    let sway = Sway::start();
    sway.copy(&[], b"already there");
    let backend = Backend::on(&sway);
    let first = backend.events.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first, ClipboardEvent::Text { text: "already there".into(), initial: true });
}

#[test]
fn an_empty_clipboard_at_startup_is_reported_as_initial_and_empty() {
    let sway = Sway::start();
    let backend = Backend::on(&sway);
    let first = backend.events.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first, ClipboardEvent::Skipped { reason: SkipReason::Empty, initial: true });
}

#[test]
fn serves_its_text_to_other_clients_and_recognises_it_as_its_own() {
    let sway = Sway::start();
    let backend = Backend::on(&sway);
    let big = "x".repeat(300 * 1024); // more than a pipe buffer
    backend.clipboard.set_text(big.clone());
    assert_eq!(backend.next(), skipped(SkipReason::Own));
    let pasted = sway.paste();
    assert!(pasted.status.success());
    assert_eq!(pasted.stdout.len(), big.len());
    assert!(!backend.quiet_for(Duration::from_millis(300)).iter().any(|e| matches!(e, ClipboardEvent::Text { .. })));
}

#[test]
fn reports_losing_the_clipboard_to_another_client() {
    let sway = Sway::start();
    let backend = Backend::on(&sway);
    backend.clipboard.set_text("mine".into());
    assert_eq!(backend.next(), skipped(SkipReason::Own));
    sway.copy(&[], b"theirs");
    let events = [backend.next(), backend.next()];
    assert!(events.contains(&ClipboardEvent::OwnershipLost), "{events:?}");
    assert!(events.contains(&ClipboardEvent::Text { text: "theirs".into(), initial: false }), "{events:?}");
}

#[test]
fn skips_what_must_not_be_synced() {
    let sway = Sway::start();
    let backend = Backend::on(&sway);
    sway.copy(&["--type", "x-kde-passwordManagerHint"], b"secret");
    assert_eq!(backend.next(), skipped(SkipReason::Sensitive));
    sway.copy(&["--type", "image/png"], b"\x89PNG\r\n\x1a\nnot really");
    assert_eq!(backend.next(), skipped(SkipReason::NotText));
    sway.copy(&[], &vec![b'a'; 1024 * 1024 + 1]);
    assert_eq!(backend.next(), skipped(SkipReason::TooLarge));
    sway.copy(&["--type", "text/plain"], b"ab\xffcd");
    assert_eq!(backend.next(), skipped(SkipReason::InvalidUtf8));
    assert!(sway.command("wl-copy").arg("--clear").status().unwrap().success());
    assert_eq!(backend.next(), skipped(SkipReason::Empty));
}

#[test]
fn ignores_the_primary_selection() {
    let sway = Sway::start();
    let backend = Backend::on(&sway);
    let _ = backend.quiet_for(Duration::from_millis(200));
    sway.copy(&["--primary"], b"middle click");
    assert_eq!(backend.quiet_for(Duration::from_millis(500)), vec![]);
}

#[test]
fn reports_when_the_compositor_goes_away() {
    let mut sway = Sway::start();
    let backend = Backend::on(&sway);
    let _ = backend.quiet_for(Duration::from_millis(200));
    sway.stop();
    assert!(matches!(backend.next(), ClipboardEvent::Closed(_)));
}

#[test]
fn fails_to_start_without_a_compositor() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("wayland-9");
    assert!(WaylandClipboard::spawn_on(Some(&missing), |_| {}).is_err());
}

#[test]
fn the_private_marker_type_carries_no_data() {
    let sway = Sway::start();
    let backend = Backend::on(&sway);
    backend.clipboard.set_text("the text".into());
    assert_eq!(backend.next(), skipped(SkipReason::Own));
    let types = sway.command("wl-paste").arg("--list-types").output().unwrap();
    let types = String::from_utf8(types.stdout).unwrap();
    let marker = types.lines().find(|t| t.starts_with("application/x-clipsync-source-")).expect("no marker type");
    let pasted = sway.command("wl-paste").args(["--no-newline", "--type", marker]).output().unwrap();
    assert!(pasted.stdout.is_empty(), "the marker type served {:?}", String::from_utf8_lossy(&pasted.stdout));
}
