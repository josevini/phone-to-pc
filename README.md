# clipsync

Open-source clipboard sharing between Linux PCs and Android phones, in any
combination: PC ↔ phone, PC ↔ PC, phone ↔ phone. Copy on one device and paste
on the others, like Samsung's cross-device clipboard but for any Linux and any
Android.

- **No server**: paired devices talk directly over the local network.
- **Private**: mutual TLS with keys pinned at pairing time; password-manager
  secrets are never sent.
- **Focused**: only the clipboard, with native Wayland support.

> **Status: early development.** The protocol spec and the Linux clipboard
> backend exist; networking, pairing and the Android app do not yet.

## Repository layout

| Path | Contents |
|------|----------|
| [`spec/`](spec/) | Protocol spec (source of truth) |
| [`linux/`](linux/) | Rust workspace: `clipsync-core` (sans-IO protocol) and `clipsyncd` (daemon) |
| [`handbook/`](handbook/README.md) | Architecture, architecture decisions, development |

## Trying the current state

You need a Wayland compositor with data-control (Hyprland, Sway, KDE Plasma) and
a recent Rust toolchain.

```sh
cd linux
cargo test                                    # core, including the spec's known-answer tests
cargo run -p clipsyncd -- watch --show-text   # log clipboard changes as the daemon sees them
cargo run -p clipsyncd -- set "hello"         # own the clipboard until another app copies
```

These act on your real clipboard. See [Development](handbook/DEVELOPMENT.md) for manual testing, coverage
and the other checks CI runs.

## License

[MIT](LICENSE)
