//! Clipboard backend for Wayland compositors with a data-control protocol
//! (`ext-data-control-v1`, or the older `wlr-data-control-unstable-v1`):
//! Hyprland, Sway and other wlroots compositors, KDE Plasma. GNOME has neither.
//!
//! The Wayland connection lives on its own thread with a calloop event loop.
//! Reading an offer and serving a paste each run on a short-lived thread, so a
//! slow or stuck peer client never blocks the event loop.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, anyhow};
use calloop::channel::{self, Channel, Sender};
use calloop::{EventLoop, LoopSignal};
use calloop_wayland_source::WaylandSource;
use clipsync_core::clip::MAX_TEXT_LEN;
use tracing::{debug, warn};
use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self as ext_device, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self as ext_offer, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self as ext_source, ExtDataControlSourceV1},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self as wlr_device, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self as wlr_offer, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self as wlr_source, ZwlrDataControlSourceV1},
};

use super::{ClipboardEvent, PASSWORD_MANAGER_HINT, SkipReason};

/// MIME types we read, most preferred first. All are UTF-8 in practice.
const READ_MIMES: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

/// MIME types we offer when we own the clipboard (same set as `wl-copy`).
const WRITE_MIMES: &[&str] = &["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING", "TEXT"];

type EventSink = Arc<dyn Fn(ClipboardEvent) + Send + Sync>;

enum Command {
    SetText(String),
    Stop,
}

/// Handle to the clipboard thread. Dropping it stops the thread, and with it
/// any text this process is serving.
pub struct WaylandClipboard {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
    protocol: &'static str,
}

impl WaylandClipboard {
    /// Connects to the compositor and starts watching the clipboard. `on_event`
    /// is called from backend threads.
    pub fn spawn(on_event: impl Fn(ClipboardEvent) + Send + Sync + 'static) -> Result<Self> {
        let (commands, channel) = channel::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let sink: EventSink = Arc::new(on_event);
        let thread =
            thread::Builder::new().name("wayland-clipboard".into()).spawn(move || run(sink, channel, ready_tx))?;
        match ready_rx.recv() {
            Ok(Ok(protocol)) => Ok(Self { commands, thread: Some(thread), protocol }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(anyhow!("clipboard thread exited during setup")),
        }
    }

    /// Name of the data-control protocol in use.
    pub fn protocol(&self) -> &'static str {
        self.protocol
    }

    /// Puts `text` on the clipboard and serves it until another client replaces it.
    pub fn set_text(&self, text: String) {
        let _ = self.commands.send(Command::SetText(text));
    }
}

impl Drop for WaylandClipboard {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(sink: EventSink, commands: Channel<Command>, ready: mpsc::Sender<Result<&'static str>>) {
    let mut event_loop = match EventLoop::<State>::try_new() {
        Ok(l) => l,
        Err(e) => return drop(ready.send(Err(e.into()))),
    };
    let mut state = match State::connect(sink.clone(), event_loop.get_signal()) {
        Ok((state, conn, queue)) => {
            let handle = event_loop.handle();
            let inserted = WaylandSource::new(conn, queue)
                .insert(handle.clone())
                .map_err(|e| anyhow!("{}", e.error))
                .and_then(|_| {
                    handle
                        .insert_source(commands, |event, _, state: &mut State| match event {
                            channel::Event::Msg(Command::SetText(text)) => state.set_text(text),
                            channel::Event::Msg(Command::Stop) | channel::Event::Closed => state.signal.stop(),
                        })
                        .map_err(|e| anyhow!("{}", e.error))
                });
            if let Err(e) = inserted {
                return drop(ready.send(Err(e)));
            }
            state
        }
        Err(e) => return drop(ready.send(Err(e))),
    };
    let _ = ready.send(Ok(state.manager.protocol()));
    if let Err(e) = event_loop.run(None, &mut state, |_| {}) {
        sink(ClipboardEvent::Closed(format!("wayland event loop failed: {e}")));
    }
}

/// Runs the same expression on whichever protocol variant `$value` holds.
macro_rules! either {
    ($value:expr, $inner:ident => $body:expr) => {
        match $value {
            Proto::Ext($inner) => $body,
            Proto::Wlr($inner) => $body,
        }
    };
}

/// One object of either data-control protocol. Both protocols are identical
/// apart from their names, so every call site is written once via `either!`.
#[derive(Clone, PartialEq)]
enum Proto<E, W> {
    Ext(E),
    Wlr(W),
}

type Manager = Proto<ExtDataControlManagerV1, ZwlrDataControlManagerV1>;
type Device = Proto<ExtDataControlDeviceV1, ZwlrDataControlDeviceV1>;
type Offer = Proto<ExtDataControlOfferV1, ZwlrDataControlOfferV1>;
type Source = Proto<ExtDataControlSourceV1, ZwlrDataControlSourceV1>;

impl Manager {
    fn bind(globals: &GlobalList, qh: &QueueHandle<State>) -> Result<Self> {
        if let Ok(m) = globals.bind(qh, 1..=1, ()) {
            return Ok(Proto::Ext(m));
        }
        globals.bind(qh, 2..=2, ()).map(Proto::Wlr).context(
            "the compositor supports neither ext-data-control-v1 nor wlr-data-control-unstable-v1 \
             (GNOME is not supported yet)",
        )
    }

    fn protocol(&self) -> &'static str {
        match self {
            Proto::Ext(_) => "ext-data-control-v1",
            Proto::Wlr(_) => "wlr-data-control-unstable-v1",
        }
    }

    fn get_device(&self, seat: &WlSeat, qh: &QueueHandle<State>) -> Device {
        match self {
            Proto::Ext(m) => Proto::Ext(m.get_data_device(seat, qh, ())),
            Proto::Wlr(m) => Proto::Wlr(m.get_data_device(seat, qh, ())),
        }
    }

    fn create_source(&self, qh: &QueueHandle<State>, data: Arc<[u8]>) -> Source {
        match self {
            Proto::Ext(m) => Proto::Ext(m.create_data_source(qh, data)),
            Proto::Wlr(m) => Proto::Wlr(m.create_data_source(qh, data)),
        }
    }
}

impl Device {
    fn set_selection(&self, source: &Source) {
        match (self, source) {
            (Proto::Ext(d), Proto::Ext(s)) => d.set_selection(Some(s)),
            (Proto::Wlr(d), Proto::Wlr(s)) => d.set_selection(Some(s)),
            _ => unreachable!("device and source come from the same manager"),
        }
    }
}

impl Offer {
    fn id(&self) -> ObjectId {
        either!(self, o => o.id())
    }

    fn receive(&self, mime: &str, fd: BorrowedFd<'_>) {
        either!(self, o => o.receive(mime.to_owned(), fd))
    }

    fn destroy(&self) {
        either!(self, o => o.destroy())
    }
}

impl Source {
    fn offer(&self, mime: &str) {
        either!(self, s => s.offer(mime.to_owned()))
    }

    fn destroy(&self) {
        either!(self, s => s.destroy())
    }
}

struct State {
    conn: Connection,
    qh: QueueHandle<State>,
    signal: LoopSignal,
    sink: EventSink,
    manager: Manager,
    device: Device,
    /// MIME types announced for each live offer.
    offers: HashMap<ObjectId, Vec<String>>,
    /// Offer describing the current clipboard selection.
    selection: Option<Offer>,
    /// Source we are serving, if we own the clipboard.
    own_source: Option<Source>,
    /// Private MIME type added to our sources so we recognise our own selection.
    marker: String,
    seen_first_selection: bool,
    /// Bumped on every selection change; a read whose generation is outdated is dropped.
    generation: Arc<AtomicU64>,
}

impl State {
    fn connect(sink: EventSink, signal: LoopSignal) -> Result<(Self, Connection, wayland_client::EventQueue<Self>)> {
        let conn = Connection::connect_to_env().context("cannot connect to the Wayland compositor")?;
        let (globals, queue) = registry_queue_init::<State>(&conn).context("wayland registry")?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=1, ()).context("the compositor has no seat")?;
        let manager = Manager::bind(&globals, &qh)?;
        let device = manager.get_device(&seat, &qh);
        let state = Self {
            conn: conn.clone(),
            qh,
            signal,
            sink,
            manager,
            device,
            offers: HashMap::new(),
            selection: None,
            own_source: None,
            marker: format!("application/x-clipsync-source-{}", clipsync_core::Hex::<8>::random()),
            seen_first_selection: false,
            generation: Arc::new(AtomicU64::new(0)),
        };
        Ok((state, conn, queue))
    }

    fn set_text(&mut self, text: String) {
        let source = self.manager.create_source(&self.qh, text.into_bytes().into());
        for mime in WRITE_MIMES {
            source.offer(mime);
        }
        source.offer(&self.marker);
        self.device.set_selection(&source);
        // The previous source, if any, gets `cancelled` and is destroyed there.
        self.own_source = Some(source);
    }

    fn forget_offer(&mut self, offer: &Offer) {
        self.offers.remove(&offer.id());
        offer.destroy();
    }

    fn on_selection(&mut self, offer: Option<Offer>) {
        let initial = !std::mem::replace(&mut self.seen_first_selection, true);
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(old) = self.selection.take() {
            self.forget_offer(&old);
        }
        let skipped = move |reason| ClipboardEvent::Skipped { reason, initial };
        let Some(offer) = offer else {
            return (self.sink)(skipped(SkipReason::Empty));
        };
        let mimes = self.offers.get(&offer.id()).cloned().unwrap_or_default();
        self.selection = Some(offer.clone());
        debug!(?mimes, initial, "selection changed");

        if mimes.contains(&self.marker) {
            // Reading our own offer would deadlock: we serve it on this thread.
            return (self.sink)(skipped(SkipReason::Own));
        }
        if mimes.iter().any(|m| m == PASSWORD_MANAGER_HINT) {
            return (self.sink)(skipped(SkipReason::Sensitive));
        }
        let Some(mime) = pick_text_mime(&mimes) else {
            return (self.sink)(skipped(SkipReason::NotText));
        };

        let (mut reader, writer) = match std::io::pipe() {
            Ok(pipe) => pipe,
            Err(e) => {
                warn!("cannot create pipe: {e}");
                return (self.sink)(skipped(SkipReason::ReadFailed));
            }
        };
        offer.receive(mime, writer.as_fd());
        // The fd is only sent on flush, so keep our end open until then.
        if let Err(e) = self.conn.flush() {
            warn!("wayland flush failed: {e}");
        }
        drop(writer);

        let latest = self.generation.clone();
        let sink = self.sink.clone();
        thread::spawn(move || {
            let mut data = Vec::new();
            let read = reader.by_ref().take(MAX_TEXT_LEN as u64 + 1).read_to_end(&mut data);
            if latest.load(Ordering::SeqCst) != generation {
                return; // the clipboard changed again while we were reading
            }
            let event = match read {
                Err(e) => {
                    warn!("reading clipboard failed: {e}");
                    skipped(SkipReason::ReadFailed)
                }
                Ok(_) if data.len() > MAX_TEXT_LEN => skipped(SkipReason::TooLarge),
                Ok(_) if data.is_empty() => skipped(SkipReason::Empty),
                Ok(_) => match String::from_utf8(data) {
                    Ok(text) => ClipboardEvent::Text { text, initial },
                    Err(_) => skipped(SkipReason::InvalidUtf8),
                },
            };
            sink(event);
        });
    }

    fn on_send(&self, mime: &str, fd: OwnedFd, data: Arc<[u8]>) {
        if mime == self.marker {
            return; // marker type carries no data; dropping fd closes the pipe
        }
        thread::spawn(move || {
            if let Err(e) = std::fs::File::from(fd).write_all(&data) {
                debug!("paste target closed early: {e}");
            }
        });
    }

    fn on_cancelled(&mut self, source: Source) {
        source.destroy();
        if self.own_source.as_ref() == Some(&source) {
            self.own_source = None;
            (self.sink)(ClipboardEvent::OwnershipLost);
        }
    }

    fn on_finished(&mut self) {
        (self.sink)(ClipboardEvent::Closed("the compositor removed the data-control device".into()));
        self.signal.stop();
    }
}

fn pick_text_mime(offered: &[String]) -> Option<&'static str> {
    READ_MIMES.iter().copied().find(|want| offered.iter().any(|m| m.eq_ignore_ascii_case(want)))
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, _: &WlSeat, _: <WlSeat as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// Dispatch glue for one protocol family; it only translates events into
/// calls on `State`, so both families share all the logic.
macro_rules! data_control_dispatch {
    ($variant:ident, $Manager:ty, $Device:ty, $device:ident, $Offer:ty, $offer:ident, $Source:ty, $source:ident) => {
        impl Dispatch<$Manager, ()> for State {
            fn event(_: &mut Self, _: &$Manager, _: <$Manager as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }

        impl Dispatch<$Device, ()> for State {
            fn event(state: &mut Self, _: &$Device, event: $device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
                match event {
                    $device::Event::DataOffer { id } => {
                        state.offers.insert(id.id(), Vec::new());
                    }
                    $device::Event::Selection { id } => state.on_selection(id.map(Proto::$variant)),
                    // Primary selection (middle click) is not synced.
                    $device::Event::PrimarySelection { id: Some(offer) } => state.forget_offer(&Proto::$variant(offer)),
                    $device::Event::Finished => state.on_finished(),
                    _ => {}
                }
            }

            event_created_child!(State, $Device, [$device::EVT_DATA_OFFER_OPCODE => ($Offer, ())]);
        }

        impl Dispatch<$Offer, ()> for State {
            fn event(state: &mut Self, offer: &$Offer, event: $offer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
                if let $offer::Event::Offer { mime_type } = event {
                    state.offers.entry(offer.id()).or_default().push(mime_type);
                }
            }
        }

        impl Dispatch<$Source, Arc<[u8]>> for State {
            fn event(state: &mut Self, source: &$Source, event: $source::Event, data: &Arc<[u8]>, _: &Connection, _: &QueueHandle<Self>) {
                match event {
                    $source::Event::Send { mime_type, fd } => state.on_send(&mime_type, fd, data.clone()),
                    $source::Event::Cancelled => state.on_cancelled(Proto::$variant(source.clone())),
                    _ => {}
                }
            }
        }
    };
}

data_control_dispatch!(
    Ext,
    ExtDataControlManagerV1,
    ExtDataControlDeviceV1,
    ext_device,
    ExtDataControlOfferV1,
    ext_offer,
    ExtDataControlSourceV1,
    ext_source
);
data_control_dispatch!(
    Wlr,
    ZwlrDataControlManagerV1,
    ZwlrDataControlDeviceV1,
    wlr_device,
    ZwlrDataControlOfferV1,
    wlr_offer,
    ZwlrDataControlSourceV1,
    wlr_source
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_utf8_mimes_case_insensitively() {
        let offered = |m: &[&str]| m.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            pick_text_mime(&offered(&["text/plain", "text/plain;charset=UTF-8"])),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(pick_text_mime(&offered(&["STRING", "UTF8_STRING"])), Some("UTF8_STRING"));
        assert_eq!(pick_text_mime(&offered(&["image/png", "STRING"])), None);
    }
}
