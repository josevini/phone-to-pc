//! clipsync daemon library: clipboard backends, storage, transport and the daemon runtime.
//! The `clipsyncd` and `clipsync` binaries are thin wrappers around it.

pub mod clipboard;
pub mod daemon;
pub mod discovery;
pub mod storage;
pub mod tls;
