//! System clipboard access. v1 only has the Wayland data-control backend.

mod wayland;

pub use wayland::WaylandClipboard;

/// MIME type password managers add to secrets they put on the clipboard.
pub const PASSWORD_MANAGER_HINT: &str = "x-kde-passwordManagerHint";

/// Why a clipboard change will not be synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The clipboard was cleared.
    Empty,
    /// We set it ourselves (a clip applied from a peer).
    Own,
    /// A password manager marked it as secret (spec §6).
    Sensitive,
    /// No text representation is offered (an image, files, …).
    NotText,
    TooLarge,
    InvalidUtf8,
    ReadFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardEvent {
    /// Another client put text on the clipboard. `initial` marks the content
    /// that was already there when the backend started.
    Text { text: String, initial: bool },
    /// The clipboard changed, but there is nothing to sync.
    Skipped { reason: SkipReason, initial: bool },
    /// Text we set was replaced by another client.
    OwnershipLost,
    /// The backend stopped (compositor gone, seat removed, …).
    Closed(String),
}
