# Architecture

What the code does **today**. The reasoning behind choices lives in
[Architecture decisions](ARCHITECTURE_DECISIONS.md), and the wire protocol in [`spec/protocol.md`](../spec/protocol.md).

Current state: on Linux, the `clipsyncd` daemon syncs the clipboard with paired devices over mutual TLS and finds them
with mDNS; the `clipsync` CLI pairs devices (QR/URI or code comparison), shows their status, sends text and unpairs.
There is no Android app yet.

## Big picture

```
                spec/protocol.md
     (the contract clipsync-core implements)
                       │
                       ▼
   ┌───────────────────────────────────────────────────┐
   │ clipsyncd                                          │
   │                                                    │
   │  clipboard/wayland.rs ──events──►┌──────────────┐  │      TLS 1.3
   │  (Hyprland, Sway, KDE)◄─set_text─│ daemon actor │◄─┼──► paired devices
   │                                  │  owns Engine │  │   (daemon/net.rs,
   │  storage/ ◄── paired devices, ───│              │  │    tls.rs)
   │   lamport, identity              └──────┬───────┘  │
   │                                         │ asks     │
   │                                         ▼          │
   │                         clipsync-core (sans-IO)    │
   └───────────────────────────────────────────────────┘
```

## Repository layout

| Path | Contents |
|------|----------|
| `spec/protocol.md` | Protocol v1 draft: identity, discovery, transport, messages, ordering, pairing |
| `linux/crates/clipsync-core/` | Protocol logic with no I/O |
| `linux/crates/clipsyncd/` | The daemon binary |
| `linux/deny.toml` | `cargo-deny` policy: advisories, permissive licences, crate sources |
| `handbook/` | This file, the architecture decisions, development setup |
| `CLAUDE.md`, `AGENTS.md` | Working rules for contributors and coding agents |
| `.github/workflows/ci.yml` | CI (see `CLAUDE.md`) |

## `clipsync-core`: deciding

Pure functions and small state machines: bytes and events go in, values come out. Nothing touches sockets, clocks
(callers pass timestamps in), threads or the clipboard. That keeps it fully testable as plain functions and lets
the Android app share it through UniFFI, doing its own I/O around it (see
[D2](ARCHITECTURE_DECISIONS.md#d2--one-protocol-core-in-rust-shared-with-android-through-uniffi)).

| Module | Responsibility |
|--------|----------------|
| `message.rs` | Message types (`hello`, `clip`, `ack`, `ping`, `pair_*`, …) and the validation rules of spec §5 |
| `frame.rs` | Length-prefixed JSON framing; `FrameDecoder` takes bytes as they arrive and yields messages |
| `engine.rs` | `Engine`: the protocol state of every connection (spec §5–§7) — hello checks, sessions, clips and acks, keepalive, the duplicate-connection rule, token and SAS pairing, unpairing |
| `clip.rs` | `ClipTracker`: Lamport ordering and echo suppression (spec §6), deciding emit / apply / ignore |
| `pairing.rs` | SAS commitment and 6-digit code (spec §7.3), pairing URI parse/format (spec §8) |
| `identity.rs` | `DeviceId` (SHA-256 of the public key's SPKI) and device-name rules |
| `hex.rs` | Fixed-size byte arrays carried as hex strings |

The host drives `Engine` with plain calls (`connection_opened`, `bytes_received`, `connection_closed`,
`local_clipboard_changed`, `confirm_pairing`, `tick`) and passes the current time in; it carries out the `Output`s
the engine queues: bytes to send, connections to close, text to put on the clipboard, and events to show or persist
(peers connecting, pairing codes, devices paired or unpaired). The engine trusts nothing but the device ID the host
reads from the peer's TLS certificate. Its API uses plain data and enums, with no generics, lifetimes or callbacks,
so the Android app can call it through UniFFI unchanged.

`tests/spec.rs` holds known-answer tests for every deterministic rule of the spec, one test per area;
`tests/engine.rs` drives two engines against each other through the public API.

## `clipsyncd`: doing

| Module | Responsibility |
|--------|----------------|
| `main.rs` | `clipsyncd` (or `clipsyncd run`) runs the daemon; `watch` and `set` exercise the clipboard backend alone |
| `daemon/mod.rs` | The runtime: the actor that owns the `Engine`, and `DaemonHandle` to drive it |
| `daemon/net.rs` | Accepting and dialing TLS connections, and moving bytes between sockets and the actor |
| `tls.rs` | TLS 1.3 client and server configurations; the peer's device ID from its certificate |
| `discovery.rs` | mDNS: advertises `_clipsync._tcp` with the device ID, reports paired devices it finds to the daemon |
| `ipc.rs` | The control socket: newline-delimited JSON requests and replies, server and client |
| `bin/clipsync.rs` | The `clipsync` CLI: status, devices, pairing, send, unpair |
| `clipboard/mod.rs` | The `Clipboard` trait and backend-neutral events: `Text`, `Skipped { reason }`, `OwnershipLost`, `Closed` |
| `clipboard/wayland.rs` | Wayland data-control backend |
| `clipboard/memory.rs` | In-memory backend, used by the tests |
| `storage/` | The files kept between runs (see [Files](#files)) |

### Wayland backend

- Binds `ext_data_control_manager_v1`, or `zwlr_data_control_manager_v1` if the compositor lacks it. Both protocols
  are identical apart from their names, so one implementation serves both (`Proto<Ext, Wlr>` plus a dispatch macro).
- Threads:
  - one **event-loop thread** (calloop) owns the Wayland connection and receives commands (`SetText`, `Stop`) over a
    channel;
  - each **offer read** runs on a short-lived thread, so a slow or stuck source client cannot block the loop; a
    generation counter drops results that a newer clipboard change has overtaken;
  - each **paste we serve** is written from a short-lived thread.
- Recognising our own writes: every source we create also offers a per-process MIME type
  `application/x-clipsync-source-<random>`. A selection carrying it is reported as `Skipped { Own }` without being
  read; reading it would deadlock, because this process is the one serving it.
- Skipped without syncing: cleared clipboard, password-manager secrets (`x-kde-passwordManagerHint`), non-text
  content, text over 1 MiB, invalid UTF-8. The primary selection (middle click) is ignored.
- Text is read as `text/plain;charset=utf-8`, `UTF8_STRING` or `text/plain`, in that order of preference, and
  offered under the same types `wl-copy` uses.

### Runtime

- **One actor** (a tokio task) owns the `Engine`, the persisted `State` and the connections' writers. Everything else
  sends it `Cmd`s: connections (opened, bytes, closed), dial failures, clipboard events, and control requests through
  `DaemonHandle` (status, pairing, sending text, unpairing, shutdown). It carries out the engine's outputs and
  re-broadcasts its events to subscribers.
- **Connections**: the accept loop and each dial run the TLS handshake (10 s limit; TCP connect 5 s), then hand the
  actor the peer's device ID from its certificate. A dial that reaches a device other than the one expected is dropped.
  Per connection, one task reads and forwards bytes; another writes what the actor queues. The actor closes a
  connection by dropping its writer, which also stops the reader.
- **Discovery**: the daemon advertises `_clipsync._tcp` (instance `name (short id)`, TXT `v=1` and `id`) with
  `mdns-sd`, and browses for other devices. A paired device found there is dialed at every address it advertises,
  except IPv6 link-local ones, which need an interface scope; unpaired devices are ignored. Without mDNS (no
  multicast), the daemon keeps working with configured peers only.
- **Reconnecting**: every configured peer address, and every address where a paired device was found, is dialed until
  its device is connected. Failed dials back off from
  1 s to 60 s; a disconnection restarts the schedule at 1 s. The actor drives the engine's timers once a second.
- **State**: paired devices are saved when paired, renamed or unpaired; the Lamport counter whenever it moves.
- The clipboard content already there when the daemon starts is not sent; later changes are (see D10 in
  [Architecture decisions](ARCHITECTURE_DECISIONS.md)).
- When the clipboard backend stops (the compositor went away), the daemon stops with an error, so its supervisor can
  restart it. `SIGINT` and `SIGTERM` stop it cleanly.
- It listens on every IPv6 and IPv4 address (`[::]`), or on IPv4 only where IPv6 is unavailable.

### Control socket

- `$XDG_RUNTIME_DIR/clipsync.sock`, mode `0600`. A daemon refuses to start while another one answers there, and
  replaces a socket file left by one that died.
- One JSON object per line. Requests carry `cmd` (`status`, `pair_start`, `pair_stop`, `pair_uri`, `pair_address`,
  `confirm`, `send`, `unpair`); replies carry `type`. Each request gets one reply.
- The pairing requests keep the connection streaming the pairing's progress (`pairing_code`, `paired`,
  `pairing_failed`, `pairing_ended`); the client answers a `pairing_code` with `confirm` on the same connection. For a
  pairing this device dialed, only the events of that connection are streamed. Closing the connection that opened
  pairing mode closes it.
- `unpair` accepts a device name, full ID or unique ID prefix (4+ characters).

### Files

| File | Contents |
|------|----------|
| `$XDG_DATA_HOME/clipsync/identity.key` | The device's EC P-256 private key (PKCS#8 PEM). The device ID is derived from it, so it is never regenerated: a damaged key is an error |
| `$XDG_DATA_HOME/clipsync/identity.crt` | Self-signed certificate for that key; reissued for the same key when missing or not matching it |
| `$XDG_DATA_HOME/clipsync/state.json` | Paired devices and the Lamport counter |
| `$XDG_CONFIG_HOME/clipsync/config.toml` | Optional settings: `name` (defaults to the hostname), `port` (47823; 0 lets the system choose), `peers` (addresses to dial besides the ones found with mDNS) |

Directories are created `0700` and files written `0600`, atomically (temporary file and rename). Without
`XDG_DATA_HOME` or `XDG_CONFIG_HOME`, the defaults under `$HOME` apply; `XDG_RUNTIME_DIR` is required.

## Flow of one copy

What happens when you copy text on a device paired with another one:

1. The compositor sends a new selection offer to the data-control device.
2. `wayland.rs` checks its MIME types (own? secret? text?), reads the text on a helper thread and emits
   `ClipboardEvent::Text`.
3. The actor passes it to `Engine::local_clipboard_changed`, which gives it the next sequence number and queues a
   `clip` frame for every connected paired device, or does nothing when it is the same content as the last known clip
   (an echo or a re-copy).
4. The actor writes the frame to each connection; TLS carries it to the peer.
5. On the peer, the engine checks the clip's ordering (spec §6) and queues `SetClipboard`; the actor hands the text to
   its backend, which serves it and ignores the change it caused. The peer acknowledges with `ack`.
