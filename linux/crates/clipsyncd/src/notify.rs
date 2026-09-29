//! Desktop notifications (org.freedesktop.Notifications; mako on Omarchy).

use clipsync_core::engine::Event;
use tokio::sync::broadcast;
use tracing::debug;

use crate::daemon::{DaemonEvent, DaemonHandle};

/// Shows a notification for each event that deserves one, until the daemon stops.
pub fn spawn(daemon: &DaemonHandle) {
    let mut events = daemon.subscribe();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Some((title, body)) = notification_for(&event) {
                        // notify-rust blocks on D-Bus; keep it off the async workers.
                        tokio::task::spawn_blocking(move || {
                            let shown = notify_rust::Notification::new()
                                .appname("clipsync")
                                .summary(&title)
                                .body(&body)
                                .icon("edit-paste")
                                .show();
                            if let Err(e) = shown {
                                debug!("notification not shown: {e}");
                            }
                        });
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Title and body to show for `event`, if it deserves a notification.
pub(crate) fn notification_for(event: &DaemonEvent) -> Option<(String, String)> {
    let DaemonEvent::Engine(event) = event else { return None };
    match event {
        Event::Paired { device } => Some((format!("Paired with {}", device.name), "Clipboards are now shared.".into())),
        Event::Unpaired { peer } => {
            Some(("Device unpaired".into(), format!("{} no longer shares this clipboard.", peer.short())))
        }
        Event::PairingCode { name, code, .. } => {
            let digits = format!("{code:06}");
            Some((
                format!("{name} wants to pair"),
                format!("Code {} {} — confirm in the terminal running `clipsync pair`.", &digits[..3], &digits[3..]),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipsync_core::engine::{CloseReason, PairedDevice};
    use clipsync_core::{DeviceId, Hex};

    const PHONE: DeviceId = DeviceId(Hex([0xab; 32]));

    fn engine(event: Event) -> DaemonEvent {
        DaemonEvent::Engine(event)
    }

    #[test]
    fn pairing_and_unpairing_are_announced() {
        let paired = engine(Event::Paired { device: PairedDevice { id: PHONE, name: "phone".into() } });
        assert_eq!(notification_for(&paired), Some(("Paired with phone".into(), "Clipboards are now shared.".into())));
        let unpaired = engine(Event::Unpaired { peer: PHONE });
        assert_eq!(
            notification_for(&unpaired),
            Some(("Device unpaired".into(), "abababab no longer shares this clipboard.".into()))
        );
    }

    #[test]
    fn a_pairing_code_asks_to_confirm_in_the_terminal() {
        let code = engine(Event::PairingCode { conn: 1, peer: PHONE, name: "phone".into(), code: 37_725 });
        let (title, body) = notification_for(&code).unwrap();
        assert_eq!(title, "phone wants to pair");
        assert!(body.contains("037 725") && body.contains("terminal"), "{body}");
    }

    #[test]
    fn routine_events_stay_quiet() {
        let quiet = [
            engine(Event::PeerConnected { peer: PHONE, name: "phone".into() }),
            engine(Event::PeerDisconnected { peer: PHONE }),
            engine(Event::ClipReceived { from: PHONE, applied: true }),
            engine(Event::ConnectionClosed { conn: 1, peer: PHONE, reason: CloseReason::Timeout }),
            DaemonEvent::DialFailed { addrs: vec![], error: "unreachable".into() },
            DaemonEvent::PairingConnection { conn: 1 },
        ];
        for event in quiet {
            assert_eq!(notification_for(&event), None, "{event:?}");
        }
    }
}
