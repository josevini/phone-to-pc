//! `clipsync`: controls the clipsync daemon through its control socket.

use std::io::{BufRead, Read, Write};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use clipsyncd::ipc::{Client, DeviceView, Reply, Request, StatusView};
use clipsyncd::storage::Dirs;
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;

#[derive(Parser)]
#[command(version, about = "Share the clipboard between paired devices")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show this device and its paired devices (the default).
    Status,
    /// List paired devices.
    Devices,
    /// Pair a device. Without TARGET, show a QR code and wait for a device to pair with
    /// this one; with a pairing URI, pair with the device that shows it; with an
    /// IP:PORT address, pair with that device by comparing codes.
    Pair { target: Option<String> },
    /// Send TEXT (or standard input) to the connected devices.
    Send { text: Option<String> },
    /// Forget a paired device, named by its name, ID or ID prefix.
    Unpair { device: String },
    /// Stop sharing the clipboard without unpairing: nothing is sent or received until `clipsync resume`.
    Pause,
    /// Share the clipboard again after `clipsync pause`.
    Resume,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let command = Cli::parse().command.unwrap_or(Command::Status);
    let mut client = Client::connect(&Dirs::from_env()?.socket()).await?;
    match command {
        Command::Status => print!("{}", render_status(&status(&mut client).await?)),
        Command::Devices => print!("{}", render_devices(&status(&mut client).await?.devices)),
        Command::Pair { target: None } => accept_pairing(&mut client).await?,
        Command::Pair { target: Some(target) } => dial_pairing(&mut client, &target).await?,
        Command::Send { text } => send(&mut client, text).await?,
        Command::Unpair { device } => match client.request(&Request::Unpair { device }).await? {
            Reply::Unpaired { id, name } => println!("Unpaired {name} ({}).", short(&id)),
            other => fail(other)?,
        },
        Command::Pause => match client.request(&Request::Pause).await? {
            Reply::Ok => println!(
                "Paused: this device neither sends nor receives the clipboard, and stays paired.\n\
                 Run `clipsync resume` to share it again."
            ),
            other => fail(other)?,
        },
        Command::Resume => match client.request(&Request::Resume).await? {
            Reply::Ok => println!("Resumed: the clipboard is shared with the paired devices again."),
            other => fail(other)?,
        },
    }
    Ok(())
}

async fn status(client: &mut Client) -> Result<StatusView> {
    match client.request(&Request::Status).await? {
        Reply::Status { status } => Ok(status),
        other => fail(other),
    }
}

/// Shows the invitation and waits until a device pairs, or pairing mode closes.
async fn accept_pairing(client: &mut Client) -> Result<()> {
    let (uri, expires_in_s) = match client.request(&Request::PairStart).await? {
        Reply::PairingStarted { uri, expires_in_s } => (uri, expires_in_s),
        other => return fail(other),
    };
    println!("Scan this code on the other device, or run there: clipsync pair '<the URI below>'\n");
    println!("{}", render_qr(&uri)?);
    println!("{uri}\n");
    println!("Waiting for a device… (pairing mode closes in {} minutes; Ctrl-C to stop)", expires_in_s.div_ceil(60));
    follow_pairing(client, true).await
}

async fn dial_pairing(client: &mut Client, target: &str) -> Result<()> {
    let request = if target.starts_with("clipsync://") {
        Request::PairUri { uri: target.into() }
    } else {
        Request::PairAddress { addr: target.into() }
    };
    match client.request(&request).await? {
        Reply::Ok => follow_pairing(client, false).await,
        other => fail(other),
    }
}

/// Reacts to the pairing's progress until it succeeds or fails.
async fn follow_pairing(client: &mut Client, accepting: bool) -> Result<()> {
    while let Some(reply) = client.next().await? {
        match reply {
            Reply::PairingCode { conn, id, name, code } => {
                let question = format!(
                    "{name} ({}) wants to pair.\nCode: {} {}\nDoes the other device show the same code? [y/N] ",
                    short(&id),
                    &code[..3],
                    &code[3..]
                );
                let accept = ask(question).await?;
                client.send(&Request::Confirm { conn, accept }).await?;
                if !accept {
                    bail!("pairing rejected");
                }
            }
            Reply::Paired { id, name } => {
                println!("Paired with {name} ({}).", short(&id));
                return Ok(());
            }
            Reply::PairingFailed { reason } => bail!("pairing failed: {reason}"),
            Reply::PairingEnded if accepting => bail!("pairing mode closed before a device paired"),
            Reply::Error { message } => bail!(message),
            _ => {}
        }
    }
    bail!("the daemon closed the connection")
}

async fn send(client: &mut Client, text: Option<String>) -> Result<()> {
    let text = match text {
        Some(text) => text,
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };
    match client.request(&Request::Send { text }).await? {
        Reply::Sent { outcome, peers } => match outcome.as_str() {
            "sent" if peers > 0 => println!("Sent to {peers} device{}.", if peers == 1 { "" } else { "s" }),
            "sent" => bail!("no paired device is connected; nothing was sent"),
            "unchanged" => bail!("this text was the last one sent or received; nothing was sent"),
            "empty" => bail!("nothing to send"),
            "too_large" => bail!("the text is larger than 1 MiB"),
            "paused" => bail!("sharing is paused; nothing was sent (run `clipsync resume` to share again)"),
            other => bail!("unexpected outcome {other:?}"),
        },
        other => return fail(other),
    }
    Ok(())
}

/// Asks a yes/no question on the terminal; anything but "y" or "yes" is no.
async fn ask(question: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        print!("{question}");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer)?;
        Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
    })
    .await?
}

fn fail<T>(reply: Reply) -> Result<T> {
    match reply {
        Reply::Error { message } => bail!(message),
        other => bail!("unexpected reply from the daemon: {other:?}"),
    }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn render_status(status: &StatusView) -> String {
    let mut out = format!("{} ({})\n", status.name, short(&status.id));
    if status.addrs.is_empty() {
        out += &format!("  Listening on port {}\n", status.port);
    } else {
        out += &format!("  Reachable at {}\n", status.addrs.join(", "));
    }
    if status.pairing {
        out += "  Pairing mode is open\n";
    }
    if status.paused {
        out += "  Sharing is paused; `clipsync resume` shares the clipboard again\n";
    }
    out.push('\n');
    out + &render_devices(&status.devices)
}

fn render_devices(devices: &[DeviceView]) -> String {
    if devices.is_empty() {
        return "No paired devices. Run `clipsync pair` to add one.\n".into();
    }
    let mut out = String::from("Paired devices:\n");
    for d in devices {
        let (mark, state) = if d.connected { ("●", "connected") } else { ("○", "offline") };
        out += &format!("  {mark} {} ({})  {state}\n", d.name, short(&d.id));
    }
    out
}

/// The QR code drawn with half blocks, light on dark: it reads correctly on a dark terminal.
fn render_qr(uri: &str) -> Result<String> {
    let code = QrCode::new(uri.as_bytes())?;
    Ok(code.render::<Dense1x2>().dark_color(Dense1x2::Light).light_color(Dense1x2::Dark).quiet_zone(true).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(pairing: bool, devices: Vec<DeviceView>) -> StatusView {
        StatusView {
            id: "86224755c0ff3b3b412a5da3ef12466cb12c48326e3454c02687f2cc88771027".into(),
            name: "alpha".into(),
            port: 47823,
            addrs: vec!["192.168.0.10:47823".into()],
            pairing,
            paused: false,
            devices,
        }
    }

    #[test]
    fn status_shows_the_device_its_addresses_and_its_peers() {
        let devices = vec![
            DeviceView { id: "bdc09de0aa".into(), name: "beta".into(), connected: true },
            DeviceView { id: "12345678ff".into(), name: "phone".into(), connected: false },
        ];
        let shown = render_status(&view(true, devices));
        assert_eq!(
            shown,
            "alpha (86224755)\n  Reachable at 192.168.0.10:47823\n  Pairing mode is open\n\nPaired devices:\n  \
             ● beta (bdc09de0)  connected\n  ○ phone (12345678)  offline\n"
        );
    }

    #[test]
    fn status_says_when_sharing_is_paused() {
        let status = StatusView { paused: true, ..view(false, vec![]) };
        assert!(
            render_status(&status).starts_with(
                "alpha (86224755)\n  Reachable at 192.168.0.10:47823\n  Sharing is paused; `clipsync resume` shares \
                 the clipboard again\n\n"
            ),
            "{}",
            render_status(&status)
        );
    }

    #[test]
    fn status_without_peers_suggests_pairing() {
        assert!(render_status(&view(false, vec![])).ends_with("No paired devices. Run `clipsync pair` to add one.\n"));
    }

    #[test]
    fn the_qr_code_is_drawn_in_half_blocks() {
        let qr = render_qr("clipsync://pair?v=1").unwrap();
        assert!(qr.lines().count() > 10);
        assert!(qr.chars().all(|c| matches!(c, '█' | '▀' | '▄' | ' ' | '\n')), "{qr}");
    }
}
