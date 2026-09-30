//! Real daemons for the tests, each with an in-memory clipboard and its control socket in a scratch directory.
//!
//! Shared by the test suites; each uses only part of it.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clipsyncd::clipboard::MemoryClipboard;
use clipsyncd::daemon::{self, DaemonConfig, DaemonHandle};
use clipsyncd::ipc;
use clipsyncd::storage::{Config, Dirs, Identity};

pub struct Daemon {
    pub handle: DaemonHandle,
    pub socket: PathBuf,
}

impl Daemon {
    /// A daemon serving its control socket in `dir`.
    pub async fn start(dir: &Path, name: &str) -> Daemon {
        let dirs = Dirs { data: dir.join("data"), config: dir.join("config"), runtime: dir.into() };
        let identity = Identity::load_or_create(&dirs.data).unwrap();
        let (clipboard, events) = MemoryClipboard::new();
        let cfg = DaemonConfig {
            dirs: dirs.clone(),
            config: Config { name: name.into(), port: 0, peers: vec![] },
            identity,
            platform: "linux".into(),
            listen: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
        };
        let handle = daemon::spawn(cfg, Arc::new(clipboard), events).await.unwrap();
        ipc::serve(&dirs.socket(), handle.clone()).await.unwrap();
        Daemon { handle, socket: dirs.socket() }
    }

    pub async fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.handle.status().await.port))
    }
}
