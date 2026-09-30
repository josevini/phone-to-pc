//! UniFFI bindings of `clipsync-core` for the Android app (D2).
//!
//! A thin translation layer: device IDs, tokens and addresses cross the boundary as strings
//! and records, the engine becomes a thread-safe object, and its outputs are drained in
//! one call. Every decision is still made by `clipsync-core`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};

use clipsync_core::engine as eng;
use clipsync_core::pairing::UriError;
use clipsync_core::{DeviceId, Hex16, discovery, identity, pairing};

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("`{id}` is not a device ID")]
    InvalidDeviceId { id: String },
    #[error("not a pairing token")]
    InvalidToken,
    /// `reason`, not `message`: Kotlin exceptions already have a `message`.
    #[error("invalid pairing URI: {reason}")]
    InvalidUri { reason: String },
    #[error("unsupported pairing URI version {version}")]
    UnsupportedUriVersion { version: String },
}

impl From<UriError> for CoreError {
    fn from(e: UriError) -> Self {
        match e {
            UriError::Invalid(reason) => CoreError::InvalidUri { reason },
            UriError::UnsupportedVersion(version) => CoreError::UnsupportedUriVersion { version },
        }
    }
}

fn device_id(id: &str) -> Result<DeviceId, CoreError> {
    id.parse().map_err(|_| CoreError::InvalidDeviceId { id: id.to_owned() })
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LocalDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
}

impl PairedDevice {
    fn to_core(&self) -> Result<eng::PairedDevice, CoreError> {
        Ok(eng::PairedDevice { id: device_id(&self.id)?, name: self.name.clone() })
    }
}

impl From<eng::PairedDevice> for PairedDevice {
    fn from(d: eng::PairedDevice) -> Self {
        PairedDevice { id: d.id.to_string(), name: d.name }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Role {
    Dialer,
    Acceptor,
}

/// Why the dialer opened a connection; acceptors pass `Session`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Intent {
    Session,
    /// `token` is the 32 hex characters from the pairing URI.
    PairToken {
        token: String,
    },
    PairSas,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Output {
    Send {
        conn: u64,
        bytes: Vec<u8>,
    },
    /// Close the connection's socket; the engine has already forgotten it.
    Close {
        conn: u64,
    },
    SetClipboard {
        text: String,
    },
    Event {
        event: EngineEvent,
    },
}

impl From<eng::Output> for Output {
    fn from(o: eng::Output) -> Self {
        match o {
            eng::Output::Send { conn, bytes } => Output::Send { conn, bytes },
            eng::Output::Close { conn } => Output::Close { conn },
            eng::Output::SetClipboard { text } => Output::SetClipboard { text },
            eng::Output::Event(event) => Output::Event { event: event.into() },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum EngineEvent {
    PeerConnected {
        peer: String,
        name: String,
    },
    PeerDisconnected {
        peer: String,
    },
    ConnectionClosed {
        conn: u64,
        peer: String,
        reason: CloseReason,
    },
    /// Show `code` (see [`format_sas`]) and call `Engine::confirm_pairing` with the user's decision.
    PairingCode {
        conn: u64,
        peer: String,
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
        peer: String,
    },
    ClipReceived {
        from: String,
        applied: bool,
    },
    ClipDelivered {
        to: String,
        applied: bool,
    },
}

impl From<eng::Event> for EngineEvent {
    fn from(e: eng::Event) -> Self {
        use eng::Event as E;
        match e {
            E::PeerConnected { peer, name } => EngineEvent::PeerConnected { peer: peer.to_string(), name },
            E::PeerDisconnected { peer } => EngineEvent::PeerDisconnected { peer: peer.to_string() },
            E::ConnectionClosed { conn, peer, reason } => {
                EngineEvent::ConnectionClosed { conn, peer: peer.to_string(), reason: reason.into() }
            }
            E::PairingCode { conn, peer, name, code } => {
                EngineEvent::PairingCode { conn, peer: peer.to_string(), name, code }
            }
            E::Paired { device } => EngineEvent::Paired { device: device.into() },
            E::PairingModeEnded => EngineEvent::PairingModeEnded,
            E::Unpaired { peer } => EngineEvent::Unpaired { peer: peer.to_string() },
            E::ClipReceived { from, applied } => EngineEvent::ClipReceived { from: from.to_string(), applied },
            E::ClipDelivered { to, applied } => EngineEvent::ClipDelivered { to: to.to_string(), applied },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CloseReason {
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
    RemoteError { code: String },
    Closed,
}

impl From<eng::CloseReason> for CloseReason {
    fn from(r: eng::CloseReason) -> Self {
        use eng::CloseReason as R;
        match r {
            R::Duplicate => CloseReason::Duplicate,
            R::Timeout => CloseReason::Timeout,
            R::Unpaired => CloseReason::Unpaired,
            R::UnsupportedVersion => CloseReason::UnsupportedVersion,
            R::IdentityMismatch => CloseReason::IdentityMismatch,
            R::NotPaired => CloseReason::NotPaired,
            R::PairingClosed => CloseReason::PairingClosed,
            R::BadToken => CloseReason::BadToken,
            R::ProtocolError => CloseReason::ProtocolError,
            R::RejectedLocally => CloseReason::RejectedLocally,
            R::RejectedByPeer => CloseReason::RejectedByPeer,
            R::RemoteError { code } => CloseReason::RemoteError { code },
            R::Closed => CloseReason::Closed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LocalChange {
    Sent { seq: u64, peers: u32 },
    Unchanged,
    Empty,
    TooLarge,
}

impl From<eng::LocalChange> for LocalChange {
    fn from(c: eng::LocalChange) -> Self {
        match c {
            eng::LocalChange::Sent { seq, peers } => LocalChange::Sent { seq, peers },
            eng::LocalChange::Unchanged => LocalChange::Unchanged,
            eng::LocalChange::Empty => LocalChange::Empty,
            eng::LocalChange::TooLarge => LocalChange::TooLarge,
        }
    }
}

/// The protocol state of every connection (`clipsync_core::engine::Engine`), safe to share
/// between threads. Callers drain [`Engine::poll_outputs`] after each call and carry them out
/// in order.
#[derive(uniffi::Object)]
pub struct Engine(Mutex<eng::Engine>);

#[uniffi::export]
impl Engine {
    /// `lamport` is the counter persisted by the previous run, or 0.
    #[uniffi::constructor]
    pub fn new(me: LocalDevice, paired: Vec<PairedDevice>, lamport: u64) -> Result<Arc<Self>, CoreError> {
        let me = eng::LocalDevice { id: device_id(&me.id)?, name: me.name, platform: me.platform };
        let paired = paired.iter().map(PairedDevice::to_core).collect::<Result<_, _>>()?;
        Ok(Arc::new(Engine(Mutex::new(eng::Engine::new(me, paired, lamport)))))
    }

    /// Current Lamport counter; persist it when it changes.
    pub fn lamport(&self) -> u64 {
        self.lock().lamport()
    }

    pub fn paired(&self) -> Vec<PairedDevice> {
        self.lock().paired().into_iter().map(Into::into).collect()
    }

    pub fn is_connected(&self, peer: String) -> Result<bool, CoreError> {
        Ok(self.lock().is_connected(&device_id(&peer)?))
    }

    pub fn connected_peers(&self) -> Vec<String> {
        self.lock().connected_peers().iter().map(ToString::to_string).collect()
    }

    pub fn pairing_active(&self, now_ms: u64) -> bool {
        self.lock().pairing_active(now_ms)
    }

    /// Opens pairing mode and returns the token for the QR code.
    pub fn start_pairing(&self, now_ms: u64) -> String {
        self.lock().start_pairing(now_ms).to_string()
    }

    pub fn stop_pairing(&self) {
        self.lock().stop_pairing();
    }

    /// A TLS connection is up. `peer` is the device ID from the peer's certificate.
    pub fn connection_opened(
        &self,
        conn: u64,
        role: Role,
        peer: String,
        intent: Intent,
        now_ms: u64,
    ) -> Result<(), CoreError> {
        let peer = device_id(&peer)?;
        let role = match role {
            Role::Dialer => eng::Role::Dialer,
            Role::Acceptor => eng::Role::Acceptor,
        };
        let intent = match intent {
            Intent::Session => eng::Intent::Session,
            Intent::PairToken { token } => {
                eng::Intent::PairToken { token: token.parse::<Hex16>().map_err(|_| CoreError::InvalidToken)? }
            }
            Intent::PairSas => eng::Intent::PairSas,
        };
        self.lock().connection_opened(conn, role, peer, intent, now_ms);
        Ok(())
    }

    pub fn bytes_received(&self, conn: u64, bytes: Vec<u8>, now_ms: u64) {
        self.lock().bytes_received(conn, &bytes, now_ms);
    }

    /// The host saw the socket close.
    pub fn connection_closed(&self, conn: u64) {
        self.lock().connection_closed(conn);
    }

    /// The user's decision on an [`EngineEvent::PairingCode`].
    pub fn confirm_pairing(&self, conn: u64, accept: bool, now_ms: u64) {
        self.lock().confirm_pairing(conn, accept, now_ms);
    }

    /// The local clipboard changed, or the user sent text; sends it to every connected peer.
    pub fn local_clipboard_changed(&self, text: String, now_ms: u64) -> LocalChange {
        self.lock().local_clipboard_changed(text, now_ms).into()
    }

    /// Forgets a paired device and tells it, if it is connected.
    pub fn unpair(&self, peer: String) -> Result<(), CoreError> {
        self.lock().unpair(&device_id(&peer)?);
        Ok(())
    }

    /// Time passed: keepalives, timeouts and the end of pairing mode.
    pub fn tick(&self, now_ms: u64) {
        self.lock().tick(now_ms);
    }

    /// Everything queued since the last call, in the order it must be done.
    pub fn poll_outputs(&self) -> Vec<Output> {
        let mut engine = self.lock();
        std::iter::from_fn(|| engine.poll_output()).map(Into::into).collect()
    }
}

impl Engine {
    fn lock(&self) -> MutexGuard<'_, eng::Engine> {
        // A panic while locked already surfaced as an exception; the state is still consistent
        // enough to report, and poisoning would turn every later call into a crash.
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// An IP address (IPv6 without brackets) and port.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SocketAddress {
    pub ip: String,
    pub port: u16,
}

impl From<SocketAddr> for SocketAddress {
    fn from(a: SocketAddr) -> Self {
        SocketAddress { ip: a.ip().to_string(), port: a.port() }
    }
}

/// Contents of a pairing QR code (spec §8).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairUri {
    pub id: String,
    pub name: String,
    pub addrs: Vec<SocketAddress>,
    pub token: String,
}

#[uniffi::export]
pub fn parse_pair_uri(uri: String) -> Result<PairUri, CoreError> {
    let uri = pairing::PairUri::parse(&uri)?;
    Ok(PairUri {
        id: uri.id.to_string(),
        name: uri.name,
        addrs: uri.addrs.into_iter().map(Into::into).collect(),
        token: uri.token.to_string(),
    })
}

/// The pairing URI a device shows in its QR code (spec §8). Values the URI cannot carry are an
/// error, so what it returns always parses.
#[uniffi::export]
pub fn format_pair_uri(uri: PairUri) -> Result<String, CoreError> {
    let addrs = uri
        .addrs
        .iter()
        .map(|a| match a.ip.parse::<IpAddr>() {
            Ok(ip) => Ok(SocketAddr::new(ip, a.port)),
            Err(_) => Err(CoreError::InvalidUri { reason: format!("`{}` is not an IP address", a.ip) }),
        })
        .collect::<Result<_, _>>()?;
    let token = uri.token.parse::<Hex16>().map_err(|_| CoreError::InvalidToken)?;
    let text = pairing::PairUri { id: device_id(&uri.id)?, name: uri.name, addrs, token }.to_uri();
    pairing::PairUri::parse(&text)?;
    Ok(text)
}

/// Formats a pairing code for display: `037 725`.
#[uniffi::export]
pub fn format_sas(code: u32) -> String {
    pairing::format_sas(code)
}

/// Device ID of a public key: SHA-256 of its SubjectPublicKeyInfo DER, as hex (spec §2).
#[uniffi::export]
pub fn device_id_from_spki(spki: Vec<u8>) -> String {
    DeviceId::from_spki_der(&spki).to_string()
}

/// First 8 hex characters of a device ID, for display.
#[uniffi::export]
pub fn short_id(id: String) -> Result<String, CoreError> {
    Ok(device_id(&id)?.short())
}

/// Whether `name` is an acceptable device name (1–64 bytes of UTF-8).
#[uniffi::export]
pub fn is_valid_name(name: String) -> bool {
    identity::is_valid_name(&name)
}

/// A clipsync device found by DNS-SD, and where to reach it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DiscoveredPeer {
    pub id: String,
    pub addrs: Vec<SocketAddress>,
}

/// TXT properties a device advertises (spec §3).
#[uniffi::export]
pub fn txt_properties(id: String) -> Result<HashMap<String, String>, CoreError> {
    let id = device_id(&id)?;
    Ok(HashMap::from([
        (discovery::TXT_VERSION.to_owned(), discovery::TXT_VERSION_VALUE.to_owned()),
        (discovery::TXT_ID.to_owned(), id.to_string()),
    ]))
}

/// Reads a browsed service from its TXT properties: `None` unless it is a clipsync v1 device other than `me`
/// with a usable address. `ips` that do not parse are skipped.
#[uniffi::export]
pub fn peer_from_service(
    me: String,
    txt: HashMap<String, String>,
    port: u16,
    ips: Vec<String>,
) -> Result<Option<DiscoveredPeer>, CoreError> {
    let me = device_id(&me)?;
    let ips: Vec<IpAddr> = ips.iter().filter_map(|ip| ip.parse().ok()).collect();
    let (version, id) = (txt.get(discovery::TXT_VERSION), txt.get(discovery::TXT_ID));
    let found = discovery::peer_from(&me, version.map(String::as_str), id.map(String::as_str), port, &ips);
    Ok(found
        .map(|(id, addrs)| DiscoveredPeer { id: id.to_string(), addrs: addrs.into_iter().map(Into::into).collect() }))
}

/// DNS-SD instance name for this device: its name and short ID.
#[uniffi::export]
pub fn instance_name(name: String, id: String) -> Result<String, CoreError> {
    Ok(discovery::instance_name(&name, &device_id(&id)?))
}

/// DNS-SD service type, without the domain (`_clipsync._tcp`).
#[uniffi::export]
pub fn service_type() -> String {
    discovery::SERVICE_TYPE.to_owned()
}

#[uniffi::export]
pub fn default_port() -> u16 {
    discovery::DEFAULT_PORT
}

/// How long pairing mode stays open, and a pairing code works (spec §7.2).
#[uniffi::export]
pub fn pairing_window_ms() -> u64 {
    eng::PAIRING_WINDOW_MS
}

/// ALPN protocol ID of the TLS connection.
#[uniffi::export]
pub fn alpn() -> String {
    discovery::ALPN.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipsync_core::Hex;

    const ID: DeviceId = DeviceId(Hex([0xab; 32]));

    fn id() -> String {
        ID.to_string()
    }

    #[test]
    fn every_close_reason_crosses_the_boundary() {
        use eng::CloseReason as R;
        let cases = [
            (R::Duplicate, CloseReason::Duplicate),
            (R::Timeout, CloseReason::Timeout),
            (R::Unpaired, CloseReason::Unpaired),
            (R::UnsupportedVersion, CloseReason::UnsupportedVersion),
            (R::IdentityMismatch, CloseReason::IdentityMismatch),
            (R::NotPaired, CloseReason::NotPaired),
            (R::PairingClosed, CloseReason::PairingClosed),
            (R::BadToken, CloseReason::BadToken),
            (R::ProtocolError, CloseReason::ProtocolError),
            (R::RejectedLocally, CloseReason::RejectedLocally),
            (R::RejectedByPeer, CloseReason::RejectedByPeer),
            (R::RemoteError { code: "x".into() }, CloseReason::RemoteError { code: "x".into() }),
            (R::Closed, CloseReason::Closed),
        ];
        for (from, to) in cases {
            assert_eq!(CloseReason::from(from), to);
        }
    }

    #[test]
    fn every_event_crosses_the_boundary() {
        use eng::Event as E;
        let device = eng::PairedDevice { id: ID, name: "n".into() };
        let cases = [
            (
                E::PeerConnected { peer: ID, name: "n".into() },
                EngineEvent::PeerConnected { peer: id(), name: "n".into() },
            ),
            (E::PeerDisconnected { peer: ID }, EngineEvent::PeerDisconnected { peer: id() }),
            (
                E::ConnectionClosed { conn: 3, peer: ID, reason: eng::CloseReason::Timeout },
                EngineEvent::ConnectionClosed { conn: 3, peer: id(), reason: CloseReason::Timeout },
            ),
            (
                E::PairingCode { conn: 3, peer: ID, name: "n".into(), code: 7 },
                EngineEvent::PairingCode { conn: 3, peer: id(), name: "n".into(), code: 7 },
            ),
            (E::Paired { device }, EngineEvent::Paired { device: PairedDevice { id: id(), name: "n".into() } }),
            (E::PairingModeEnded, EngineEvent::PairingModeEnded),
            (E::Unpaired { peer: ID }, EngineEvent::Unpaired { peer: id() }),
            (E::ClipReceived { from: ID, applied: true }, EngineEvent::ClipReceived { from: id(), applied: true }),
            (E::ClipDelivered { to: ID, applied: false }, EngineEvent::ClipDelivered { to: id(), applied: false }),
        ];
        for (from, to) in cases {
            assert_eq!(EngineEvent::from(from), to);
        }
    }

    #[test]
    fn every_output_and_local_change_crosses_the_boundary() {
        assert_eq!(
            Output::from(eng::Output::Send { conn: 1, bytes: vec![2] }),
            Output::Send { conn: 1, bytes: vec![2] }
        );
        assert_eq!(Output::from(eng::Output::Close { conn: 1 }), Output::Close { conn: 1 });
        assert_eq!(
            Output::from(eng::Output::SetClipboard { text: "t".into() }),
            Output::SetClipboard { text: "t".into() }
        );
        assert_eq!(
            Output::from(eng::Output::Event(eng::Event::PairingModeEnded)),
            Output::Event { event: EngineEvent::PairingModeEnded }
        );
        let cases = [
            (eng::LocalChange::Sent { seq: 2, peers: 1 }, LocalChange::Sent { seq: 2, peers: 1 }),
            (eng::LocalChange::Unchanged, LocalChange::Unchanged),
            (eng::LocalChange::Empty, LocalChange::Empty),
            (eng::LocalChange::TooLarge, LocalChange::TooLarge),
        ];
        for (from, to) in cases {
            assert_eq!(LocalChange::from(from), to);
        }
    }

    #[test]
    fn errors_have_readable_messages() {
        assert_eq!(CoreError::InvalidDeviceId { id: "x".into() }.to_string(), "`x` is not a device ID");
        assert_eq!(CoreError::InvalidToken.to_string(), "not a pairing token");
        assert_eq!(CoreError::InvalidUri { reason: "m".into() }.to_string(), "invalid pairing URI: m");
        assert_eq!(
            CoreError::UnsupportedUriVersion { version: "2".into() }.to_string(),
            "unsupported pairing URI version 2"
        );
    }
}
