//! clipsync daemon.
//!
//! It exposes the clipboard backend through two debug commands; networking and
//! pairing are not implemented yet.

use std::io::Read;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use clipsync_core::{ClipTracker, DeviceId, Hex32, LocalOutcome};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use clipsyncd::clipboard::{ClipboardEvent, WaylandClipboard};

#[derive(Parser)]
#[command(version, about = "Share the clipboard between paired devices")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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

    match Cli::parse().command {
        Command::Watch { show_text } => watch(show_text),
        Command::Set { text } => set(text),
    }
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
