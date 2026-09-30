//! Pairing from the app, through the daemon: showing this device's code, or pairing with another's link. Each
//! pairing has its own connection to the control socket, as the daemon streams a pairing's progress on the
//! connection that started it.

use std::path::Path;

use anyhow::{Context, Result, bail};
use clipsyncd::ipc::{Client, Reply, Request};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::qr::Qr;

/// What happens during a pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PairingEvent {
    /// A device asks to pair by comparing codes: show `code` and [`Pairing::confirm`] the user's answer.
    Code {
        name: String,
        code: String,
    },
    Paired {
        name: String,
    },
    Failed {
        reason: String,
    },
    /// Pairing mode closed before a device paired: the code expired.
    Ended,
}

/// This device's pairing code, to show while pairing mode is open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Invite {
    pub uri: String,
    pub expires_in_s: u64,
    pub qr: Qr,
}

/// A pairing in progress. Dropping it stops following it; for a code shown here, that closes pairing mode.
pub struct Pairing {
    answers: mpsc::UnboundedSender<bool>,
    task: JoinHandle<()>,
}

impl Pairing {
    /// Opens pairing mode on the daemon at `socket` and returns the code to show. `on_event` then gets what happens,
    /// up to a device pairing or pairing mode ending.
    pub async fn show_code(
        socket: &Path,
        on_event: impl FnMut(PairingEvent) + Send + 'static,
    ) -> Result<(Invite, Pairing)> {
        let mut client = Client::connect(socket).await?;
        let (uri, expires_in_s) = match client.request(&Request::PairStart).await? {
            Reply::PairingStarted { uri, expires_in_s } => (uri, expires_in_s),
            other => bail!(unexpected(other)),
        };
        let qr = Qr::of(&uri).context("the pairing link does not fit in a QR code")?;
        Ok((Invite { uri, expires_in_s, qr }, Pairing::follow(client, on_event)))
    }

    /// Pairs with the device whose pairing link is `uri`. `on_event` then gets the outcome.
    pub async fn with_link(
        socket: &Path,
        uri: &str,
        on_event: impl FnMut(PairingEvent) + Send + 'static,
    ) -> Result<Pairing> {
        let mut client = Client::connect(socket).await?;
        match client.request(&Request::PairUri { uri: uri.trim().into() }).await? {
            Reply::Ok => Ok(Pairing::follow(client, on_event)),
            other => bail!(unexpected(other)),
        }
    }

    /// The user's answer to the last [`PairingEvent::Code`].
    pub fn confirm(&self, accept: bool) {
        let _ = self.answers.send(accept);
    }

    fn follow(mut client: Client, mut on_event: impl FnMut(PairingEvent) + Send + 'static) -> Pairing {
        let (answers, mut answered) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut asking = None;
            loop {
                tokio::select! {
                    reply = client.next() => {
                        let event = match reply {
                            Ok(Some(Reply::PairingCode { conn, name, code, .. })) => {
                                asking = Some(conn);
                                PairingEvent::Code { name, code: format!("{} {}", &code[..3], &code[3..]) }
                            }
                            Ok(Some(Reply::Paired { name, .. })) => PairingEvent::Paired { name },
                            Ok(Some(Reply::PairingFailed { reason })) => PairingEvent::Failed { reason },
                            Ok(Some(Reply::PairingEnded)) => PairingEvent::Ended,
                            Ok(Some(Reply::Error { message })) => PairingEvent::Failed { reason: message },
                            Ok(Some(_)) => continue,
                            Ok(None) | Err(_) => PairingEvent::Failed { reason: "the clipsync daemon stopped".into() },
                        };
                        let done = !matches!(event, PairingEvent::Code { .. });
                        on_event(event);
                        if done {
                            return;
                        }
                    }
                    Some(accept) = answered.recv() => {
                        if let Some(conn) = asking.take() {
                            let _ = client.send(&Request::Confirm { conn, accept }).await;
                        }
                    }
                }
            }
        });
        Pairing { answers, task }
    }
}

impl Drop for Pairing {
    fn drop(&mut self) {
        // Ends the task, which closes its connection: the daemon closes the pairing mode that connection opened.
        self.task.abort();
    }
}

fn unexpected(reply: Reply) -> String {
    match reply {
        Reply::Error { message } => message,
        other => format!("unexpected reply from the daemon: {other:?}"),
    }
}
