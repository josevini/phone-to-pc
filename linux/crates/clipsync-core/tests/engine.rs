//! Behaviour of the session engine (spec §5–§7), driven through its public API only.

use clipsync_core::engine::{
    CloseReason, ConnId, Engine, Event, Intent, KEEPALIVE_IDLE_MS, LocalChange, LocalDevice, Output, PAIRING_WINDOW_MS,
    PairedDevice, Role, TIMEOUT_MS,
};
use clipsync_core::frame::{self, FrameDecoder};
use clipsync_core::message::{Hello, Message, PROTO_VERSION};
use clipsync_core::{DeviceId, Hex, Hex16};

fn id(byte: u8) -> DeviceId {
    DeviceId(Hex([byte; 32]))
}

fn device(byte: u8, name: &str) -> LocalDevice {
    LocalDevice { id: id(byte), name: name.into(), platform: "linux".into() }
}

fn paired(byte: u8, name: &str) -> PairedDevice {
    PairedDevice { id: id(byte), name: name.into() }
}

/// What one engine did, drained from its outputs.
#[derive(Default)]
struct Seen {
    events: Vec<Event>,
    clipboard: Vec<String>,
    closed: Vec<ConnId>,
}

/// Two engines, A and B, joined by any number of connections.
struct Net {
    a: Engine,
    b: Engine,
    /// (conn id on A, conn id on B)
    links: Vec<(ConnId, ConnId)>,
    seen_a: Seen,
    seen_b: Seen,
    now: u64,
}

const A: u8 = 0x11;
const B: u8 = 0xee;

impl Net {
    fn new(a_knows_b: bool, b_knows_a: bool) -> Self {
        let a = Engine::new(device(A, "alpha"), if a_knows_b { vec![paired(B, "beta")] } else { vec![] }, 0);
        let b = Engine::new(device(B, "beta"), if b_knows_a { vec![paired(A, "alpha")] } else { vec![] }, 0);
        Net { a, b, links: vec![], seen_a: Seen::default(), seen_b: Seen::default(), now: 1_000 }
    }

    fn paired() -> Self {
        Net::new(true, true)
    }

    /// A dials B with `intent`; returns the link index.
    fn a_dials_b(&mut self, intent: Intent) -> usize {
        let conn = self.links.len() as ConnId + 1;
        self.links.push((conn, conn));
        self.a.connection_opened(conn, Role::Dialer, id(B), intent, self.now);
        self.b.connection_opened(conn, Role::Acceptor, id(A), Intent::Session, self.now);
        self.pump();
        self.links.len() - 1
    }

    /// B dials A (always a plain session); returns the link index.
    fn b_dials_a(&mut self) -> usize {
        let conn = self.links.len() as ConnId + 1;
        self.links.push((conn, conn));
        self.b.connection_opened(conn, Role::Dialer, id(A), Intent::Session, self.now);
        self.a.connection_opened(conn, Role::Acceptor, id(B), Intent::Session, self.now);
        self.pump();
        self.links.len() - 1
    }

    /// Delivers every pending output until both engines are quiet.
    fn pump(&mut self) {
        loop {
            let mut moved = false;
            while let Some(out) = self.a.poll_output() {
                moved = true;
                match out {
                    Output::Send { conn, bytes } => {
                        let (_, peer_conn) = *self.links.iter().find(|(a, _)| *a == conn).unwrap();
                        if !self.seen_b.closed.contains(&peer_conn) {
                            self.b.bytes_received(peer_conn, &bytes, self.now);
                        }
                    }
                    Output::Close { conn } => {
                        self.seen_a.closed.push(conn);
                        let (_, peer_conn) = *self.links.iter().find(|(a, _)| *a == conn).unwrap();
                        if !self.seen_b.closed.contains(&peer_conn) {
                            self.seen_b.closed.push(peer_conn);
                            self.b.connection_closed(peer_conn);
                        }
                    }
                    Output::SetClipboard { text } => self.seen_a.clipboard.push(text),
                    Output::Event(e) => self.seen_a.events.push(e),
                }
            }
            while let Some(out) = self.b.poll_output() {
                moved = true;
                match out {
                    Output::Send { conn, bytes } => {
                        let (peer_conn, _) = *self.links.iter().find(|(_, b)| *b == conn).unwrap();
                        if !self.seen_a.closed.contains(&peer_conn) {
                            self.a.bytes_received(peer_conn, &bytes, self.now);
                        }
                    }
                    Output::Close { conn } => {
                        self.seen_b.closed.push(conn);
                        let (peer_conn, _) = *self.links.iter().find(|(_, b)| *b == conn).unwrap();
                        if !self.seen_a.closed.contains(&peer_conn) {
                            self.seen_a.closed.push(peer_conn);
                            self.a.connection_closed(peer_conn);
                        }
                    }
                    Output::SetClipboard { text } => self.seen_b.clipboard.push(text),
                    Output::Event(e) => self.seen_b.events.push(e),
                }
            }
            if !moved {
                break;
            }
        }
    }

    fn tick(&mut self, advance_ms: u64) {
        self.now += advance_ms;
        self.a.tick(self.now);
        self.b.tick(self.now);
        self.pump();
    }
}

fn has(events: &[Event], want: &Event) -> bool {
    events.contains(want)
}

fn close_reasons(events: &[Event]) -> Vec<CloseReason> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ConnectionClosed { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------- sessions (spec §7)

#[test]
fn paired_devices_establish_a_session() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session);
    assert!(has(&net.seen_a.events, &Event::PeerConnected { peer: id(B), name: "beta".into() }));
    assert!(has(&net.seen_b.events, &Event::PeerConnected { peer: id(A), name: "alpha".into() }));
    assert!(net.a.is_connected(&id(B)) && net.b.is_connected(&id(A)));
}

#[test]
fn acceptor_rejects_an_unknown_peer_outside_pairing_mode() {
    let mut net = Net::new(true, false);
    net.a_dials_b(Intent::Session);
    assert_eq!(close_reasons(&net.seen_b.events), vec![CloseReason::NotPaired]);
    assert_eq!(close_reasons(&net.seen_a.events), vec![CloseReason::RemoteError { code: "not_paired".into() }]);
    assert!(!net.a.is_connected(&id(B)));
}

/// Feeds raw messages into one engine and collects what it answers.
struct Probe {
    engine: Engine,
    outputs: Vec<Output>,
}

impl Probe {
    fn acceptor(knows_peer: bool) -> Self {
        let paired = if knows_peer { vec![paired(A, "alpha")] } else { vec![] };
        let mut engine = Engine::new(device(B, "beta"), paired, 0);
        engine.connection_opened(1, Role::Acceptor, id(A), Intent::Session, 0);
        let mut probe = Probe { engine, outputs: vec![] };
        probe.drain();
        probe
    }

    fn drain(&mut self) {
        while let Some(out) = self.engine.poll_output() {
            self.outputs.push(out);
        }
    }

    fn send(&mut self, msg: &Message) {
        self.engine.bytes_received(1, &frame::encode(msg), 0);
        self.drain();
    }

    fn sent_messages(&self) -> Vec<Message> {
        let mut decoder = FrameDecoder::new();
        let mut out = vec![];
        for o in &self.outputs {
            if let Output::Send { bytes, .. } = o {
                decoder.push(bytes);
                while let Some(frame::Decoded::Message(m)) = decoder.next_frame().unwrap() {
                    out.push(m);
                }
            }
        }
        out
    }

    fn closed(&self) -> bool {
        self.outputs.iter().any(|o| matches!(o, Output::Close { conn: 1 }))
    }

    fn close_reason(&self) -> Option<CloseReason> {
        self.outputs.iter().find_map(|o| match o {
            Output::Event(Event::ConnectionClosed { reason, .. }) => Some(reason.clone()),
            _ => None,
        })
    }
}

fn hello(from: u8, proto: u32) -> Message {
    Message::Hello(Hello {
        proto,
        id: id(from),
        name: "alpha".into(),
        platform: "linux".into(),
        caps: vec!["text".into()],
        seq: 0,
    })
}

#[test]
fn both_sides_send_hello_first() {
    let probe = Probe::acceptor(true);
    let sent = probe.sent_messages();
    assert!(matches!(&sent[..], [Message::Hello(h)] if h.id == id(B) && h.proto == PROTO_VERSION && h.name == "beta"));
}

#[test]
fn rejects_another_protocol_version() {
    let mut probe = Probe::acceptor(true);
    probe.send(&hello(A, 2));
    assert!(probe.closed());
    assert_eq!(probe.close_reason(), Some(CloseReason::UnsupportedVersion));
    assert!(probe.sent_messages().iter().any(|m| matches!(m, Message::Error(e) if e.code == "unsupported_version")));
}

#[test]
fn rejects_a_hello_whose_id_differs_from_the_certificate() {
    let mut probe = Probe::acceptor(true);
    probe.send(&hello(0x42, PROTO_VERSION));
    assert_eq!(probe.close_reason(), Some(CloseReason::IdentityMismatch));
    assert!(probe.sent_messages().iter().any(|m| matches!(m, Message::Error(e) if e.code == "identity_mismatch")));
}

#[test]
fn a_malformed_frame_closes_the_connection() {
    let mut probe = Probe::acceptor(true);
    probe.engine.bytes_received(1, &[0, 0, 0, 0], 0);
    probe.drain();
    assert_eq!(probe.close_reason(), Some(CloseReason::ProtocolError));
}

#[test]
fn a_clip_before_the_session_is_established_is_a_protocol_error() {
    let mut probe = Probe::acceptor(false);
    let text = "early".to_string();
    probe.send(&Message::Clip(clipsync_core::message::Clip {
        id: Hex16::random(),
        origin: id(A),
        seq: 1,
        ts: 0,
        mime: clipsync_core::clip::TEXT_MIME.into(),
        sha256: clipsync_core::clip::text_digest(&text),
        text,
    }));
    assert!(probe.closed());
}

#[test]
fn hello_seq_raises_the_lamport_counter() {
    let mut probe = Probe::acceptor(true);
    let mut h = hello(A, PROTO_VERSION);
    if let Message::Hello(h) = &mut h {
        h.seq = 41;
    }
    probe.send(&h);
    assert_eq!(probe.engine.lamport(), 41);
}

// ---------------------------------------------------------------- clips (spec §6)

#[test]
fn a_local_copy_reaches_the_peer_and_is_acknowledged() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session);
    let outcome = net.a.local_clipboard_changed("olá".into(), net.now);
    assert_eq!(outcome, LocalChange::Sent { seq: 1, peers: 1 });
    net.pump();
    assert_eq!(net.seen_b.clipboard, vec!["olá".to_string()]);
    assert!(has(&net.seen_b.events, &Event::ClipReceived { from: id(A), applied: true }));
    assert!(has(&net.seen_a.events, &Event::ClipDelivered { to: id(B), applied: true }));
}

#[test]
fn the_echo_of_an_applied_clip_is_not_sent_back() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session);
    net.a.local_clipboard_changed("olá".into(), net.now);
    net.pump();
    assert_eq!(net.seen_b.clipboard, vec!["olá".to_string()]);
    // B's clipboard watcher reports the text the engine just wrote.
    assert_eq!(net.b.local_clipboard_changed("olá".into(), net.now), LocalChange::Unchanged);
}

#[test]
fn local_changes_without_peers_are_counted_but_not_sent() {
    let mut engine = Engine::new(device(A, "alpha"), vec![], 7);
    assert_eq!(engine.local_clipboard_changed("x".into(), 0), LocalChange::Sent { seq: 8, peers: 0 });
    assert_eq!(engine.local_clipboard_changed("".into(), 0), LocalChange::Empty);
    let big = "a".repeat(clipsync_core::clip::MAX_TEXT_LEN + 1);
    assert_eq!(engine.local_clipboard_changed(big, 0), LocalChange::TooLarge);
    assert_eq!(engine.lamport(), 8);
}

#[test]
fn a_stale_clip_is_acknowledged_as_not_applied() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session);
    net.b.local_clipboard_changed("newer".into(), net.now); // B: seq 1
    net.b.local_clipboard_changed("newest".into(), net.now); // B: seq 2
    net.pump();
    net.seen_a.clipboard.clear();
    // A has seen seq 2 from B; its own next clip gets seq 3 and wins.
    assert_eq!(net.a.local_clipboard_changed("mine".into(), net.now), LocalChange::Sent { seq: 3, peers: 1 });
    net.pump();
    assert_eq!(net.seen_b.clipboard, vec!["mine".to_string()]);
}

#[test]
fn a_paused_device_stays_connected_but_neither_sends_nor_applies() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session);
    assert!(!net.b.paused());
    net.b.set_paused(true);
    assert!(net.b.paused());

    assert_eq!(net.b.local_clipboard_changed("private".into(), net.now), LocalChange::Paused);
    net.pump();
    assert!(net.seen_a.clipboard.is_empty());

    net.a.local_clipboard_changed("from alpha".into(), net.now);
    net.pump();
    assert!(net.seen_b.clipboard.is_empty());
    assert!(has(&net.seen_b.events, &Event::ClipReceived { from: id(A), applied: false }));
    assert!(has(&net.seen_a.events, &Event::ClipDelivered { to: id(B), applied: false }));
    assert!(net.a.is_connected(&id(B)) && net.b.is_connected(&id(A)));

    net.b.set_paused(false);
    assert_eq!(net.b.local_clipboard_changed("shared".into(), net.now), LocalChange::Sent { seq: 2, peers: 1 });
    net.pump();
    assert_eq!(net.seen_a.clipboard, vec!["shared".to_string()]);
}

#[test]
fn a_renamed_device_introduces_itself_with_its_new_name() {
    let mut net = Net::paired();
    net.a.set_name("renamed".into());
    net.a_dials_b(Intent::Session);
    // B updates the name it keeps for A, as for any hello with a new name.
    assert!(has(&net.seen_b.events, &Event::Paired { device: paired(A, "renamed") }));
    assert!(has(&net.seen_b.events, &Event::PeerConnected { peer: id(A), name: "renamed".into() }));
}

// ---------------------------------------------------------------- keepalive (spec §7.1)

#[test]
fn idle_connections_are_pinged_and_dead_ones_closed() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session);
    // Pings keep a live connection open well past the timeout.
    for _ in 0..10 {
        net.tick(KEEPALIVE_IDLE_MS);
    }
    assert!(net.a.is_connected(&id(B)));

    // Now B stops answering: A times out.
    let mut a = Engine::new(device(A, "alpha"), vec![paired(B, "beta")], 0);
    a.connection_opened(1, Role::Dialer, id(B), Intent::Session, 0);
    let mut hello_b = frame::encode(&Message::Hello(Hello {
        proto: PROTO_VERSION,
        id: id(B),
        name: "beta".into(),
        platform: "linux".into(),
        caps: vec!["text".into()],
        seq: 0,
    }));
    a.bytes_received(1, &hello_b, 0);
    hello_b.clear();
    a.tick(TIMEOUT_MS + 1);
    let outputs: Vec<Output> = std::iter::from_fn(|| a.poll_output()).collect();
    assert!(outputs.contains(&Output::Close { conn: 1 }));
    assert!(outputs.contains(&Output::Event(Event::PeerDisconnected { peer: id(B) })));
    assert!(outputs.contains(&Output::Event(Event::ConnectionClosed {
        conn: 1,
        peer: id(B),
        reason: CloseReason::Timeout
    })));
}

// ---------------------------------------------------------------- duplicate connections (spec §7.1)

#[test]
fn duplicate_connections_keep_the_one_dialed_by_the_smaller_id() {
    let mut net = Net::paired();
    net.b_dials_a(); // link 0: dialer B (larger id) — must lose
    net.a_dials_b(Intent::Session); // link 1: dialer A (smaller id) — must win
    let (loser_a, loser_b) = net.links[0];
    assert!(net.seen_a.closed.contains(&loser_a) && net.seen_b.closed.contains(&loser_b));
    let (winner_a, winner_b) = net.links[1];
    assert!(!net.seen_a.closed.contains(&winner_a) && !net.seen_b.closed.contains(&winner_b));
    assert!(net.a.is_connected(&id(B)) && net.b.is_connected(&id(A)));
    // The peer was reported connected once, and never disconnected.
    let connected = net.seen_a.events.iter().filter(|e| matches!(e, Event::PeerConnected { .. })).count();
    assert_eq!(connected, 1);
    assert!(!net.seen_a.events.iter().any(|e| matches!(e, Event::PeerDisconnected { .. })));
    assert!(close_reasons(&net.seen_a.events).contains(&CloseReason::Duplicate));
}

// ---------------------------------------------------------------- pairing with a token (spec §7.2)

#[test]
fn token_pairing_pairs_both_devices() {
    let mut net = Net::new(false, false);
    let token = net.b.start_pairing(net.now);
    net.a_dials_b(Intent::PairToken { token });
    assert!(has(&net.seen_a.events, &Event::Paired { device: paired(B, "beta") }));
    assert!(has(&net.seen_b.events, &Event::Paired { device: paired(A, "alpha") }));
    assert!(has(&net.seen_b.events, &Event::PairingModeEnded));
    assert!(net.a.is_connected(&id(B)) && net.b.is_connected(&id(A)));
    assert_eq!(net.a.paired(), vec![paired(B, "beta")]);
}

#[test]
fn a_wrong_token_is_rejected_and_three_failures_end_pairing_mode() {
    let mut net = Net::new(false, false);
    net.b.start_pairing(net.now);
    for attempt in 1..=3 {
        net.a_dials_b(Intent::PairToken { token: Hex([attempt; 16]) });
        assert!(close_reasons(&net.seen_b.events).contains(&CloseReason::BadToken));
    }
    assert!(has(&net.seen_b.events, &Event::PairingModeEnded));
    assert!(net.a.paired().is_empty() && net.b.paired().is_empty());
}

#[test]
fn pairing_requests_outside_pairing_mode_are_refused() {
    // B knows A, so the hello succeeds and the pair_request itself is refused (spec §7.2).
    // An unknown dialer is refused earlier, with not_paired (spec §7 step 4).
    let mut net = Net::new(false, true);
    net.a_dials_b(Intent::PairToken { token: Hex([1; 16]) });
    assert!(close_reasons(&net.seen_a.events).contains(&CloseReason::RemoteError { code: "pairing_closed".into() }));
}

#[test]
fn pairing_mode_expires() {
    let mut net = Net::new(false, false);
    let token = net.b.start_pairing(net.now);
    net.tick(PAIRING_WINDOW_MS + 1);
    assert!(has(&net.seen_b.events, &Event::PairingModeEnded));
    net.a_dials_b(Intent::PairToken { token });
    assert!(net.b.paired().is_empty());
}

// ---------------------------------------------------------------- pairing by comparing codes (spec §7.3)

fn codes(events: &[Event]) -> Vec<(ConnId, u32)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::PairingCode { conn, code, .. } => Some((*conn, *code)),
            _ => None,
        })
        .collect()
}

#[test]
fn sas_pairing_shows_the_same_code_and_pairs_after_both_accept() {
    let mut net = Net::new(false, false);
    net.b.start_pairing(net.now);
    net.a_dials_b(Intent::PairSas);
    let (conn_a, code_a) = codes(&net.seen_a.events)[0];
    let (conn_b, code_b) = codes(&net.seen_b.events)[0];
    assert_eq!(code_a, code_b);
    assert!(code_a < 1_000_000);

    net.a.confirm_pairing(conn_a, true, net.now);
    net.pump();
    assert!(net.a.paired().is_empty(), "A must wait for B's decision too");
    net.b.confirm_pairing(conn_b, true, net.now);
    net.pump();
    assert_eq!(net.a.paired(), vec![paired(B, "beta")]);
    assert_eq!(net.b.paired(), vec![paired(A, "alpha")]);
    assert!(net.a.is_connected(&id(B)));
}

#[test]
fn sas_pairing_stops_when_either_side_rejects() {
    let mut net = Net::new(false, false);
    net.b.start_pairing(net.now);
    net.a_dials_b(Intent::PairSas);
    let (conn_a, _) = codes(&net.seen_a.events)[0];
    net.a.confirm_pairing(conn_a, true, net.now);
    let (conn_b, _) = codes(&net.seen_b.events)[0];
    net.b.confirm_pairing(conn_b, false, net.now);
    net.pump();
    assert!(net.a.paired().is_empty() && net.b.paired().is_empty());
    assert!(close_reasons(&net.seen_b.events).contains(&CloseReason::RejectedLocally));
    assert!(close_reasons(&net.seen_a.events).contains(&CloseReason::RejectedByPeer));
}

#[test]
fn a_reveal_that_does_not_match_the_commitment_is_a_protocol_error() {
    use clipsync_core::message::{PairMethod, PairNonce, PairRequest};
    let mut engine = Engine::new(device(B, "beta"), vec![], 0);
    engine.start_pairing(0);
    engine.connection_opened(1, Role::Acceptor, id(A), Intent::Session, 0);
    let commit = clipsync_core::pairing::sas_commit(&Hex([7; 32]));
    for msg in [
        hello(A, PROTO_VERSION),
        Message::PairRequest(PairRequest { method: PairMethod::Sas, token: None, commit: Some(commit) }),
        Message::PairReveal(PairNonce { nonce: Hex([8; 32]) }),
    ] {
        engine.bytes_received(1, &frame::encode(&msg), 0);
    }
    let outputs: Vec<Output> = std::iter::from_fn(|| engine.poll_output()).collect();
    assert!(outputs.contains(&Output::Close { conn: 1 }));
    assert!(!outputs.iter().any(|o| matches!(o, Output::Event(Event::PairingCode { .. }))));
}

// ---------------------------------------------------------------- unpairing

#[test]
fn unpairing_removes_the_device_on_both_sides() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session);
    net.a.unpair(&id(B));
    net.pump();
    assert!(has(&net.seen_a.events, &Event::Unpaired { peer: id(B) }));
    assert!(has(&net.seen_b.events, &Event::Unpaired { peer: id(A) }));
    assert!(net.a.paired().is_empty() && net.b.paired().is_empty());
    assert!(!net.a.is_connected(&id(B)) && !net.b.is_connected(&id(A)));
}

#[test]
fn a_closed_connection_disconnects_the_peer() {
    let mut net = Net::paired();
    let link = net.a_dials_b(Intent::Session);
    let (conn_a, _) = net.links[link];
    net.a.connection_closed(conn_a);
    net.pump();
    assert!(has(&net.seen_a.events, &Event::PeerDisconnected { peer: id(B) }));
    assert!(!net.a.is_connected(&id(B)));
}

// ---------------------------------------------------------------- edge cases

impl Probe {
    fn dialer(intent: Intent) -> Self {
        let mut engine = Engine::new(device(A, "alpha"), vec![paired(B, "beta")], 0);
        engine.connection_opened(1, Role::Dialer, id(B), intent, 0);
        let mut probe = Probe { engine, outputs: vec![] };
        probe.drain();
        probe
    }

    fn events(&self) -> Vec<Event> {
        self.outputs
            .iter()
            .filter_map(|o| match o {
                Output::Event(e) => Some(e.clone()),
                _ => None,
            })
            .collect()
    }
}

fn hello_named(from: u8, name: &str) -> Message {
    let mut h = hello(from, PROTO_VERSION);
    if let Message::Hello(h) = &mut h {
        h.name = name.into();
    }
    h
}

fn clip_from(origin: u8, mime: &str, text: &str) -> Message {
    Message::Clip(clipsync_core::message::Clip {
        id: Hex16::random(),
        origin: id(origin),
        seq: 1,
        ts: 0,
        mime: mime.into(),
        sha256: clipsync_core::clip::text_digest(text),
        text: text.into(),
    })
}

#[test]
fn connected_peers_lists_established_sessions() {
    let mut net = Net::paired();
    assert!(net.a.connected_peers().is_empty());
    net.a_dials_b(Intent::Session);
    assert_eq!(net.a.connected_peers(), vec![id(B)]);
}

#[test]
fn stopping_pairing_mode_refuses_connections_waiting_to_pair() {
    let mut engine = Engine::new(device(B, "beta"), vec![], 0);
    engine.start_pairing(0);
    engine.connection_opened(1, Role::Acceptor, id(A), Intent::Session, 0);
    engine.bytes_received(1, &frame::encode(&hello(A, PROTO_VERSION)), 0);
    engine.stop_pairing();
    engine.stop_pairing(); // already closed: nothing more happens
    let outputs: Vec<Output> = std::iter::from_fn(|| engine.poll_output()).collect();
    let ended = outputs.iter().filter(|o| **o == Output::Event(Event::PairingModeEnded)).count();
    assert_eq!(ended, 1);
    assert!(outputs.contains(&Output::Event(Event::ConnectionClosed {
        conn: 1,
        peer: id(A),
        reason: CloseReason::PairingClosed
    })));
    assert!(!engine.pairing_active(0));
}

#[test]
fn messages_of_unknown_types_are_ignored() {
    let mut probe = Probe::acceptor(true);
    probe.send(&hello(A, PROTO_VERSION));
    let unknown = br#"{"type":"file_offer","size":3}"#;
    let mut bytes = (unknown.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(unknown);
    probe.engine.bytes_received(1, &bytes, 0);
    probe.drain();
    assert!(!probe.closed());
}

#[test]
fn a_decision_for_a_connection_not_awaiting_one_is_ignored() {
    let mut probe = Probe::acceptor(true);
    probe.send(&hello(A, PROTO_VERSION));
    probe.engine.confirm_pairing(1, true, 0);
    probe.engine.confirm_pairing(99, true, 0);
    probe.drain();
    assert!(!probe.closed());
    assert!(!probe.sent_messages().iter().any(|m| matches!(m, Message::PairResult(_))));
}

#[test]
fn unpairing_an_unknown_device_does_nothing() {
    let mut engine = Engine::new(device(A, "alpha"), vec![], 0);
    engine.unpair(&id(B));
    assert_eq!(engine.poll_output(), None);
}

#[test]
fn a_token_rejected_by_the_acceptor_closes_the_dialer() {
    use clipsync_core::message::PairResult;
    let mut probe = Probe::dialer(Intent::PairToken { token: Hex([1; 16]) });
    probe.send(&hello_named(B, "beta"));
    probe.send(&Message::PairResult(PairResult { ok: false, reason: None }));
    assert_eq!(probe.close_reason(), Some(CloseReason::RejectedByPeer));
    assert!(probe.engine.paired() == vec![paired(B, "beta")], "an existing pairing is kept");
}

#[test]
fn a_message_unexpected_in_the_current_phase_is_a_protocol_error() {
    use clipsync_core::message::PairNonce;
    let mut probe = Probe::acceptor(true);
    probe.send(&hello(A, PROTO_VERSION));
    probe.send(&Message::PairNonce(PairNonce { nonce: Hex([0; 32]) }));
    assert_eq!(probe.close_reason(), Some(CloseReason::ProtocolError));
}

#[test]
fn a_relayed_clip_is_a_protocol_error() {
    let mut probe = Probe::acceptor(true);
    probe.send(&hello(A, PROTO_VERSION));
    probe.send(&clip_from(0x42, clipsync_core::clip::TEXT_MIME, "relayed"));
    assert_eq!(probe.close_reason(), Some(CloseReason::ProtocolError));
    assert!(!probe.outputs.iter().any(|o| matches!(o, Output::SetClipboard { .. })));
}

#[test]
fn a_clip_of_an_unsupported_type_is_acknowledged_but_not_applied() {
    let mut probe = Probe::acceptor(true);
    probe.send(&hello(A, PROTO_VERSION));
    probe.send(&clip_from(A, "image/png", "not really an image"));
    assert!(!probe.closed());
    assert!(!probe.outputs.iter().any(|o| matches!(o, Output::SetClipboard { .. })));
    assert!(probe.sent_messages().iter().any(|m| matches!(m, Message::Ack(a) if !a.applied)));
    assert!(probe.events().contains(&Event::ClipReceived { from: id(A), applied: false }));
}

#[test]
fn a_paired_device_that_changed_its_name_is_updated() {
    let mut probe = Probe::acceptor(true);
    probe.send(&hello_named(A, "alpha-renamed"));
    assert!(probe.events().contains(&Event::Paired { device: paired(A, "alpha-renamed") }));
    assert_eq!(probe.engine.paired(), vec![paired(A, "alpha-renamed")]);
}

#[test]
fn a_new_connection_that_loses_the_duplicate_rule_is_closed() {
    let mut net = Net::paired();
    net.a_dials_b(Intent::Session); // link 0: dialer A — wins
    net.b_dials_a(); // link 1: dialer B — must lose, although it is newer
    let (loser_a, loser_b) = net.links[1];
    assert!(net.seen_a.closed.contains(&loser_a) && net.seen_b.closed.contains(&loser_b));
    let (winner_a, _) = net.links[0];
    assert!(!net.seen_a.closed.contains(&winner_a));
    assert!(net.a.is_connected(&id(B)));
}
