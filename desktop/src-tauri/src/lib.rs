//! clipsync desktop app: a tray icon and a window over the running clipsync daemon (D13). The daemon does the work;
//! the app follows its status through the control socket and sends it requests, like the `clipsync` CLI.

pub mod autostart;
pub mod daemon;
pub mod pairing;
pub mod qr;
pub mod tray;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use clipsyncd::ipc::{Reply, Request, StatusView};
use clipsyncd::storage::Dirs;
use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, Wry};

use crate::autostart::Autostart;
use crate::pairing::{Invite, Pairing, PairingEvent};
use crate::tray::TrayView;

/// How often to look for the daemon while it is not running.
const RETRY: Duration = Duration::from_secs(2);
const ICON: &[u8] = include_bytes!("../icons/tray.png");
const ICON_DIMMED: &[u8] = include_bytes!("../icons/tray-dimmed.png");

/// What the window shows: the daemon's status while it runs, and the tray's view of it.
#[derive(Debug, Clone, Serialize)]
struct Shown {
    status: Option<StatusView>,
    view: TrayView,
}

struct App {
    socket: PathBuf,
    /// `None` without a home directory to keep the entry in.
    autostart: Option<Autostart>,
    shown: Mutex<Shown>,
    /// The pairing the window is following, if any.
    pairing: Mutex<Option<Pairing>>,
}

/// The parts of the tray that change with the status.
struct TrayItems {
    tray: TrayIcon,
    summary: MenuItem<Wry>,
    share: CheckMenuItem<Wry>,
}

/// What the window shows now; later changes arrive as `shown` events.
#[tauri::command]
fn shown(app: State<'_, App>) -> Shown {
    app.shown.lock().unwrap().clone()
}

#[tauri::command]
async fn set_paused(app: State<'_, App>, paused: bool) -> Result<(), String> {
    pause(&app.socket, paused).await
}

/// Whether the app starts with the session.
#[tauri::command]
fn autostart(app: State<'_, App>) -> bool {
    app.autostart.as_ref().is_some_and(Autostart::enabled)
}

#[tauri::command]
fn set_autostart(app: State<'_, App>, on: bool) -> Result<(), String> {
    let autostart = app.autostart.as_ref().ok_or("HOME is not set")?;
    let program = std::env::current_exe().map_err(|e| format!("cannot tell where the app is: {e}"))?;
    autostart.set(on, &program).map_err(|e| format!("{e}"))
}

/// Unpairs device `id`, telling it if it is connected.
#[tauri::command]
async fn unpair(app: State<'_, App>, id: String) -> Result<(), String> {
    daemon::unpair(&app.socket, &id).await.map(drop).map_err(|e| format!("{e:#}"))
}

/// Opens pairing mode and returns this device's code; what happens next arrives as `pairing` events.
#[tauri::command]
async fn show_code(handle: AppHandle, app: State<'_, App>) -> Result<Invite, String> {
    app.pairing.lock().unwrap().take();
    let (invite, pairing) = Pairing::show_code(&app.socket, reporter(handle)).await.map_err(|e| format!("{e:#}"))?;
    *app.pairing.lock().unwrap() = Some(pairing);
    Ok(invite)
}

/// Pairs with the device whose pairing link is `uri`; the outcome arrives as a `pairing` event.
#[tauri::command]
async fn pair_with_link(handle: AppHandle, app: State<'_, App>, uri: String) -> Result<(), String> {
    app.pairing.lock().unwrap().take();
    let pairing = Pairing::with_link(&app.socket, &uri, reporter(handle)).await.map_err(|e| format!("{e:#}"))?;
    *app.pairing.lock().unwrap() = Some(pairing);
    Ok(())
}

#[tauri::command]
fn confirm_pairing(app: State<'_, App>, accept: bool) {
    if let Some(pairing) = app.pairing.lock().unwrap().as_ref() {
        pairing.confirm(accept);
    }
}

/// Stops following the pairing; a code shown here stops working.
#[tauri::command]
fn stop_pairing(app: State<'_, App>) {
    app.pairing.lock().unwrap().take();
}

fn reporter(handle: AppHandle) -> impl FnMut(PairingEvent) + Send + 'static {
    move |event| {
        let _ = handle.emit("pairing", event);
    }
}

async fn pause(socket: &std::path::Path, paused: bool) -> Result<(), String> {
    let request = if paused { Request::Pause } else { Request::Resume };
    match daemon::request(socket, &request).await {
        Ok(Reply::Ok) => Ok(()),
        Ok(other) => Err(format!("unexpected reply from the daemon: {other:?}")),
        Err(e) => Err(format!("{e:#}")),
    }
}

pub fn run() {
    let socket = match Dirs::from_env() {
        Ok(dirs) => dirs.socket(),
        Err(e) => {
            eprintln!("clipsync: {e:#}");
            std::process::exit(1);
        }
    };
    let startup = Autostart::from_env();
    let initial = Shown { status: None, view: TrayView::of(None) };
    tauri::Builder::default()
        // First, as the plugin requires: starting the app again shows the running one's window instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| open_window(app)))
        .manage(App {
            socket: socket.clone(),
            autostart: startup,
            shown: Mutex::new(initial),
            pairing: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            shown,
            set_paused,
            show_code,
            pair_with_link,
            confirm_pairing,
            stop_pairing,
            unpair,
            autostart,
            set_autostart
        ])
        .setup(move |app| {
            let items = build_tray(app.handle())?;
            // `--hidden` starts in the tray only, as when started with the session.
            if !std::env::args().any(|arg| arg == "--hidden") {
                open_window(app.handle());
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(daemon::watch(socket, RETRY, move |status| show(&handle, &items, status)));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                // A hidden window cannot show a code or ask to confirm one.
                window.state::<App>().pairing.lock().unwrap().take();
                let _ = window.emit("closed", ());
            }
        })
        .run(tauri::generate_context!())
        .expect("clipsync could not start");
}

fn build_tray(app: &AppHandle) -> tauri::Result<TrayItems> {
    let view = TrayView::of(None);
    let summary = MenuItem::with_id(app, "summary", &view.summary, false, None::<&str>)?;
    let share = CheckMenuItem::with_id(app, "share", "Share the clipboard", false, true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open clipsync", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&summary, &separator, &share, &open, &separator, &quit])?;
    let toggled = share.clone();
    let tray = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(ICON_DIMMED)?)
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "share" => {
                // The item has already flipped; the daemon's next status sets it again either way.
                let paused = !toggled.is_checked().unwrap_or(true);
                let socket = app.state::<App>().socket.clone();
                tauri::async_runtime::spawn(async move { pause(&socket, paused).await });
            }
            "open" => open_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(TrayItems { tray, summary, share })
}

fn open_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Shows `status` in the tray and the window.
fn show(app: &AppHandle, items: &TrayItems, status: Option<StatusView>) {
    let view = TrayView::of(status.as_ref());
    let _ = items.summary.set_text(&view.summary);
    let _ = items.share.set_enabled(view.sharing.is_some());
    let _ = items.share.set_checked(view.sharing.unwrap_or(false));
    if let Ok(icon) = Image::from_bytes(if view.dimmed { ICON_DIMMED } else { ICON }) {
        let _ = items.tray.set_icon(Some(icon));
    }
    let shown = Shown { status, view };
    *app.state::<App>().shown.lock().unwrap() = shown.clone();
    let _ = app.emit("shown", shown);
}
