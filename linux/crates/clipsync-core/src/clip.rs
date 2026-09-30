//! Clip ordering and echo suppression (spec §6).

use sha2::{Digest, Sha256};

use crate::hex::{Hex, Hex16, Hex32};
use crate::identity::DeviceId;
use crate::message::Clip;

/// Largest clip text, in bytes of UTF-8.
pub const MAX_TEXT_LEN: usize = 1024 * 1024;

/// The only MIME type v1 sends.
pub const TEXT_MIME: &str = "text/plain;charset=utf-8";

pub fn text_digest(text: &str) -> Hex32 {
    Hex(Sha256::digest(text.as_bytes()).into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalOutcome {
    Empty,
    TooLarge,
    /// Sharing is paused: nothing is sent.
    Paused,
    /// Same content as the known clipboard: an echo or a re-copy. Nothing to send.
    Unchanged,
    /// Send this clip to every connected paired peer.
    Emit(Clip),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOutcome {
    /// Older than what we have; drop it.
    Stale,
    /// Newer, but the clipboard already holds this text; don't rewrite it.
    SameContent,
    /// Write the clip's text to the local clipboard.
    Apply,
    /// Sharing is paused; drop it.
    Paused,
}

impl RemoteOutcome {
    /// Value of `ack.applied` for this outcome.
    pub fn applied(self) -> bool {
        !matches!(self, RemoteOutcome::Stale | RemoteOutcome::Paused)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Current {
    seq: u64,
    origin: DeviceId,
    sha256: Hex32,
}

/// Per-device ordering state. Holds no I/O; the caller owns the clipboard and sockets.
#[derive(Debug, Clone)]
pub struct ClipTracker {
    self_id: DeviceId,
    lamport: u64,
    current: Option<Current>,
    paused: bool,
}

impl ClipTracker {
    /// `lamport` is the persisted counter from the previous run, or 0.
    pub fn new(self_id: DeviceId, lamport: u64) -> Self {
        Self { self_id, lamport, current: None, paused: false }
    }

    /// Current Lamport counter; persist it and send it in `hello.seq`.
    pub fn lamport(&self) -> u64 {
        self.lamport
    }

    pub fn observe_hello(&mut self, seq: u64) {
        self.lamport = self.lamport.max(seq);
    }

    /// Pauses or resumes sharing: while paused, nothing is sent or applied.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The local clipboard changed to `text`. `ts_ms` is the current Unix time in ms.
    pub fn local_change(&mut self, text: String, ts_ms: u64) -> LocalOutcome {
        if self.paused {
            // The clipboard now holds text the other devices never saw.
            self.current = None;
            return LocalOutcome::Paused;
        }
        if text.is_empty() {
            return LocalOutcome::Empty;
        }
        if text.len() > MAX_TEXT_LEN {
            return LocalOutcome::TooLarge;
        }
        let sha256 = text_digest(&text);
        if self.current.is_some_and(|c| c.sha256 == sha256) {
            return LocalOutcome::Unchanged;
        }
        self.lamport += 1;
        self.current = Some(Current { seq: self.lamport, origin: self.self_id, sha256 });
        LocalOutcome::Emit(Clip {
            id: Hex16::random(),
            origin: self.self_id,
            seq: self.lamport,
            ts: ts_ms,
            mime: TEXT_MIME.to_owned(),
            text,
            sha256,
        })
    }

    /// A validated clip arrived from a paired peer.
    pub fn receive(&mut self, clip: &Clip) -> RemoteOutcome {
        self.lamport = self.lamport.max(clip.seq);
        if self.paused {
            return RemoteOutcome::Paused;
        }
        let prev = self.current;
        if prev.is_some_and(|c| (clip.seq, clip.origin) <= (c.seq, c.origin)) {
            return RemoteOutcome::Stale;
        }
        self.current = Some(Current { seq: clip.seq, origin: clip.origin, sha256: clip.sha256 });
        if prev.is_some_and(|c| c.sha256 == clip.sha256) { RemoteOutcome::SameContent } else { RemoteOutcome::Apply }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stale_and_paused_clips_are_acked_as_not_applied() {
        assert!(RemoteOutcome::Apply.applied());
        assert!(RemoteOutcome::SameContent.applied());
        assert!(!RemoteOutcome::Stale.applied());
        assert!(!RemoteOutcome::Paused.applied());
    }
}
