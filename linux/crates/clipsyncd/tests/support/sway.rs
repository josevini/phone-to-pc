// Each test crate that includes this module uses only some of it.
#![allow(dead_code)]

//! A headless Sway with its own runtime directory: a private compositor and clipboard per test.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

pub struct Sway {
    child: Child,
    dir: tempfile::TempDir,
    socket: PathBuf,
}

impl Sway {
    pub fn start() -> Sway {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let child = Command::new("sway")
            .args(["-c", "/dev/null"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", dir.path())
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("WLR_BACKENDS", "headless")
            // Software rendering: CI runners have no GPU.
            .env("WLR_RENDERER", "pixman")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the compositor tests need `sway` (see handbook/DEVELOPMENT.md)");
        let deadline = Instant::now() + Duration::from_secs(10);
        let socket = loop {
            let found = std::fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
            });
            if let Some(socket) = found {
                break socket;
            }
            assert!(Instant::now() < deadline, "sway did not create its Wayland socket");
            std::thread::sleep(Duration::from_millis(20));
        };
        Sway { child, dir, socket }
    }

    /// Absolute path of the compositor's socket, usable as `WAYLAND_DISPLAY`.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// A directory private to this compositor, for other test files.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// A command that talks to this compositor only.
    pub fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.dir.path())
            .env("WAYLAND_DISPLAY", &self.socket);
        cmd
    }

    /// Copies `data` as another client would (wl-copy stays behind serving it).
    pub fn copy(&self, args: &[&str], data: &[u8]) {
        let mut child = self.command("wl-copy").args(args).stdin(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(data).unwrap();
        assert!(child.wait().unwrap().success(), "wl-copy failed");
    }

    pub fn paste(&self) -> Output {
        self.command("wl-paste").arg("--no-newline").output().unwrap()
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Sway {
    fn drop(&mut self) {
        self.stop();
    }
}
