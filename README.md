# clipsync

Open-source clipboard sharing between Linux PCs and Android phones, in any
combination: PC ↔ phone, PC ↔ PC, phone ↔ phone. Copy on one device and paste
on the others, like Samsung's cross-device clipboard but for any Linux and any
Android.

- **No server**: paired devices talk directly over the local network.
- **Private**: mutual TLS with keys pinned at pairing time; password-manager
  secrets are never sent.
- **Focused**: only the clipboard, with native Wayland support.

> **Status: early development.** Linux devices pair and sync their clipboards; the
> Android app pairs with them and receives their clipboard, but does not send yet.

## Repository layout

| Path | Contents |
|------|----------|
| [`spec/`](spec/) | Protocol spec (source of truth) |
| [`android/`](android/) | Android app (Kotlin) around the shared core |
| [`linux/`](linux/) | Rust workspace: `clipsync-core` (sans-IO protocol), `clipsync-ffi` (its Kotlin bindings) and `clipsyncd` (daemon) |
| [`handbook/`](handbook/README.md) | Architecture, architecture decisions, development |

## Trying it

You need a Wayland compositor with data-control (Hyprland, Sway, KDE Plasma) and a
recent Rust toolchain.

```sh
cd linux
cargo install --path crates/clipsyncd   # installs clipsyncd and clipsync
clipsyncd &                             # the daemon, on your real clipboard
clipsync pair                           # on one PC: shows a QR code and its URI
clipsync pair 'clipsync://pair?…'       # on the other PC: pairs with it
```

For the Android app, build and install it from `android/` (see
[Development](handbook/DEVELOPMENT.md#android)), then tap **Pair with a PC** and scan
the code `clipsync pair` shows.

See the [handbook](handbook/README.md) for the flows, and
[Development](handbook/DEVELOPMENT.md) for building, testing and trying it safely.

## License

[MIT](LICENSE)
