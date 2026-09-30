//! The control socket: newline-delimited JSON over a Unix socket in `$XDG_RUNTIME_DIR`.
//!
//! Each request gets one reply. `pair_start`, `pair_uri` and `pair_address` also keep
//! streaming the pairing's progress on the same connection until it succeeds or fails;
//! the client answers a `pairing_code` with `confirm`. Closing the connection that
//! opened pairing mode closes pairing mode.

use std::path::Path;

use anyhow::{Context, Result, bail};
use clipsync_core::DeviceId;
use clipsync_core::engine::{CloseReason, ConnId, Event, LocalChange};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use crate::daemon::{DaemonEvent, DaemonHandle, DeviceStatus, Status};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    /// Opens pairing mode; replies with the URI to show as a QR code.
    PairStart,
    PairStop,
    /// Pairs with the device whose QR code encodes `uri`.
    PairUri {
        uri: String,
    },
    /// Pairs by comparing codes with the device at `addr` (`ip:port`).
    PairAddress {
        addr: String,
    },
    /// The user's answer to a `pairing_code`.
    Confirm {
        conn: ConnId,
        accept: bool,
    },
    /// Sends `text` to the connected devices, as if copied here.
    Send {
        text: String,
    },
    /// Stops sharing the clipboard, keeping paired devices connected.
    Pause,
    Resume,
    /// Unpairs the device named by `device`: its ID, an ID prefix, or its name.
    Unpair {
        device: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Ok,
    Error { message: String },
    Status { status: StatusView },
    PairingStarted { uri: String, expires_in_s: u64 },
    Sent { outcome: String, peers: u32 },
    Unpaired { id: String, name: String },
    // Streamed during a pairing:
    PairingCode { conn: ConnId, id: String, name: String, code: String },
    Paired { id: String, name: String },
    PairingFailed { reason: String },
    PairingEnded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusView {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub addrs: Vec<String>,
    pub pairing: bool,
    pub paused: bool,
    pub devices: Vec<DeviceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub connected: bool,
}

impl From<Status> for StatusView {
    fn from(s: Status) -> Self {
        StatusView {
            id: s.id.to_string(),
            name: s.name,
            port: s.port,
            addrs: s.addrs.iter().map(|a| a.to_string()).collect(),
            pairing: s.pairing,
            paused: s.paused,
            devices: s
                .devices
                .into_iter()
                .map(|d| DeviceView { id: d.id.to_string(), name: d.name, connected: d.connected })
                .collect(),
        }
    }
}

// ---------------------------------------------------------------- server

/// Listens on `path` until the daemon stops. Refuses if another daemon answers there;
/// replaces a stale socket file left by one that died.
pub async fn serve(path: &Path, daemon: DaemonHandle) -> Result<()> {
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            bail!("another clipsync daemon is already running ({})", path.display());
        }
        std::fs::remove_file(path).with_context(|| format!("removing stale socket {}", path.display()))?;
    }
    let listener = UnixListener::bind(path).with_context(|| format!("listening on {}", path.display()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    tokio::spawn(async move {
        loop {
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => { tokio::spawn(serve_client(stream, daemon.clone())); }
                    Err(e) => warn!("control socket accept failed: {e}"),
                },
                _ = daemon.stopped() => break,
            }
        }
    });
    Ok(())
}

async fn serve_client(stream: UnixStream, daemon: DaemonHandle) {
    let (read, mut write) = stream.into_split();
    let (out, mut outgoing) = mpsc::unbounded_channel::<Reply>();
    tokio::spawn(async move {
        while let Some(reply) = outgoing.recv().await {
            let mut line = serde_json::to_vec(&reply).expect("replies always serialise");
            line.push(b'\n');
            if write.write_all(&line).await.is_err() {
                break;
            }
        }
    });

    let mut opened_pairing_mode = false;
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(e) => {
                let _ = out.send(Reply::Error { message: format!("invalid request: {e}") });
                continue;
            }
        };
        debug!(?request, "control request");
        opened_pairing_mode |= request == Request::PairStart;
        handle_request(request, &daemon, &out).await;
    }
    if opened_pairing_mode {
        daemon.stop_pairing();
    }
}

async fn handle_request(request: Request, daemon: &DaemonHandle, out: &mpsc::UnboundedSender<Reply>) {
    let reply = match request {
        Request::Status => Reply::Status { status: daemon.status().await.into() },
        Request::PairStart => {
            let events = daemon.subscribe();
            match daemon.start_pairing().await {
                Ok(invite) => {
                    tokio::spawn(forward_acceptor(events, out.clone()));
                    Reply::PairingStarted { uri: invite.uri, expires_in_s: invite.expires_in.as_secs() }
                }
                Err(e) => Reply::Error { message: format!("{e:#}") },
            }
        }
        Request::PairStop => {
            daemon.stop_pairing();
            Reply::Ok
        }
        Request::PairUri { uri } => {
            let events = daemon.subscribe();
            match daemon.pair_with_uri(&uri).await {
                Ok(()) => {
                    tokio::spawn(forward_dialer(events, out.clone()));
                    Reply::Ok
                }
                Err(e) => Reply::Error { message: format!("{e:#}") },
            }
        }
        Request::PairAddress { addr } => match addr.parse() {
            Ok(addr) => {
                let events = daemon.subscribe();
                match daemon.pair_with_address(addr).await {
                    Ok(()) => {
                        tokio::spawn(forward_dialer(events, out.clone()));
                        Reply::Ok
                    }
                    Err(e) => Reply::Error { message: format!("{e:#}") },
                }
            }
            Err(_) => Reply::Error { message: format!("{addr:?} is not an ip:port address") },
        },
        Request::Confirm { conn, accept } => {
            daemon.confirm_pairing(conn, accept).await;
            Reply::Ok
        }
        Request::Send { text } => {
            let (outcome, peers) = match daemon.send_text(text).await {
                LocalChange::Sent { peers, .. } => ("sent", peers),
                LocalChange::Unchanged => ("unchanged", 0),
                LocalChange::Empty => ("empty", 0),
                LocalChange::TooLarge => ("too_large", 0),
                LocalChange::Paused => ("paused", 0),
            };
            Reply::Sent { outcome: outcome.into(), peers }
        }
        Request::Pause | Request::Resume => {
            daemon.set_paused(request == Request::Pause).await;
            Reply::Ok
        }
        Request::Unpair { device } => {
            let status = daemon.status().await;
            match resolve(&status.devices, &device) {
                Ok(found) => {
                    let found = found.clone();
                    daemon.unpair(found.id).await;
                    Reply::Unpaired { id: found.id.to_string(), name: found.name }
                }
                Err(message) => Reply::Error { message },
            }
        }
    };
    let _ = out.send(reply);
}

/// Streams the progress of pairing mode until it ends.
async fn forward_acceptor(mut events: broadcast::Receiver<DaemonEvent>, out: mpsc::UnboundedSender<Reply>) {
    while let Ok(event) = events.recv().await {
        let reply = match event {
            DaemonEvent::Engine(Event::PairingCode { conn, peer, name, code }) => code_reply(conn, peer, name, code),
            DaemonEvent::Engine(Event::Paired { device }) => {
                Reply::Paired { id: device.id.to_string(), name: device.name }
            }
            DaemonEvent::Engine(Event::PairingModeEnded) => {
                let _ = out.send(Reply::PairingEnded);
                return;
            }
            _ => continue,
        };
        if out.send(reply).is_err() {
            return;
        }
    }
}

/// Streams the progress of one pairing this device dialed, until it succeeds or fails.
async fn forward_dialer(mut events: broadcast::Receiver<DaemonEvent>, out: mpsc::UnboundedSender<Reply>) {
    let mut ours: Option<ConnId> = None;
    while let Ok(event) = events.recv().await {
        let (reply, done) = match event {
            DaemonEvent::PairingConnection { conn } if ours.is_none() => {
                ours = Some(conn);
                continue;
            }
            DaemonEvent::DialFailed { error, .. } if ours.is_none() => (Reply::PairingFailed { reason: error }, true),
            DaemonEvent::Engine(Event::PairingCode { conn, peer, name, code }) if Some(conn) == ours => {
                (code_reply(conn, peer, name, code), false)
            }
            DaemonEvent::Engine(Event::Paired { device }) if ours.is_some() => {
                (Reply::Paired { id: device.id.to_string(), name: device.name }, true)
            }
            DaemonEvent::Engine(Event::ConnectionClosed { conn, reason, .. }) if Some(conn) == ours => {
                (Reply::PairingFailed { reason: describe(&reason) }, true)
            }
            _ => continue,
        };
        if out.send(reply).is_err() || done {
            return;
        }
    }
}

fn code_reply(conn: ConnId, peer: DeviceId, name: String, code: u32) -> Reply {
    Reply::PairingCode { conn, id: peer.to_string(), name, code: format!("{code:06}") }
}

/// Why a pairing connection closed, in words for the user.
fn describe(reason: &CloseReason) -> String {
    match reason {
        CloseReason::RejectedByPeer => "the code was rejected on the other device".into(),
        CloseReason::RejectedLocally => "the code was rejected here".into(),
        CloseReason::RemoteError { code } => match code.as_str() {
            "pairing_closed" | "not_paired" => "the other device is not in pairing mode".into(),
            "bad_token" => "the QR code is no longer valid; show a new one".into(),
            other => format!("the other device refused ({other})"),
        },
        CloseReason::Timeout => "the other device stopped answering".into(),
        CloseReason::Closed => "the connection closed".into(),
        other => format!("{other:?}"),
    }
}

/// Finds the paired device a user named: by full ID, unique ID prefix (4+ characters) or unique name.
fn resolve<'a>(devices: &'a [DeviceStatus], selector: &str) -> Result<&'a DeviceStatus, String> {
    let lower = selector.to_ascii_lowercase();
    let by_id = |d: &&DeviceStatus| {
        let id = d.id.to_string();
        id == lower || (lower.len() >= 4 && id.starts_with(&lower))
    };
    let by_name = |d: &&DeviceStatus| d.name.eq_ignore_ascii_case(selector);
    for matches in [devices.iter().filter(by_id).collect::<Vec<_>>(), devices.iter().filter(by_name).collect()] {
        match matches.as_slice() {
            [one] => return Ok(one),
            [] => continue,
            _ => return Err(format!("{selector:?} matches more than one device; use its ID")),
        }
    }
    Err(format!("no paired device matches {selector:?}"))
}

// ---------------------------------------------------------------- client

/// A connection to the control socket.
pub struct Client {
    lines: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
}

impl Client {
    pub async fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .await
            .with_context(|| format!("cannot reach the clipsync daemon at {} — is it running?", path.display()))?;
        let (read, writer) = stream.into_split();
        Ok(Client { lines: BufReader::new(read).lines(), writer })
    }

    pub async fn send(&mut self, request: &Request) -> Result<()> {
        let mut line = serde_json::to_vec(request)?;
        line.push(b'\n');
        self.writer.write_all(&line).await?;
        Ok(())
    }

    /// The next reply or streamed event; `None` when the daemon closed the connection.
    pub async fn next(&mut self) -> Result<Option<Reply>> {
        match self.lines.next_line().await? {
            Some(line) => Ok(Some(serde_json::from_str(&line).context("invalid reply from the daemon")?)),
            None => Ok(None),
        }
    }

    /// Sends `request` and returns its reply.
    pub async fn request(&mut self, request: &Request) -> Result<Reply> {
        self.send(request).await?;
        self.next().await?.context("the daemon closed the connection")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipsync_core::Hex;

    fn device(byte: u8, name: &str) -> DeviceStatus {
        DeviceStatus { id: DeviceId(Hex([byte; 32])), name: name.into(), connected: false }
    }

    #[test]
    fn devices_are_found_by_id_prefix_or_name() {
        let devices = [device(0xab, "phone"), device(0xcd, "desktop"), device(0xce, "Laptop")];
        assert_eq!(resolve(&devices, "phone").unwrap().name, "phone");
        assert_eq!(resolve(&devices, "laptop").unwrap().name, "Laptop");
        assert_eq!(resolve(&devices, "ABAB").unwrap().name, "phone");
        assert_eq!(resolve(&devices, &"cd".repeat(32)).unwrap().name, "desktop");
        assert!(resolve(&devices, "ab").is_err(), "prefixes need 4 characters");
        assert!(resolve(&devices, "tablet").is_err());
    }

    #[test]
    fn ambiguous_selectors_are_refused() {
        let devices = [device(0xab, "phone"), device(0xac, "phone")];
        assert!(resolve(&devices, "phone").unwrap_err().contains("more than one"));
    }

    #[test]
    fn requests_and_replies_are_tagged_json() {
        let json = serde_json::to_string(&Request::Confirm { conn: 3, accept: true }).unwrap();
        assert_eq!(json, r#"{"cmd":"confirm","conn":3,"accept":true}"#);
        let reply: Reply = serde_json::from_str(r#"{"type":"paired","id":"ab","name":"phone"}"#).unwrap();
        assert_eq!(reply, Reply::Paired { id: "ab".into(), name: "phone".into() });
    }

    #[test]
    fn close_reasons_are_described_for_people() {
        assert!(describe(&CloseReason::RejectedByPeer).contains("rejected"));
        assert!(describe(&CloseReason::RemoteError { code: "bad_token".into() }).contains("QR code"));
        assert!(describe(&CloseReason::RemoteError { code: "pairing_closed".into() }).contains("pairing mode"));
        assert!(describe(&CloseReason::RemoteError { code: "odd".into() }).contains("odd"));
    }
}
