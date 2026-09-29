//! In-memory clipboard for tests and for running without a desktop session.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use super::{Clipboard, ClipboardEvent};

/// A clipboard that lives in memory. `copy` plays the part of the user copying something.
#[derive(Clone)]
pub struct MemoryClipboard {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    contents: Option<String>,
    events: mpsc::UnboundedSender<ClipboardEvent>,
}

impl MemoryClipboard {
    /// The clipboard and the stream of changes a daemon consumes.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<ClipboardEvent>) {
        let (events, rx) = mpsc::unbounded_channel();
        (MemoryClipboard { inner: Arc::new(Mutex::new(Inner { contents: None, events })) }, rx)
    }

    /// The user copied `text`.
    pub fn copy(&self, text: &str) {
        self.change(text, false);
    }

    /// `text` was already on the clipboard when the daemon started.
    pub fn preload(&self, text: &str) {
        self.change(text, true);
    }

    /// The backend stopped, as when the compositor goes away.
    pub fn close(&self, reason: &str) {
        let _ = self.inner.lock().unwrap().events.send(ClipboardEvent::Closed(reason.to_owned()));
    }

    pub fn contents(&self) -> Option<String> {
        self.inner.lock().unwrap().contents.clone()
    }

    fn change(&self, text: &str, initial: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.contents = Some(text.to_owned());
        let _ = inner.events.send(ClipboardEvent::Text { text: text.to_owned(), initial });
    }
}

impl Clipboard for MemoryClipboard {
    fn set_text(&self, text: String) {
        // Like the Wayland backend recognising its own write: no event.
        self.inner.lock().unwrap().contents = Some(text);
    }
}
