//! What the tray shows for a status, apart from drawing it. Tray icons on Linux have no tooltip: the summary is the
//! first line of the menu.

use clipsyncd::ipc::StatusView;
use serde::Serialize;

/// The tray's state, which the window shows too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrayView {
    /// One line on what this device is doing.
    pub summary: String,
    /// Whether "Share the clipboard" is on; `None` while there is no daemon to ask.
    pub sharing: Option<bool>,
    /// Whether the icon is drawn dimmed: nothing is being shared right now.
    pub dimmed: bool,
}

impl TrayView {
    /// The view of `status`, or of an unreachable daemon.
    pub fn of(status: Option<&StatusView>) -> TrayView {
        let Some(status) = status else {
            return TrayView { summary: "The clipsync daemon is not running".into(), sharing: None, dimmed: true };
        };
        let connected = status.devices.iter().filter(|d| d.connected).count();
        let summary = match connected {
            _ if status.devices.is_empty() => "No paired devices yet".into(),
            _ if status.paused => "Sharing paused".into(),
            0 => "Waiting for paired devices".into(),
            1 => "Connected to 1 device".into(),
            n => format!("Connected to {n} devices"),
        };
        TrayView { summary, sharing: Some(!status.paused), dimmed: status.paused || connected == 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipsyncd::ipc::DeviceView;

    fn status(paused: bool, connected: &[bool]) -> StatusView {
        let devices = connected
            .iter()
            .enumerate()
            .map(|(i, &connected)| DeviceView { id: format!("{i:064}"), name: format!("device {i}"), connected })
            .collect();
        StatusView {
            id: "ab".repeat(32),
            name: "book2".into(),
            port: 47823,
            addrs: vec![],
            pairing: false,
            paused,
            devices,
        }
    }

    fn summary(status: Option<&StatusView>) -> String {
        TrayView::of(status).summary
    }

    #[test]
    fn the_summary_says_what_this_device_is_doing() {
        assert_eq!(summary(None), "The clipsync daemon is not running");
        assert_eq!(summary(Some(&status(false, &[]))), "No paired devices yet");
        assert_eq!(summary(Some(&status(false, &[false]))), "Waiting for paired devices");
        assert_eq!(summary(Some(&status(false, &[true, false]))), "Connected to 1 device");
        assert_eq!(summary(Some(&status(false, &[true, true]))), "Connected to 2 devices");
        assert_eq!(summary(Some(&status(true, &[true]))), "Sharing paused");
    }

    #[test]
    fn sharing_can_be_toggled_only_while_the_daemon_runs() {
        assert_eq!(TrayView::of(None).sharing, None);
        assert_eq!(TrayView::of(Some(&status(false, &[]))).sharing, Some(true));
        assert_eq!(TrayView::of(Some(&status(true, &[]))).sharing, Some(false));
    }

    #[test]
    fn the_icon_is_dimmed_unless_something_is_shared() {
        assert!(TrayView::of(None).dimmed);
        assert!(TrayView::of(Some(&status(false, &[false]))).dimmed);
        assert!(TrayView::of(Some(&status(true, &[true]))).dimmed);
        assert!(!TrayView::of(Some(&status(false, &[true]))).dimmed);
    }
}
