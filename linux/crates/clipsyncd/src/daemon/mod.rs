//! The daemon runtime: one actor task owns the protocol [`Engine`] and everything it
//! drives — connections, dialing, the clipboard and the persisted state. Other tasks
//! (sockets, the clipboard backend, the control socket) talk to it through [`Cmd`]s.

mod net;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use clipsync_core::DeviceId;
use clipsync_core::engine::{ConnId, Engine, Event, Intent, LocalChange, LocalDevice, Output, PairedDevice, Role};
use clipsync_core::pairing::PairUri;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, info, warn};

use crate::clipboard::{Clipboard, ClipboardEvent};
use crate::discovery::{Discovery, is_link_local_v6};
use crate::storage::state::PairedRecord;
use crate::storage::{Config, Dirs, Identity, State};
use crate::tls::Tls;

/// How often the engine's timers are driven and failed dials retried.
const TICK: Duration = Duration::from_secs(1);
const RETRY_MIN: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);

pub struct DaemonConfig {
    pub dirs: Dirs,
    pub config: Config,
    pub identity: Identity,
    /// `hello.platform`.
    pub platform: String,
    /// Address to listen on.
    pub listen: SocketAddr,
    /// Whether to advertise and browse with mDNS.
    pub advertise: bool,
}

/// What the daemon reports to observers (the control socket, notifications, tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonEvent {
    Engine(Event),
    DialFailed {
        addrs: Vec<SocketAddr>,
        error: String,
    },
    /// A connection this device dialed to pair is up; later events name it by `conn`.
    PairingConnection {
        conn: ConnId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub id: DeviceId,
    pub name: String,
    pub port: u16,
    /// Addresses other devices can reach this one on.
    pub addrs: Vec<SocketAddr>,
    pub pairing: bool,
    pub devices: Vec<DeviceStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStatus {
    pub id: DeviceId,
    pub name: String,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingInvite {
    /// The pairing URI to show as a QR code (spec §8).
    pub uri: String,
    pub expires_in: Duration,
}

pub(crate) enum Cmd {
    Opened {
        conn: ConnId,
        role: Role,
        peer: DeviceId,
        intent: Intent,
        dialed: Option<SocketAddr>,
        writer: mpsc::UnboundedSender<Vec<u8>>,
    },
    Bytes {
        conn: ConnId,
        bytes: Vec<u8>,
    },
    Closed {
        conn: ConnId,
    },
    DialFailed {
        addrs: Vec<SocketAddr>,
        error: String,
    },
    Discovered {
        id: DeviceId,
        addrs: Vec<SocketAddr>,
    },
    Clipboard(ClipboardEvent),
    Status(oneshot::Sender<Status>),
    StartPairing(oneshot::Sender<Result<PairingInvite>>),
    StopPairing,
    PairWithUri(String, oneshot::Sender<Result<()>>),
    PairWithAddress(SocketAddr),
    ConfirmPairing {
        conn: ConnId,
        accept: bool,
    },
    SendText(String, oneshot::Sender<LocalChange>),
    Unpair(DeviceId, oneshot::Sender<bool>),
    Shutdown(oneshot::Sender<()>),
}

/// Control handle to a running daemon; cheap to clone.
#[derive(Clone)]
pub struct DaemonHandle {
    cmds: mpsc::UnboundedSender<Cmd>,
    events: broadcast::Sender<DaemonEvent>,
}

impl DaemonHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<DaemonEvent> {
        self.events.subscribe()
    }

    pub async fn status(&self) -> Status {
        self.ask(Cmd::Status).await.expect("daemon is running")
    }

    /// Opens pairing mode and returns what to show as a QR code.
    pub async fn start_pairing(&self) -> Result<PairingInvite> {
        self.ask(Cmd::StartPairing).await.ok_or_else(|| anyhow!("daemon stopped"))?
    }

    pub fn stop_pairing(&self) {
        let _ = self.cmds.send(Cmd::StopPairing);
    }

    /// Pairs with the device whose pairing URI (QR code) is `uri` (spec §7.2).
    pub async fn pair_with_uri(&self, uri: &str) -> Result<()> {
        let uri = uri.to_owned();
        self.ask(|tx| Cmd::PairWithUri(uri, tx)).await.ok_or_else(|| anyhow!("daemon stopped"))?
    }

    /// Pairs by comparing codes with the device at `addr` (spec §7.3).
    pub async fn pair_with_address(&self, addr: SocketAddr) -> Result<()> {
        self.cmds.send(Cmd::PairWithAddress(addr)).map_err(|_| anyhow!("daemon stopped"))
    }

    pub async fn confirm_pairing(&self, conn: ConnId, accept: bool) {
        let _ = self.cmds.send(Cmd::ConfirmPairing { conn, accept });
    }

    /// Sends `text` to the connected peers as if it had been copied here.
    pub async fn send_text(&self, text: String) -> LocalChange {
        self.ask(|tx| Cmd::SendText(text, tx)).await.unwrap_or(LocalChange::Unchanged)
    }

    /// Unpairs `peer`; false if it was not paired.
    pub async fn unpair(&self, peer: DeviceId) -> bool {
        self.ask(|tx| Cmd::Unpair(peer, tx)).await.unwrap_or(false)
    }

    /// Stops the daemon and waits until it has.
    pub async fn shutdown(&self) {
        let _ = self.ask(Cmd::Shutdown).await;
    }

    /// Reports device `id` reachable at `addrs`, as found by mDNS. Paired devices are dialed
    /// there until connected; anything else is ignored.
    pub fn discovered(&self, id: DeviceId, addrs: Vec<SocketAddr>) {
        let _ = self.cmds.send(Cmd::Discovered { id, addrs });
    }

    /// Resolves when the daemon has stopped.
    pub async fn stopped(&self) {
        self.cmds.closed().await;
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        self.cmds.send(make(tx)).ok()?;
        rx.await.ok()
    }
}

/// Starts a daemon: binds the listener, then runs until [`DaemonHandle::shutdown`].
pub async fn spawn(
    cfg: DaemonConfig,
    clipboard: Arc<dyn Clipboard>,
    mut clipboard_events: mpsc::UnboundedReceiver<ClipboardEvent>,
) -> Result<DaemonHandle> {
    let tls = Tls::new(&cfg.identity)?;
    let listener = TcpListener::bind(cfg.listen).await.with_context(|| format!("listening on {}", cfg.listen))?;
    let local = listener.local_addr()?;
    let state = State::load(&cfg.dirs.state_file())?;

    let (cmds, rx) = mpsc::unbounded_channel();
    let (events, _) = broadcast::channel(256);
    let handle = DaemonHandle { cmds: cmds.clone(), events: events.clone() };

    let ids = net::ConnIds::default();
    tokio::spawn(net::accept_loop(listener, tls.clone(), ids.clone(), cmds.clone()));
    let forward = cmds.clone();
    tokio::spawn(async move {
        while let Some(event) = clipboard_events.recv().await {
            if forward.send(Cmd::Clipboard(event)).is_err() {
                break;
            }
        }
    });

    let me = LocalDevice { id: cfg.identity.id, name: cfg.config.name.clone(), platform: cfg.platform.clone() };
    let paired = state.paired.iter().map(|p| PairedDevice { id: p.id, name: p.name.clone() }).collect();
    let engine = Engine::new(me.clone(), paired, state.lamport);
    let mut targets = HashMap::new();
    for addr in &cfg.config.peers {
        targets.insert(*addr, Target::new(None));
    }
    let actor = Actor {
        me,
        engine,
        state,
        state_file: cfg.dirs.state_file(),
        local,
        clipboard,
        tls,
        ids,
        cmds,
        events,
        writers: HashMap::new(),
        targets,
    };
    info!(id = %cfg.identity.id.short(), name = %cfg.config.name, %local, "clipsync daemon started");
    let discovery = if cfg.advertise {
        match Discovery::start(cfg.identity.id, &cfg.config.name, local.port(), handle.clone()) {
            Ok(discovery) => Some(discovery),
            Err(e) => {
                warn!("mDNS unavailable, only configured peers will be dialed: {e:#}");
                None
            }
        }
    } else {
        None
    };
    tokio::spawn(async move {
        actor.run(rx).await;
        drop(discovery);
    });
    Ok(handle)
}

/// An address to keep dialing, with its retry schedule.
struct Target {
    /// The device found there, once known.
    device: Option<DeviceId>,
    dialing: bool,
    retry_at: tokio::time::Instant,
    delay: Duration,
}

impl Target {
    fn new(device: Option<DeviceId>) -> Self {
        Target { device, dialing: false, retry_at: tokio::time::Instant::now(), delay: RETRY_MIN }
    }
}

struct Actor {
    me: LocalDevice,
    engine: Engine,
    state: State,
    state_file: std::path::PathBuf,
    local: SocketAddr,
    clipboard: Arc<dyn Clipboard>,
    tls: Tls,
    ids: net::ConnIds,
    cmds: mpsc::UnboundedSender<Cmd>,
    events: broadcast::Sender<DaemonEvent>,
    writers: HashMap<ConnId, mpsc::UnboundedSender<Vec<u8>>>,
    /// Addresses to reconnect to: configured peers and where paired devices were found.
    targets: HashMap<SocketAddr, Target>,
}

impl Actor {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Cmd>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    if let Cmd::Shutdown(done) = cmd {
                        self.writers.clear();
                        let _ = done.send(());
                        break;
                    }
                    if let Cmd::Clipboard(ClipboardEvent::Closed(reason)) = &cmd {
                        // Without a clipboard there is nothing to do: stop, so a supervisor restarts us.
                        warn!("clipboard backend stopped: {reason}");
                        break;
                    }
                    self.handle(cmd);
                }
                _ = tick.tick() => {
                    self.engine.tick(now_ms());
                    self.redial();
                }
            }
            self.drain();
        }
        info!("clipsync daemon stopped");
    }

    fn handle(&mut self, cmd: Cmd) {
        let now = now_ms();
        match cmd {
            Cmd::Opened { conn, role, peer, intent, dialed, writer } => {
                if let Some(addr) = dialed
                    && let Some(target) = self.targets.get_mut(&addr)
                {
                    target.dialing = false;
                    target.device = Some(peer);
                    target.delay = RETRY_MIN;
                }
                self.writers.insert(conn, writer);
                if intent != Intent::Session {
                    let _ = self.events.send(DaemonEvent::PairingConnection { conn });
                }
                self.engine.connection_opened(conn, role, peer, intent, now);
            }
            Cmd::Bytes { conn, bytes } => self.engine.bytes_received(conn, &bytes, now),
            Cmd::Closed { conn } => {
                self.writers.remove(&conn);
                self.engine.connection_closed(conn);
            }
            Cmd::DialFailed { addrs, error } => {
                debug!(?addrs, "dial failed: {error}");
                for addr in &addrs {
                    if let Some(target) = self.targets.get_mut(addr) {
                        target.dialing = false;
                        target.retry_at = tokio::time::Instant::now() + target.delay;
                        target.delay = (target.delay * 2).min(RETRY_MAX);
                    }
                }
                let _ = self.events.send(DaemonEvent::DialFailed { addrs, error });
            }
            Cmd::Discovered { id, addrs } => {
                // Remember where every device is; `redial` only dials the paired ones.
                for addr in addrs {
                    self.targets.entry(addr).or_insert_with(|| Target::new(Some(id))).device = Some(id);
                }
                self.redial();
            }
            Cmd::Clipboard(event) => self.on_clipboard(event, now),
            Cmd::Status(reply) => {
                let _ = reply.send(self.status(now));
            }
            Cmd::StartPairing(reply) => {
                let token = self.engine.start_pairing(now);
                let _ = reply.send(self.invite(token));
            }
            Cmd::StopPairing => self.engine.stop_pairing(),
            Cmd::PairWithUri(uri, reply) => {
                let result = PairUri::parse(&uri).map_err(anyhow::Error::from).map(|uri| {
                    let intent = Intent::PairToken { token: uri.token };
                    self.dial(uri.addrs, Some(uri.id), intent);
                });
                let _ = reply.send(result);
            }
            Cmd::PairWithAddress(addr) => self.dial(vec![addr], None, Intent::PairSas),
            Cmd::ConfirmPairing { conn, accept } => self.engine.confirm_pairing(conn, accept, now),
            Cmd::SendText(text, reply) => {
                let _ = reply.send(self.engine.local_clipboard_changed(text, now));
            }
            Cmd::Unpair(peer, reply) => {
                let known = self.state.paired.iter().any(|p| p.id == peer);
                self.engine.unpair(&peer);
                let _ = reply.send(known);
            }
            Cmd::Shutdown(_) => unreachable!("handled by run"),
        }
    }

    fn on_clipboard(&mut self, event: ClipboardEvent, now: u64) {
        match event {
            // Content that was already there when the daemon started is not sent.
            ClipboardEvent::Text { initial: true, .. } => {}
            ClipboardEvent::Text { text, initial: false } => {
                let outcome = self.engine.local_clipboard_changed(text, now);
                debug!(?outcome, "local clipboard changed");
            }
            ClipboardEvent::Skipped { reason, .. } => debug!(?reason, "clipboard change not synced"),
            ClipboardEvent::OwnershipLost => {}
            ClipboardEvent::Closed(_) => unreachable!("handled by run"),
        }
    }

    /// Carries out everything the engine queued.
    fn drain(&mut self) {
        while let Some(output) = self.engine.poll_output() {
            match output {
                Output::Send { conn, bytes } => {
                    if let Some(writer) = self.writers.get(&conn) {
                        let _ = writer.send(bytes);
                    }
                }
                Output::Close { conn } => {
                    self.writers.remove(&conn);
                }
                Output::SetClipboard { text } => self.clipboard.set_text(text),
                Output::Event(event) => self.on_event(event),
            }
        }
        if self.engine.lamport() != self.state.lamport {
            self.state.lamport = self.engine.lamport();
            self.save();
        }
    }

    fn on_event(&mut self, event: Event) {
        match &event {
            Event::Paired { device } => {
                self.state.paired.retain(|p| p.id != device.id);
                self.state.paired.push(PairedRecord { id: device.id, name: device.name.clone() });
                self.save();
                info!(peer = %device.id.short(), name = %device.name, "paired");
            }
            Event::Unpaired { peer } => {
                self.state.paired.retain(|p| p.id != *peer);
                self.targets.retain(|_, t| t.device != Some(*peer));
                self.save();
                info!(peer = %peer.short(), "unpaired");
            }
            Event::PeerConnected { peer, name } => info!(peer = %peer.short(), %name, "connected"),
            Event::PeerDisconnected { peer } => {
                info!(peer = %peer.short(), "disconnected");
                // Reconnect soon, starting the backoff over.
                for target in self.targets.values_mut().filter(|t| t.device == Some(*peer)) {
                    target.retry_at = tokio::time::Instant::now() + RETRY_MIN;
                    target.delay = RETRY_MIN;
                }
            }
            Event::ConnectionClosed { reason, peer, .. } => debug!(peer = %peer.short(), ?reason, "connection closed"),
            _ => {}
        }
        let _ = self.events.send(DaemonEvent::Engine(event));
    }

    /// Dials targets whose device is paired but not connected, when their retry time has come.
    fn redial(&mut self) {
        let now = tokio::time::Instant::now();
        let mut due = vec![];
        for (addr, target) in &self.targets {
            let wanted = match target.device {
                Some(id) => self.state.paired.iter().any(|p| p.id == id) && !self.engine.is_connected(&id),
                None => true,
            };
            if wanted && !target.dialing && now >= target.retry_at {
                due.push((*addr, target.device));
            }
        }
        for (addr, device) in due {
            if let Some(target) = self.targets.get_mut(&addr) {
                target.dialing = true;
                // Pushed back until the dial reports; a successful one resets the schedule.
                target.retry_at = now + target.delay;
            }
            self.dial(vec![addr], device, Intent::Session);
        }
    }

    fn dial(&self, addrs: Vec<SocketAddr>, expected: Option<DeviceId>, intent: Intent) {
        net::dial(addrs, expected, intent, self.tls.clone(), self.ids.clone(), self.cmds.clone());
    }

    fn status(&self, now: u64) -> Status {
        let devices = self
            .state
            .paired
            .iter()
            .map(|p| DeviceStatus { id: p.id, name: p.name.clone(), connected: self.engine.is_connected(&p.id) })
            .collect();
        let (id, name) = (self.me.id, self.me.name.clone());
        let (port, addrs) = (self.local.port(), advertised_addrs(self.local));
        Status { id, name, port, addrs, pairing: self.engine.pairing_active(now), devices }
    }

    fn invite(&self, token: clipsync_core::Hex16) -> Result<PairingInvite> {
        let addrs = advertised_addrs(self.local);
        let uri = PairUri { id: self.me.id, name: self.me.name.clone(), addrs, token };
        Ok(PairingInvite {
            uri: uri.to_uri(),
            expires_in: Duration::from_millis(clipsync_core::engine::PAIRING_WINDOW_MS),
        })
    }

    fn save(&self) {
        if let Err(e) = self.state.save(&self.state_file) {
            warn!("could not save state: {e:#}");
        }
    }
}

/// Addresses other devices can reach this one on: the listening address itself, or every
/// non-loopback interface address when listening on all of them.
fn advertised_addrs(local: SocketAddr) -> Vec<SocketAddr> {
    if !local.ip().is_unspecified() {
        return vec![local];
    }
    let mut addrs: Vec<SocketAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|iface| iface.ip())
        .filter(|ip| !ip.is_loopback() && !is_link_local_v6(ip))
        .map(|ip| SocketAddr::new(ip, local.port()))
        .collect();
    addrs.sort_by_key(|a| a.is_ipv6());
    addrs
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
