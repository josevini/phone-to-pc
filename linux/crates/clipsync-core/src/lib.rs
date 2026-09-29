//! Sans-IO core of the clipsync protocol (see `spec/protocol.md`).
//!
//! Nothing here touches sockets, clocks or the clipboard: callers feed bytes and
//! events in and act on the values that come out. That keeps the logic testable
//! as plain functions and lets every host (the Linux daemon, the Android app)
//! do its own I/O around it.

pub mod clip;
pub mod engine;
pub mod frame;
pub mod hex;
pub mod identity;
pub mod message;
pub mod pairing;

pub use clip::{ClipTracker, LocalOutcome, RemoteOutcome};
pub use frame::{Decoded, FrameDecoder, FrameError};
pub use hex::{Hex, Hex16, Hex32};
pub use identity::DeviceId;
pub use message::Message;
pub use pairing::PairUri;
