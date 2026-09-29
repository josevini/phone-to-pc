# Development

Building, testing and trying clipsync locally. For how the code is organised,
see [Architecture](ARCHITECTURE.md).

## Requirements

- Rust stable through `rustup` (the workspace needs 1.88 or newer).
- To run the daemon: a Wayland compositor with data-control (Hyprland, Sway,
  KDE Plasma).
- To run the tests: `sway` and `wl-clipboard` (`wl-copy`, `wl-paste`). The
  Wayland tests start a private headless Sway per test, with its own runtime
  directory and clipboard, so they never touch your desktop session.
- Tools CI also runs, needed only to reproduce its checks locally:
  - `cargo-llvm-cov`, plus `rustup component add llvm-tools-preview`;
  - `cargo-deny`;
  - `diff-cover`, a Python tool (`pipx run diff-cover` needs no install).

## Build and test

From `linux/`:

```sh
cargo build
cargo test                                   # every suite below
cargo clippy --all-targets -- -D warnings
cargo fmt                                    # rustfmt.toml: max width 120
cargo llvm-cov --workspace                   # coverage table; --open for the HTML report
cargo deny check                             # advisories, licences, sources (deny.toml)
```

One test is ignored by default because it needs mDNS multicast on the local network, which CI runners may not
allow. Run it with `cargo test -p clipsyncd --test daemon -- --ignored`.

The suites:

| Suite | What it runs |
|-------|--------------|
| `clipsync-core` unit tests, `tests/spec.rs`, `tests/engine.rs` | The protocol core: known-answer tests for the spec, and two engines driven against each other |
| `clipsyncd` unit tests | Storage, TLS (including impersonation), discovery parsing, the control protocol, notifications |
| `tests/daemon.rs`, `tests/ipc.rs`, `tests/cli.rs` | Real daemons over loopback TLS with in-memory clipboards, driven through their handle, the control socket and the `clipsync` binary |
| `tests/wayland.rs` | The Wayland backend against headless Sway |
| `tests/binary.rs` | The `clipsyncd` binary on headless Sway, including two daemons on two compositors paired from the CLI |

Diff coverage, as CI computes it:

```sh
cargo llvm-cov --workspace --cobertura --output-path coverage.xml   # ignored by git
pipx run diff-cover coverage.xml --compare-branch=origin/main --fail-under=100 --exclude '*/clipsyncd/*'
```

Diff coverage applies to `clipsync-core` only; `clipsyncd`'s coverage is
measured but not held to a threshold, which does not exempt it from tests.

## Running the daemon

`cargo run -p clipsyncd` runs the daemon on your real clipboard, with your real identity and state under
`~/.local/share/clipsync`. To try it without touching those, point it at scratch directories, and give each extra
daemon its own port:

```sh
# The compositor's socket lives in the real runtime dir: pin it before moving XDG_RUNTIME_DIR.
export WAYLAND_DISPLAY="$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY"
export XDG_DATA_HOME=/tmp/cs-a/data XDG_CONFIG_HOME=/tmp/cs-a/config XDG_RUNTIME_DIR=/tmp/cs-a/run
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME/clipsync" && chmod 700 "$XDG_RUNTIME_DIR"
echo 'port = 47900' > "$XDG_CONFIG_HOME/clipsync/config.toml"
cargo run -p clipsyncd &
cargo run -p clipsyncd --bin clipsync -- status   # the CLI finds the daemon through the same variables
```

Two daemons on the same desktop share one clipboard: a copy reaches the other daemon both through the compositor and
over the network. That exercises echo suppression, but it is not how two machines behave.

## Trying the clipboard backend

The debug commands act on the **real clipboard** of the running session.
Use a separate compositor session with a disposable clipboard when one is
available. Otherwise, announce that the commands affect the desktop session's
clipboard and preserve its contents before testing.

`wl-paste -n` saves one representation of the selection. It does not preserve
all offered MIME types or metadata, and copying that data back cannot guarantee
a complete restoration. For rich content, multiple representations or content
that cannot be backed up, use an isolated session.

For a text selection, run this, then run the manual tests only if the backup
succeeds. `--type text` accepts any text representation the selection offers,
and `$XDG_RUNTIME_DIR` is private to your user and cleared at logout:

```sh
clipboard_backup=$(mktemp "${XDG_RUNTIME_DIR:?}/clipboard-backup.XXXXXX")
if wl-paste --no-newline --type text > "$clipboard_backup"; then
  printf 'Text backed up; keep this shell open for restoration.\n'
else
  rm -f -- "$clipboard_backup"
  unset clipboard_backup
  printf 'Backup failed; use an isolated clipboard session for testing.\n' >&2
fi
```

After testing, restore the text from the same shell. Delete the backup only
after restoration succeeds:

```sh
if [ -n "${clipboard_backup:-}" ]; then
  if wl-copy --type text/plain < "$clipboard_backup"; then
    rm -f -- "$clipboard_backup"
    unset clipboard_backup
  fi
fi
```

From `linux/`:

```sh
cargo run -p clipsyncd -- watch --show-text   # log clipboard changes and what would be sent
cargo run -p clipsyncd -- set "hello"         # own the clipboard until another app copies
RUST_LOG=debug cargo run -p clipsyncd -- watch  # also log the MIME types each app offers
```

With `watch` running, these exercise each outcome:

| Command | Expected log |
|---------|--------------|
| `wl-copy "olá 👋"` | `would send clip` |
| the same `wl-copy` again | `not sent outcome=Unchanged` |
| `printf '\x89PNG' \| wl-copy --type image/png` | `skipped reason=NotText` |
| `head -c 1100000 /dev/zero \| tr '\0' a \| wl-copy` | `skipped reason=TooLarge` |
| `printf 'ab\xffcd' \| wl-copy --type text/plain` | `skipped reason=InvalidUtf8` |
| `clipsyncd set …`, then copy anything else | `set` exits with `another app took the clipboard` |
