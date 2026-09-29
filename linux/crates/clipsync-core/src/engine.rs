//! Session engine: the protocol state of every connection (spec §5–§7), with no I/O.
//!
//! The host owns sockets, TLS, timers and the clipboard. It reports what happens
//! (a connection opened, bytes arrived, the clipboard changed, time passed) and
//! carries out the [`Output`]s the engine queues: bytes to send, connections to
//! close, text to put on the clipboard, and events to show or persist.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::clip::{ClipTracker, LocalOutcome, RemoteOutcome, TEXT_MIME};
use crate::frame::{self, Decoded, FrameDecoder};
use crate::hex::{Hex16, Hex32};
use crate::identity::DeviceId;
use crate::message::{
    Ack, Clip, ErrorMsg, Hello, Message, PROTO_VERSION, PairMethod, PairNonce, PairRequest, PairResult, error_code,
};
use crate::pairing::{sas_code, sas_commit};

/// Host-chosen identifier of one connection, unique for the engine's lifetime.
pub type ConnId = u64;

/// Send a `ping` after this long without sending anything.
pub const KEEPALIVE_IDLE_MS: u64 = 30_000;
/// Close a connection after this long without receiving anything.
pub const TIMEOUT_MS: u64 = 90_000;
/// How long pairing mode stays open.
pub const PAIRING_WINDOW_MS: u64 = 120_000;
/// Wrong tokens accepted before pairing mode closes.
pub const MAX_TOKEN_FAILURES: u8 = 3;

/// Which side of the TLS connection this device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Dialer,
    Acceptor,
}

/// Why the dialer opened a connection. Acceptors always pass [`Intent::Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// A session with an already paired device.
    Session,
    /// Pairing with the token from the acceptor's QR code (spec §7.2).
    PairToken { token: Hex16 },
    /// Pairing by comparing codes (spec §7.3).
    PairSas,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDevice {
    pub id: DeviceId,
    pub name: String,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    pub id: DeviceId,
    pub name: String,
}

/// Work for the host, in the order it must be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Send {
        conn: ConnId,
        bytes: Vec<u8>,
    },
    /// Close the connection's socket; the engine has already forgotten it.
    Close {
        conn: ConnId,
    },
    SetClipboard {
        text: String,
    },
    Event(Event),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    PeerConnected {
        peer: DeviceId,
        name: String,
    },
    PeerDisconnected {
        peer: DeviceId,
    },
    ConnectionClosed {
        conn: ConnId,
        peer: DeviceId,
        reason: CloseReason,
    },
    /// Show `code` to the user and call [`Engine::confirm_pairing`] with their decision.
    PairingCode {
        conn: ConnId,
        peer: DeviceId,
        name: String,
        code: u32,
    },
    /// A device was paired, or a paired device changed its name. Persist it.
    Paired {
        device: PairedDevice,
    },
    PairingModeEnded,
    /// Persist the removal.
    Unpaired {
        peer: DeviceId,
    },
    ClipReceived {
        from: DeviceId,
        applied: bool,
    },
    ClipDelivered {
        to: DeviceId,
        applied: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    /// A better connection to the same peer won (spec §7.1).
    Duplicate,
    Timeout,
    Unpaired,
    UnsupportedVersion,
    IdentityMismatch,
    NotPaired,
    PairingClosed,
    BadToken,
    ProtocolError,
    RejectedLocally,
    RejectedByPeer,
    /// The peer sent `error` with this code.
    RemoteError {
        code: String,
    },
    /// The host reported the socket closed.
    Closed,
}

/// What happened to a local clipboard change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalChange {
    Sent { seq: u64, peers: u32 },
    Unchanged,
    Empty,
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    AwaitingHello,
    /// Acceptor, unknown peer, pairing mode open.
    AwaitingPairRequest,
    Established,
    /// Dialer, token flow.
    TokenSent,
    /// Dialer, SAS flow.
    SasCommitted {
        nonce_d: Hex32,
    },
    /// Acceptor, SAS flow.
    SasNonceSent {
        commit: Hex32,
        nonce_a: Hex32,
    },
    SasDecision {
        local: Option<bool>,
        remote: Option<bool>,
    },
}

struct Conn {
    role: Role,
    peer: DeviceId,
    intent: Intent,
    peer_name: String,
    decoder: FrameDecoder,
    phase: Phase,
    last_rx: u64,
    last_tx: u64,
}

struct PairingMode {
    token: Hex16,
    expires_at: u64,
    failures: u8,
}

pub struct Engine {
    me: LocalDevice,
    paired: BTreeMap<DeviceId, PairedDevice>,
    tracker: ClipTracker,
    pairing: Option<PairingMode>,
    conns: BTreeMap<ConnId, Conn>,
    /// The established connection carrying each peer's session.
    active: BTreeMap<DeviceId, ConnId>,
    /// Peers reported with `PeerConnected` and not yet with `PeerDisconnected`.
    reported: BTreeSet<DeviceId>,
    out: VecDeque<Output>,
}

impl Engine {
    /// `lamport` is the counter persisted by the previous run, or 0.
    pub fn new(me: LocalDevice, paired: Vec<PairedDevice>, lamport: u64) -> Self {
        Engine {
            tracker: ClipTracker::new(me.id, lamport),
            me,
            paired: paired.into_iter().map(|d| (d.id, d)).collect(),
            pairing: None,
            conns: BTreeMap::new(),
            active: BTreeMap::new(),
            reported: BTreeSet::new(),
            out: VecDeque::new(),
        }
    }

    /// Current Lamport counter; persist it when it changes.
    pub fn lamport(&self) -> u64 {
        self.tracker.lamport()
    }

    pub fn paired(&self) -> Vec<PairedDevice> {
        self.paired.values().cloned().collect()
    }

    pub fn is_connected(&self, peer: &DeviceId) -> bool {
        self.active.contains_key(peer)
    }

    pub fn connected_peers(&self) -> Vec<DeviceId> {
        self.active.keys().copied().collect()
    }

    pub fn pairing_active(&self, now_ms: u64) -> bool {
        self.pairing.as_ref().is_some_and(|p| now_ms < p.expires_at)
    }

    /// Opens pairing mode for [`PAIRING_WINDOW_MS`] and returns the token for the QR code.
    pub fn start_pairing(&mut self, now_ms: u64) -> Hex16 {
        let token = Hex16::random();
        self.pairing = Some(PairingMode { token, expires_at: now_ms + PAIRING_WINDOW_MS, failures: 0 });
        token
    }

    pub fn stop_pairing(&mut self) {
        self.end_pairing_mode();
    }

    /// A TLS connection is up. `peer` is the device ID from the peer's certificate.
    pub fn connection_opened(&mut self, conn: ConnId, role: Role, peer: DeviceId, intent: Intent, now_ms: u64) {
        self.conns.insert(
            conn,
            Conn {
                role,
                peer,
                intent,
                peer_name: String::new(),
                decoder: FrameDecoder::new(),
                phase: Phase::AwaitingHello,
                last_rx: now_ms,
                last_tx: now_ms,
            },
        );
        let hello = Message::Hello(Hello {
            proto: PROTO_VERSION,
            id: self.me.id,
            name: self.me.name.clone(),
            platform: self.me.platform.clone(),
            caps: vec!["text".into()],
            seq: self.tracker.lamport(),
        });
        self.send(conn, &hello, now_ms);
    }

    pub fn bytes_received(&mut self, conn: ConnId, bytes: &[u8], now_ms: u64) {
        let Some(c) = self.conns.get_mut(&conn) else { return };
        c.last_rx = now_ms;
        c.decoder.push(bytes);
        loop {
            let Some(c) = self.conns.get_mut(&conn) else { return };
            match c.decoder.next_frame() {
                Ok(None) => return,
                Ok(Some(Decoded::Ignored { .. })) => {}
                Ok(Some(Decoded::Message(msg))) => self.handle(conn, msg, now_ms),
                Err(_) => {
                    return self.fail(conn, error_code::PROTOCOL_ERROR, CloseReason::ProtocolError, now_ms);
                }
            }
        }
    }

    /// The host saw the socket close.
    pub fn connection_closed(&mut self, conn: ConnId) {
        self.forget(conn, CloseReason::Closed);
    }

    /// The user's decision on a [`Event::PairingCode`].
    pub fn confirm_pairing(&mut self, conn: ConnId, accept: bool, now_ms: u64) {
        let Some(Phase::SasDecision { remote, .. }) = self.conns.get(&conn).map(|c| c.phase.clone()) else {
            return;
        };
        self.send(conn, &Message::PairResult(PairResult { ok: accept, reason: None }), now_ms);
        if !accept {
            return self.close(conn, CloseReason::RejectedLocally);
        }
        if remote == Some(true) {
            self.complete_pairing(conn);
        } else if let Some(c) = self.conns.get_mut(&conn) {
            c.phase = Phase::SasDecision { local: Some(true), remote };
        }
    }

    /// The local clipboard changed; sends the text to every connected peer (spec §6).
    pub fn local_clipboard_changed(&mut self, text: String, now_ms: u64) -> LocalChange {
        match self.tracker.local_change(text, now_ms) {
            LocalOutcome::Empty => LocalChange::Empty,
            LocalOutcome::TooLarge => LocalChange::TooLarge,
            LocalOutcome::Unchanged => LocalChange::Unchanged,
            LocalOutcome::Emit(clip) => {
                let seq = clip.seq;
                let conns: Vec<ConnId> = self.active.values().copied().collect();
                let msg = Message::Clip(clip);
                for &conn in &conns {
                    self.send(conn, &msg, now_ms);
                }
                LocalChange::Sent { seq, peers: conns.len() as u32 }
            }
        }
    }

    /// Forgets a paired device and tells it, if it is connected.
    pub fn unpair(&mut self, peer: &DeviceId) {
        if self.paired.remove(peer).is_none() {
            return;
        }
        self.event(Event::Unpaired { peer: *peer });
        if let Some(&conn) = self.active.get(peer) {
            let now = self.conns[&conn].last_tx;
            self.send(conn, &Message::Unpair {}, now);
            self.close(conn, CloseReason::Unpaired);
        }
    }

    /// Time passed: keepalives, timeouts and the end of pairing mode.
    pub fn tick(&mut self, now_ms: u64) {
        if self.pairing.as_ref().is_some_and(|p| now_ms >= p.expires_at) {
            self.end_pairing_mode();
        }
        let conns: Vec<ConnId> = self.conns.keys().copied().collect();
        for conn in conns {
            let Some(c) = self.conns.get(&conn) else { continue };
            if now_ms.saturating_sub(c.last_rx) >= TIMEOUT_MS {
                self.close(conn, CloseReason::Timeout);
            } else if now_ms.saturating_sub(c.last_tx) >= KEEPALIVE_IDLE_MS {
                self.send(conn, &Message::Ping {}, now_ms);
            }
        }
    }

    pub fn poll_output(&mut self) -> Option<Output> {
        self.out.pop_front()
    }

    // ------------------------------------------------------------ message handling

    fn handle(&mut self, conn: ConnId, msg: Message, now: u64) {
        let phase = self.conns[&conn].phase.clone();
        match (phase, msg) {
            (Phase::AwaitingHello, Message::Hello(h)) => self.on_hello(conn, h, now),
            (Phase::AwaitingHello, _) | (_, Message::Hello(_)) => {
                self.fail(conn, error_code::PROTOCOL_ERROR, CloseReason::ProtocolError, now)
            }
            (_, Message::Ping {}) => self.send(conn, &Message::Pong {}, now),
            (_, Message::Pong {}) => {}
            (_, Message::Error(ErrorMsg { code, .. })) => self.close(conn, CloseReason::RemoteError { code }),
            (Phase::Established, Message::Clip(clip)) => self.on_clip(conn, clip, now),
            (Phase::Established, Message::Ack(Ack { applied, .. })) => {
                let to = self.conns[&conn].peer;
                self.event(Event::ClipDelivered { to, applied });
            }
            (Phase::Established, Message::Unpair {}) => {
                let peer = self.conns[&conn].peer;
                if self.paired.remove(&peer).is_some() {
                    self.event(Event::Unpaired { peer });
                }
                self.close(conn, CloseReason::Unpaired);
            }
            (Phase::Established | Phase::AwaitingPairRequest, Message::PairRequest(req)) => {
                self.on_pair_request(conn, req, now)
            }
            (Phase::TokenSent, Message::PairResult(PairResult { ok, .. })) => {
                if ok {
                    self.complete_pairing(conn);
                } else {
                    self.close(conn, CloseReason::RejectedByPeer);
                }
            }
            (Phase::SasCommitted { nonce_d }, Message::PairNonce(PairNonce { nonce: nonce_a })) => {
                self.send(conn, &Message::PairReveal(PairNonce { nonce: nonce_d }), now);
                let code = sas_code(&self.me.id, &self.conns[&conn].peer, &nonce_d, &nonce_a);
                self.await_decision(conn, code);
            }
            (Phase::SasNonceSent { commit, nonce_a }, Message::PairReveal(PairNonce { nonce: nonce_d })) => {
                if sas_commit(&nonce_d) != commit {
                    return self.fail(conn, error_code::PROTOCOL_ERROR, CloseReason::ProtocolError, now);
                }
                let code = sas_code(&self.conns[&conn].peer, &self.me.id, &nonce_d, &nonce_a);
                self.await_decision(conn, code);
            }
            (Phase::SasDecision { local, .. }, Message::PairResult(PairResult { ok, .. })) => {
                if !ok {
                    self.close(conn, CloseReason::RejectedByPeer);
                } else if local == Some(true) {
                    self.complete_pairing(conn);
                } else if let Some(c) = self.conns.get_mut(&conn) {
                    c.phase = Phase::SasDecision { local, remote: Some(true) };
                }
            }
            _ => self.fail(conn, error_code::PROTOCOL_ERROR, CloseReason::ProtocolError, now),
        }
    }

    fn on_hello(&mut self, conn: ConnId, hello: Hello, now: u64) {
        if hello.proto != PROTO_VERSION {
            return self.fail(conn, error_code::UNSUPPORTED_VERSION, CloseReason::UnsupportedVersion, now);
        }
        let c = &self.conns[&conn];
        if hello.id != c.peer {
            return self.fail(conn, error_code::IDENTITY_MISMATCH, CloseReason::IdentityMismatch, now);
        }
        self.tracker.observe_hello(hello.seq);
        let (role, peer, intent) = (c.role, c.peer, c.intent.clone());
        self.conns.get_mut(&conn).unwrap().peer_name = hello.name;
        let known = self.paired.contains_key(&peer);
        match (role, intent) {
            (Role::Dialer, Intent::PairToken { token }) => {
                let req = PairRequest { method: PairMethod::Token, token: Some(token), commit: None };
                self.send(conn, &Message::PairRequest(req), now);
                self.set_phase(conn, Phase::TokenSent);
            }
            (Role::Dialer, Intent::PairSas) => {
                let nonce_d = Hex32::random();
                let req = PairRequest { method: PairMethod::Sas, token: None, commit: Some(sas_commit(&nonce_d)) };
                self.send(conn, &Message::PairRequest(req), now);
                self.set_phase(conn, Phase::SasCommitted { nonce_d });
            }
            _ if known => self.establish(conn),
            (Role::Acceptor, _) if self.pairing_active(now) => self.set_phase(conn, Phase::AwaitingPairRequest),
            _ => self.fail(conn, error_code::NOT_PAIRED, CloseReason::NotPaired, now),
        }
    }

    fn on_clip(&mut self, conn: ConnId, clip: Clip, now: u64) {
        let peer = self.conns[&conn].peer;
        if clip.origin != peer {
            // v1 has no relaying: a peer only sends clips it originated (spec §6).
            return self.fail(conn, error_code::PROTOCOL_ERROR, CloseReason::ProtocolError, now);
        }
        let applied = if clip.mime != TEXT_MIME {
            false
        } else {
            let outcome = self.tracker.receive(&clip);
            if outcome == RemoteOutcome::Apply {
                self.out.push_back(Output::SetClipboard { text: clip.text });
            }
            outcome.applied()
        };
        self.send(conn, &Message::Ack(Ack { id: clip.id, applied }), now);
        self.event(Event::ClipReceived { from: peer, applied });
    }

    fn on_pair_request(&mut self, conn: ConnId, req: PairRequest, now: u64) {
        if !self.pairing_active(now) {
            return self.fail(conn, error_code::PAIRING_CLOSED, CloseReason::PairingClosed, now);
        }
        match req.method {
            PairMethod::Token => {
                let mode = self.pairing.as_mut().unwrap();
                if req.token.is_some_and(|t| constant_time_eq(&t.0, &mode.token.0)) {
                    self.send(conn, &Message::PairResult(PairResult { ok: true, reason: None }), now);
                    self.complete_pairing(conn);
                } else {
                    mode.failures += 1;
                    let exhausted = mode.failures >= MAX_TOKEN_FAILURES;
                    self.fail(conn, error_code::BAD_TOKEN, CloseReason::BadToken, now);
                    if exhausted {
                        self.end_pairing_mode();
                    }
                }
            }
            PairMethod::Sas => {
                let nonce_a = Hex32::random();
                self.send(conn, &Message::PairNonce(PairNonce { nonce: nonce_a }), now);
                let commit = req.commit.expect("validated by Message::validate");
                self.set_phase(conn, Phase::SasNonceSent { commit, nonce_a });
            }
        }
    }

    // ------------------------------------------------------------ state changes

    fn await_decision(&mut self, conn: ConnId, code: u32) {
        self.set_phase(conn, Phase::SasDecision { local: None, remote: None });
        let c = &self.conns[&conn];
        let event = Event::PairingCode { conn, peer: c.peer, name: c.peer_name.clone(), code };
        self.event(event);
    }

    fn complete_pairing(&mut self, conn: ConnId) {
        let c = &self.conns[&conn];
        let device = PairedDevice { id: c.peer, name: c.peer_name.clone() };
        let acceptor = c.role == Role::Acceptor;
        self.paired.insert(device.id, device.clone());
        self.event(Event::Paired { device });
        // Establish first: ending pairing mode closes connections still waiting to pair.
        self.establish(conn);
        if acceptor {
            // One pairing per pairing mode.
            self.end_pairing_mode();
        }
    }

    fn establish(&mut self, conn: ConnId) {
        self.set_phase(conn, Phase::Established);
        let c = &self.conns[&conn];
        let (peer, name) = (c.peer, c.peer_name.clone());
        if let Some(device) = self.paired.get_mut(&peer)
            && device.name != name
        {
            device.name = name.clone();
            let device = device.clone();
            self.event(Event::Paired { device });
        }
        if let Some(&other) = self.active.get(&peer) {
            // Spec §7.1: keep the connection whose TLS client has the smaller device ID;
            // between two from the same client, keep the newer one.
            if self.dialer_of(conn) <= self.dialer_of(other) {
                self.active.insert(peer, conn);
                self.close(other, CloseReason::Duplicate);
            } else {
                self.close(conn, CloseReason::Duplicate);
            }
            return;
        }
        self.active.insert(peer, conn);
        if self.reported.insert(peer) {
            self.event(Event::PeerConnected { peer, name });
        }
    }

    fn dialer_of(&self, conn: ConnId) -> DeviceId {
        let c = &self.conns[&conn];
        match c.role {
            Role::Dialer => self.me.id,
            Role::Acceptor => c.peer,
        }
    }

    fn end_pairing_mode(&mut self) {
        if self.pairing.take().is_none() {
            return;
        }
        self.event(Event::PairingModeEnded);
        let waiting: Vec<ConnId> =
            self.conns.iter().filter(|(_, c)| c.phase == Phase::AwaitingPairRequest).map(|(&id, _)| id).collect();
        for conn in waiting {
            let now = self.conns[&conn].last_tx;
            self.fail(conn, error_code::PAIRING_CLOSED, CloseReason::PairingClosed, now);
        }
    }

    fn set_phase(&mut self, conn: ConnId, phase: Phase) {
        if let Some(c) = self.conns.get_mut(&conn) {
            c.phase = phase;
        }
    }

    // ------------------------------------------------------------ output helpers

    fn send(&mut self, conn: ConnId, msg: &Message, now: u64) {
        if let Some(c) = self.conns.get_mut(&conn) {
            c.last_tx = now;
            self.out.push_back(Output::Send { conn, bytes: frame::encode(msg) });
        }
    }

    fn event(&mut self, event: Event) {
        self.out.push_back(Output::Event(event));
    }

    /// Sends `error` with `code`, then closes.
    fn fail(&mut self, conn: ConnId, code: &str, reason: CloseReason, now: u64) {
        self.send(conn, &Message::Error(ErrorMsg { code: code.into(), message: None }), now);
        self.close(conn, reason);
    }

    fn close(&mut self, conn: ConnId, reason: CloseReason) {
        if self.conns.contains_key(&conn) {
            self.out.push_back(Output::Close { conn });
            self.forget(conn, reason);
        }
    }

    fn forget(&mut self, conn: ConnId, reason: CloseReason) {
        let Some(c) = self.conns.remove(&conn) else { return };
        let peer = c.peer;
        self.event(Event::ConnectionClosed { conn, peer, reason });
        if self.active.get(&peer) == Some(&conn) {
            self.active.remove(&peer);
        }
        // A replacement connection may still be completing its hello: only report the
        // peer gone once no connection to it is left.
        let pending = self.conns.values().any(|c| c.peer == peer);
        if !pending && self.reported.remove(&peer) {
            self.event(Event::PeerDisconnected { peer });
        }
    }
}

fn constant_time_eq(a: &[u8; 16], b: &[u8; 16]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
