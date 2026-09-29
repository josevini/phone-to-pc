//! clipsync daemon: `clipsyncd` (or `clipsyncd run`) runs it; `watch` and `set` exercise
//! the clipboard backend on its own.

use std::io::Read;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use clipsync_core::{ClipTracker, DeviceId, Hex32, LocalOutcome};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use clipsyncd::clipboard::{ClipboardEvent, WaylandClipboard};
use clipsyncd::daemon::{self, DaemonConfig};
use clipsyncd::storage::config::default_device_name;
use clipsyncd::storage::{Config, Dirs, Identity};

#[derive(Parser)]
#[command(version, about = "Share the clipboard between paired devices")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon (the default).
    Run,
    /// Watch the clipboard and log what would be sent to peers.
    Watch {
        /// Also print a preview of the copied text.
        #[arg(long)]
        show_text: bool,
    },
    /// Put TEXT (or stdin) on the clipboard and serve it until another app copies.
    Set { text: Option<String> },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command.unwrap_or(Command::Run) {
        Command::Run => tokio::runtime::Runtime::new()?.block_on(run()),
        Command::Watch { show_text } => watch(show_text),
        Command::Set { text } => set(text),
    }
}

async fn run() -> Result<()> {
    let dirs = Dirs::from_env()?;
    let config = Config::load(&dirs.config_file(), default_device_name)?;
    let identity = Identity::load_or_create(&dirs.data)?;
    let (tx, events) = tokio::sync::mpsc::unbounded_channel();
    let clipboard = WaylandClipboard::spawn(move |event| {
        let _ = tx.send(event);
    })?;
    info!(protocol = clipboard.protocol(), "clipboard backend ready");

    let handle = spawn_daemon(dirs, config, identity, Arc::new(clipboard), events).await?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => handle.shutdown().await,
        _ = terminate.recv() => handle.shutdown().await,
        _ = handle.stopped() => bail!("the daemon stopped (see the log above)"),
    }
    Ok(())
}

/// Listens on every IPv6 and IPv4 address, or only IPv4 where IPv6 is unavailable.
async fn spawn_daemon(
    dirs: Dirs,
    config: Config,
    identity: Identity,
    clipboard: Arc<WaylandClipboard>,
    events: tokio::sync::mpsc::UnboundedReceiver<ClipboardEvent>,
) -> Result<daemon::DaemonHandle> {
    let port = config.port;
    let any_v6 = SocketAddr::from((Ipv6Addr::UNSPECIFIED, port));
    let listen = match std::net::TcpListener::bind(any_v6) {
        Ok(probe) => {
            drop(probe);
            any_v6
        }
        Err(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
    };
    let cfg = DaemonConfig { dirs, config, identity, platform: "linux".into(), listen, advertise: true };
    daemon::spawn(cfg, clipboard, events).await
}

fn watch(show_text: bool) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let clipboard = WaylandClipboard::spawn(move |event| {
        let _ = tx.send(event);
    })?;
    // A throwaway identity: there are no persistent device keys without pairing.
    let mut tracker = ClipTracker::new(DeviceId(Hex32::random()), 0);
    info!(protocol = clipboard.protocol(), "watching the clipboard; copy something (Ctrl-C to stop)");

    for event in rx {
        match event {
            ClipboardEvent::Text { text, initial } => {
                let preview = show_text.then(|| preview(&text));
                match tracker.local_change(text, unix_ms()) {
                    LocalOutcome::Emit(clip) => info!(
                        seq = clip.seq,
                        bytes = clip.text.len(),
                        sha256 = %&clip.sha256.to_string()[..12],
                        initial,
                        preview,
                        "{}",
                        if initial { "clipboard at startup" } else { "would send clip" },
                    ),
                    outcome => info!(?outcome, preview, "not sent"),
                }
            }
            ClipboardEvent::Skipped { reason, initial } => info!(?reason, initial, "skipped"),
            ClipboardEvent::OwnershipLost => {}
            ClipboardEvent::Closed(reason) => bail!("clipboard backend stopped: {reason}"),
        }
    }
    Ok(())
}

fn set(text: Option<String>) -> Result<()> {
    let text = match text {
        Some(t) => t,
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).context("reading stdin")?;
            buf
        }
    };
    if text.is_empty() {
        bail!("refusing to set an empty clipboard");
    }

    let (tx, rx) = mpsc::channel();
    let clipboard = WaylandClipboard::spawn(move |event| {
        let _ = tx.send(event);
    })?;
    let bytes = text.len();
    clipboard.set_text(text);
    info!(bytes, "clipboard set; serving it until another app copies (Ctrl-C to stop)");

    for event in rx {
        match event {
            ClipboardEvent::OwnershipLost => {
                info!("another app took the clipboard; exiting");
                return Ok(());
            }
            ClipboardEvent::Closed(reason) => bail!("clipboard backend stopped: {reason}"),
            ClipboardEvent::Text { .. } | ClipboardEvent::Skipped { .. } => {}
        }
    }
    warn!("clipboard backend exited");
    Ok(())
}

fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn preview(text: &str) -> String {
    const MAX_CHARS: usize = 60;
    let flat: String = text.chars().take(MAX_CHARS).map(|c| if c.is_control() { '⏎' } else { c }).collect();
    if text.chars().count() > MAX_CHARS { format!("{flat}…") } else { flat }
}
