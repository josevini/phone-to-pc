//! The `clipsyncd` binary, each instance on its own headless compositor.

mod support;

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use support::sway::Sway;

/// Scratch XDG directories for one daemon.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new(name: &str) -> Home {
        let dir = tempfile::tempdir().unwrap();
        for sub in ["run", "config/clipsync"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        std::fs::set_permissions(dir.path().join("run"), std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let config = format!("name = \"{name}\"\nport = 0\n");
        std::fs::write(dir.path().join("config/clipsync/config.toml"), config).unwrap();
        Home { dir }
    }

    fn socket(&self) -> PathBuf {
        self.dir.path().join("run/clipsync.sock")
    }

    /// `program` with this home's XDG variables, talking to `sway`.
    fn command(&self, program: &str, sway: &Sway) -> Command {
        let mut cmd = sway.command(program);
        cmd.env("HOME", self.dir.path())
            .env("XDG_DATA_HOME", self.dir.path().join("data"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("RUST_LOG", "info,clipsyncd=debug");
        // Let coverage runs measure the binaries too.
        for var in ["LLVM_PROFILE_FILE", "CARGO_LLVM_COV_TARGET_DIR"] {
            if let Some(value) = std::env::var_os(var) {
                cmd.env(var, value);
            }
        }
        cmd
    }

    fn clipsync(&self, sway: &Sway, args: &[&str]) -> std::process::Output {
        self.command(env!("CARGO_BIN_EXE_clipsync"), sway).args(args).output().unwrap()
    }
}

struct Daemon {
    child: Child,
    log: PathBuf,
}

impl Daemon {
    fn start(home: &Home, sway: &Sway, args: &[&str]) -> Daemon {
        let log = home.dir.path().join(format!("daemon-{}.log", Instant::now().elapsed().as_nanos()));
        let child = home
            .command(env!("CARGO_BIN_EXE_clipsyncd"), sway)
            .args(args)
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        Daemon { child, log }
    }

    /// Starts the daemon and waits until its control socket answers.
    fn run(home: &Home, sway: &Sway) -> Daemon {
        let daemon = Daemon::start(home, sway, &[]);
        wait_until("the control socket", || {
            home.socket().exists() && home.clipsync(sway, &["status"]).status.success()
        });
        daemon
    }

    fn signal(&self, signal: &str) {
        assert!(Command::new("kill").args([signal, &self.child.id().to_string()]).status().unwrap().success());
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "clipsyncd did not exit; log:\n{}", self.log());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn pasted(sway: &Sway) -> String {
    String::from_utf8_lossy(&sway.paste().stdout).into_owned()
}

fn out(output: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn run_serves_the_cli_and_stops_cleanly_on_sigterm() {
    let sway = Sway::start();
    let home = Home::new("solo");
    let mut daemon = Daemon::run(&home, &sway);
    let status = home.clipsync(&sway, &[]);
    assert!(out(&status).contains("solo ("), "{}", out(&status));
    daemon.signal("-TERM");
    assert!(daemon.wait().success(), "{}", daemon.log());
    assert!(daemon.log().contains("clipsync daemon stopped"), "{}", daemon.log());
}

#[test]
fn run_exits_with_an_error_when_the_compositor_goes_away() {
    let mut sway = Sway::start();
    let home = Home::new("solo");
    let mut daemon = Daemon::run(&home, &sway);
    sway.stop();
    assert!(!daemon.wait().success());
    assert!(daemon.log().contains("clipboard backend stopped"), "{}", daemon.log());
}

#[test]
fn a_second_daemon_for_the_same_user_refuses_to_start() {
    let sway = Sway::start();
    let home = Home::new("solo");
    let _first = Daemon::run(&home, &sway);
    let mut second = Daemon::start(&home, &sway, &[]);
    assert!(!second.wait().success());
    assert!(second.log().contains("already running"), "{}", second.log());
}

#[test]
fn run_fails_clearly_without_a_compositor() {
    let sway = Sway::start();
    let home = Home::new("solo");
    let mut daemon = Daemon {
        child: home
            .command(env!("CARGO_BIN_EXE_clipsyncd"), &sway)
            .env("WAYLAND_DISPLAY", sway.dir().join("no-such-socket"))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
        log: PathBuf::new(),
    };
    let mut stderr = String::new();
    daemon.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert!(!daemon.wait().success());
    assert!(stderr.contains("cannot connect to the Wayland compositor"), "{stderr}");
}

#[test]
fn watch_logs_what_would_be_sent() {
    let sway = Sway::start();
    let home = Home::new("solo");
    let mut watch = home
        .command(env!("CARGO_BIN_EXE_clipsyncd"), &sway)
        .args(["watch", "--show-text"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = watch.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let expect = |needle: &str| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let line = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect(needle);
            if line.contains(needle) {
                return line;
            }
        }
    };
    expect("watching the clipboard");
    sway.copy(&[], b"watched text");
    assert!(expect("would send clip").contains("watched text"));
    sway.copy(&["--type", "image/png"], b"\x89PNG");
    expect("skipped");
    let _ = watch.kill();
    let _ = watch.wait();
}

#[test]
fn set_serves_its_text_until_another_client_copies() {
    let sway = Sway::start();
    let home = Home::new("solo");
    let mut set = Daemon::start(&home, &sway, &["set", "from clipsyncd set"]);
    wait_until("the text to be served", || pasted(&sway) == "from clipsyncd set");
    sway.copy(&[], b"someone else");
    assert!(set.wait().success(), "{}", set.log());

    let mut from_stdin =
        home.command(env!("CARGO_BIN_EXE_clipsyncd"), &sway).arg("set").stdin(Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(&mut from_stdin.stdin.take().unwrap(), b"piped").unwrap();
    wait_until("stdin text to be served", || pasted(&sway) == "piped");
    let _ = from_stdin.kill();
    let _ = from_stdin.wait();
}

#[test]
fn two_daemons_sync_their_wayland_clipboards_after_pairing_from_the_cli() {
    let (sway_a, sway_b) = (Sway::start(), Sway::start());
    let (home_a, home_b) = (Home::new("alpha"), Home::new("beta"));
    let _a = Daemon::run(&home_a, &sway_a);
    let _b = Daemon::run(&home_b, &sway_b);

    // b shows the invitation; a pairs with it.
    let mut acceptor =
        home_b.command(env!("CARGO_BIN_EXE_clipsync"), &sway_b).arg("pair").stdout(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(acceptor.stdout.take().unwrap()).lines();
    let uri = lines.by_ref().map_while(Result::ok).find(|l| l.starts_with("clipsync://")).expect("no pairing URI");
    let dialed = home_a.clipsync(&sway_a, &["pair", &uri]);
    assert!(dialed.status.success(), "{}", out(&dialed));
    let rest: Vec<String> = lines.map_while(Result::ok).collect();
    assert!(acceptor.wait().unwrap().success());
    assert!(rest.iter().any(|l| l.contains("Paired with alpha")), "{rest:?}");

    // A copy on a's desktop lands on b's clipboard, and back.
    sway_a.copy(&[], "copiado no alpha ✓".as_bytes());
    wait_until("b's clipboard", || pasted(&sway_b) == "copiado no alpha ✓");
    sway_b.copy(&[], b"copied on beta");
    wait_until("a's clipboard", || pasted(&sway_a) == "copied on beta");

    // No echo: both clipboards stay put once the copies have settled.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!((pasted(&sway_a), pasted(&sway_b)), ("copied on beta".into(), "copied on beta".into()));
}
