//! The `clipsync` binary against running daemons.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;

use clipsyncd::clipboard::MemoryClipboard;
use clipsyncd::daemon::{self, DaemonConfig, DaemonHandle};
use clipsyncd::ipc;
use clipsyncd::storage::{Config, Dirs, Identity};

struct Node {
    handle: DaemonHandle,
    clipboard: MemoryClipboard,
    tmp: tempfile::TempDir,
}

impl Node {
    async fn start(name: &str) -> Node {
        let tmp = tempfile::tempdir().unwrap();
        let dirs =
            Dirs { data: tmp.path().join("data"), config: tmp.path().join("config"), runtime: tmp.path().into() };
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
        let handle = daemon::spawn(cfg, Arc::new(clipboard.clone()), events).await.unwrap();
        ipc::serve(&dirs.socket(), handle.clone()).await.unwrap();
        Node { handle, clipboard, tmp }
    }

    /// `clipsync` pointed at this node's socket through the XDG variables.
    fn cli(&self) -> Command {
        cli_in(self.tmp.path())
    }
}

fn cli_in(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_clipsync"));
    cmd.env("XDG_RUNTIME_DIR", dir).env("HOME", dir).env_remove("XDG_DATA_HOME").env_remove("XDG_CONFIG_HOME");
    cmd
}

async fn run(mut cmd: Command) -> Output {
    tokio::task::spawn_blocking(move || cmd.output().unwrap()).await.unwrap()
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// Pairs `a` with `b` using only the CLI: `b` shows the invitation, `a` accepts it.
async fn pair_with_cli(a: &Node, b: &Node) {
    let mut acceptor = b.cli();
    acceptor.arg("pair").stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = acceptor.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    // Read the acceptor's whole output: stopping early would break its pipe.
    let (uri_tx, uri_rx) = tokio::sync::oneshot::channel();
    let reader = tokio::task::spawn_blocking(move || {
        let mut uri_tx = Some(uri_tx);
        let mut all = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.starts_with("clipsync://")
                && let Some(tx) = uri_tx.take()
            {
                let _ = tx.send(line.clone());
            }
            all += &line;
            all.push('\n');
        }
        all
    });
    let uri = tokio::time::timeout(Duration::from_secs(5), uri_rx).await.expect("no pairing URI").unwrap();

    let mut dialer = a.cli();
    dialer.args(["pair", &uri]);
    let out = run(dialer).await;
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Paired with beta"), "{}", text(&out));

    let wait = tokio::task::spawn_blocking(move || child.wait().unwrap());
    let status = tokio::time::timeout(Duration::from_secs(5), wait).await.expect("acceptor did not exit").unwrap();
    let shown = tokio::time::timeout(Duration::from_secs(5), reader).await.expect("acceptor still running").unwrap();
    assert!(status.success(), "{shown}");
    assert!(shown.contains("Paired with alpha"), "{shown}");
}

#[tokio::test]
async fn status_without_paired_devices() {
    let a = Node::start("alpha").await;
    let out = run(a.cli()).await;
    assert!(out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("alpha"), "{shown}");
    assert!(shown.contains("No paired devices"), "{shown}");
}

#[tokio::test]
async fn pairing_sending_and_unpairing_from_the_command_line() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    pair_with_cli(&a, &b).await;

    let mut status = a.cli();
    status.arg("devices");
    let shown = text(&run(status).await);
    assert!(shown.contains("beta") && shown.contains("connected"), "{shown}");

    let mut send = a.cli();
    send.args(["send", "from the terminal"]);
    let out = run(send).await;
    assert!(out.status.success() && text(&out).contains("1 device"), "{}", text(&out));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while b.clipboard.contents().as_deref() != Some("from the terminal") {
        assert!(std::time::Instant::now() < deadline, "b never received the text");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let mut unpair = a.cli();
    unpair.args(["unpair", "beta"]);
    let out = run(unpair).await;
    assert!(out.status.success() && text(&out).contains("Unpaired beta"), "{}", text(&out));
    assert!(a.handle.status().await.devices.is_empty());
}

#[tokio::test]
async fn pausing_and_resuming_from_the_command_line() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    pair_with_cli(&a, &b).await;
    let cli = |args: &[&str]| {
        let mut cmd = a.cli();
        cmd.args(args);
        run(cmd)
    };

    let out = cli(&["pause"]).await;
    assert!(out.status.success() && text(&out).contains("Paused"), "{}", text(&out));
    let shown = text(&cli(&["status"]).await);
    assert!(shown.contains("Sharing is paused"), "{shown}");
    let out = cli(&["send", "while paused"]).await;
    assert!(!out.status.success() && text(&out).contains("paused"), "{}", text(&out));

    let out = cli(&["resume"]).await;
    assert!(out.status.success() && text(&out).contains("Resumed"), "{}", text(&out));
    assert!(!text(&cli(&["status"]).await).contains("paused"));
    let out = cli(&["send", "after resuming"]).await;
    assert!(out.status.success(), "{}", text(&out));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while b.clipboard.contents().as_deref() != Some("after resuming") {
        assert!(std::time::Instant::now() < deadline, "b never received the text");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn send_reads_standard_input_when_no_text_is_given() {
    let (a, b) = (Node::start("alpha").await, Node::start("beta").await);
    pair_with_cli(&a, &b).await;
    let mut send = a.cli();
    send.arg("send").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = tokio::task::spawn_blocking(move || {
        let mut child = send.spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b"piped text").unwrap();
        child.wait_with_output().unwrap()
    })
    .await
    .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while b.clipboard.contents().as_deref() != Some("piped text") {
        assert!(std::time::Instant::now() < deadline, "b never received the text");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn errors_exit_with_a_failure_and_a_message() {
    let a = Node::start("alpha").await;
    let mut unpair = a.cli();
    unpair.args(["unpair", "nobody"]);
    let out = run(unpair).await;
    assert!(!out.status.success());
    assert!(text(&out).contains("no paired device matches"), "{}", text(&out));

    let empty = tempfile::tempdir().unwrap();
    let out = run(cli_in(empty.path())).await;
    assert!(!out.status.success());
    assert!(text(&out).contains("is it running?"), "{}", text(&out));
}
