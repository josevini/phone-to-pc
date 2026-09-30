//! Two engines driven against each other through the exported API only, as the Android app
//! drives one against a peer.

use std::sync::Arc;

use clipsync_ffi::{
    CloseReason, CoreError, Engine, EngineEvent, Intent, LocalChange, LocalDevice, Output, PairedDevice, Role,
    device_id_from_spki, format_sas,
};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn engine(id: &str, name: &str, paired: Vec<PairedDevice>) -> Arc<Engine> {
    let me = LocalDevice { id: id.into(), name: name.into(), platform: "android".into() };
    Engine::new(me, paired, 0).unwrap()
}

/// Everything an engine asked its host to do, besides sending bytes.
#[derive(Default)]
struct Seen {
    events: Vec<EngineEvent>,
    clipboard: Vec<String>,
    closed: Vec<u64>,
}

/// Engine `a` dialed engine `b`; connection 1 on both sides.
struct Pair {
    a: Arc<Engine>,
    b: Arc<Engine>,
    seen_a: Seen,
    seen_b: Seen,
}

impl Pair {
    fn connect(a: Arc<Engine>, b: Arc<Engine>, intent: Intent) -> Pair {
        a.connection_opened(1, Role::Dialer, B.into(), intent, 0).unwrap();
        b.connection_opened(1, Role::Acceptor, A.into(), Intent::Session, 0).unwrap();
        let mut pair = Pair { a, b, seen_a: Seen::default(), seen_b: Seen::default() };
        pair.pump();
        pair
    }

    /// Moves bytes between the engines until neither has anything left to do.
    fn pump(&mut self) {
        loop {
            let (from_a, from_b) = (self.a.poll_outputs(), self.b.poll_outputs());
            if from_a.is_empty() && from_b.is_empty() {
                return;
            }
            deliver(from_a, &self.b, &mut self.seen_a);
            deliver(from_b, &self.a, &mut self.seen_b);
        }
    }
}

fn deliver(outputs: Vec<Output>, to: &Engine, seen: &mut Seen) {
    for output in outputs {
        match output {
            Output::Send { conn, bytes } => to.bytes_received(conn, bytes, 0),
            Output::Close { conn } => {
                seen.closed.push(conn);
                to.connection_closed(conn);
            }
            Output::SetClipboard { text } => seen.clipboard.push(text),
            Output::Event { event } => seen.events.push(event),
        }
    }
}

#[test]
fn a_token_pairing_then_a_clip_reaches_the_other_device() {
    let (a, b) = (engine(A, "phone", vec![]), engine(B, "laptop", vec![]));
    let token = b.start_pairing(0);
    assert!(b.pairing_active(0));
    let mut pair = Pair::connect(a, b, Intent::PairToken { token });

    let laptop = PairedDevice { id: B.into(), name: "laptop".into() };
    assert!(pair.seen_a.events.contains(&EngineEvent::Paired { device: laptop.clone() }));
    assert!(pair.seen_a.events.contains(&EngineEvent::PeerConnected { peer: B.into(), name: "laptop".into() }));
    assert!(pair.seen_b.events.contains(&EngineEvent::PairingModeEnded));
    assert_eq!(pair.a.paired(), vec![laptop]);
    assert!(pair.a.is_connected(B.into()).unwrap());
    assert_eq!(pair.a.connected_peers(), vec![B.to_string()]);

    assert_eq!(pair.b.local_clipboard_changed("olá 👋".into(), 5), LocalChange::Sent { seq: 1, peers: 1 });
    pair.pump();
    assert_eq!(pair.seen_a.clipboard, vec!["olá 👋"]);
    assert_eq!(pair.a.lamport(), 1);
    assert!(pair.seen_b.events.contains(&EngineEvent::ClipDelivered { to: A.into(), applied: true }));
    assert!(pair.seen_a.events.contains(&EngineEvent::ClipReceived { from: B.into(), applied: true }));
}

#[test]
fn local_changes_report_why_nothing_was_sent() {
    let a = engine(A, "phone", vec![]);
    assert_eq!(a.local_clipboard_changed(String::new(), 0), LocalChange::Empty);
    assert_eq!(a.local_clipboard_changed("x".repeat(1024 * 1024 + 1), 0), LocalChange::TooLarge);
    assert_eq!(a.local_clipboard_changed("hi".into(), 0), LocalChange::Sent { seq: 1, peers: 0 });
    assert_eq!(a.local_clipboard_changed("hi".into(), 0), LocalChange::Unchanged);
}

#[test]
fn a_paused_engine_stays_connected_but_neither_sends_nor_applies() {
    let a = engine(A, "phone", vec![PairedDevice { id: B.into(), name: "laptop".into() }]);
    let b = engine(B, "laptop", vec![PairedDevice { id: A.into(), name: "phone".into() }]);
    let mut pair = Pair::connect(a, b, Intent::Session);
    pair.a.set_paused(true);
    assert!(pair.a.paused());

    assert_eq!(pair.a.local_clipboard_changed("private".into(), 0), LocalChange::Paused);
    pair.b.local_clipboard_changed("from laptop".into(), 0);
    pair.pump();
    assert!(pair.seen_a.clipboard.is_empty() && pair.seen_b.clipboard.is_empty());
    assert!(pair.seen_b.events.contains(&EngineEvent::ClipDelivered { to: A.into(), applied: false }));
    assert!(pair.a.is_connected(B.into()).unwrap());

    pair.a.set_paused(false);
    assert!(!pair.a.paused());
    assert_eq!(pair.a.local_clipboard_changed("shared".into(), 0), LocalChange::Sent { seq: 2, peers: 1 });
}

#[test]
fn a_sas_pairing_shows_the_same_code_on_both_sides_and_needs_both_confirmations() {
    let (a, b) = (engine(A, "phone", vec![]), engine(B, "laptop", vec![]));
    b.start_pairing(0);
    let mut pair = Pair::connect(a, b, Intent::PairSas);
    let code = |seen: &Seen| {
        seen.events.iter().find_map(|e| match e {
            EngineEvent::PairingCode { conn, code, name, .. } => Some((*conn, *code, name.clone())),
            _ => None,
        })
    };
    let (conn_a, code_a, name_a) = code(&pair.seen_a).unwrap();
    let (conn_b, code_b, name_b) = code(&pair.seen_b).unwrap();
    assert_eq!(code_a, code_b);
    assert_eq!((name_a.as_str(), name_b.as_str()), ("laptop", "phone"));

    pair.a.confirm_pairing(conn_a, true, 0);
    pair.b.confirm_pairing(conn_b, true, 0);
    pair.pump();
    assert!(pair.a.is_connected(B.into()).unwrap());
    assert!(pair.b.is_connected(A.into()).unwrap());
}

#[test]
fn stopping_pairing_mode_refuses_unknown_devices() {
    let (a, b) = (engine(A, "phone", vec![]), engine(B, "laptop", vec![]));
    let token = b.start_pairing(0);
    b.stop_pairing();
    assert!(!b.pairing_active(0));
    let pair = Pair::connect(a, b, Intent::PairToken { token });
    let reason = pair.seen_a.events.iter().find_map(|e| match e {
        EngineEvent::ConnectionClosed { reason, .. } => Some(reason.clone()),
        _ => None,
    });
    assert_eq!(reason, Some(CloseReason::RemoteError { code: "not_paired".into() }));
    assert_eq!(pair.seen_a.closed, vec![1]);
}

#[test]
fn unpairing_tells_the_peer_and_timers_close_silent_connections() {
    let a = engine(A, "phone", vec![PairedDevice { id: B.into(), name: "laptop".into() }]);
    let b = engine(B, "laptop", vec![PairedDevice { id: A.into(), name: "phone".into() }]);
    let mut pair = Pair::connect(a, b, Intent::Session);
    assert!(pair.a.is_connected(B.into()).unwrap());

    pair.a.unpair(B.into()).unwrap();
    pair.pump();
    assert!(pair.seen_b.events.contains(&EngineEvent::Unpaired { peer: A.into() }));
    assert!(pair.seen_a.events.contains(&EngineEvent::PeerDisconnected { peer: B.into() }));

    let c = engine(A, "phone", vec![PairedDevice { id: B.into(), name: "laptop".into() }]);
    c.connection_opened(7, Role::Dialer, B.into(), Intent::Session, 0).unwrap();
    c.tick(90_000);
    assert!(c.poll_outputs().contains(&Output::Close { conn: 7 }));
}

#[test]
fn malformed_ids_and_tokens_are_errors() {
    let bad = || CoreError::InvalidDeviceId { id: "nope".into() };
    let me = LocalDevice { id: "nope".into(), name: "phone".into(), platform: "android".into() };
    assert_eq!(Engine::new(me, vec![], 0).err(), Some(bad()));
    let me = LocalDevice { id: A.into(), name: "phone".into(), platform: "android".into() };
    let paired = vec![PairedDevice { id: "nope".into(), name: "x".into() }];
    assert_eq!(Engine::new(me, paired, 0).err(), Some(bad()));

    let a = engine(A, "phone", vec![]);
    assert_eq!(a.connection_opened(1, Role::Dialer, "nope".into(), Intent::Session, 0), Err(bad()));
    let intent = Intent::PairToken { token: "zz".into() };
    assert_eq!(a.connection_opened(1, Role::Dialer, B.into(), intent, 0), Err(CoreError::InvalidToken));
    assert_eq!(a.is_connected("nope".into()), Err(bad()));
    assert_eq!(a.unpair("nope".into()), Err(bad()));
}

#[test]
fn identity_helpers_match_the_core() {
    // SPKI and device ID from the spec's known-answer tests.
    let spki = hex(
        "3059301306072a8648ce3d020106082a8648ce3d030107034200041a8666b545ff4f6d74eeaa4c2ab0cb1514e25631499413a079c3\
         ec11b5f8b851b6ceb51c0710f78400664b9129c4b507042231e55fc354607a09820d0aea7186",
    );
    assert_eq!(device_id_from_spki(spki), "86224755c0ff3b3b412a5da3ef12466cb12c48326e3454c02687f2cc88771027");
    assert_eq!(format_sas(37_725), "037 725");
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}
