# Architecture

What the code does **today**. The reasoning behind choices lives in
[Architecture decisions](ARCHITECTURE_DECISIONS.md), and the wire protocol in [`spec/protocol.md`](../spec/protocol.md).

Current state: on Linux, the `clipsyncd` daemon syncs the clipboard with paired devices over mutual TLS and finds them
with mDNS; the `clipsync` CLI pairs devices (QR/URI or code comparison), shows their status, sends text, pauses
sharing and unpairs. The Android app pairs with a PC by scanning its QR code and writes the text the PC sends to the
phone's clipboard; it sends text chosen in the text-selection menu or the share sheet, and the clipboard's text from a
Quick Settings tile or the notification.

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
| `linux/crates/clipsync-ffi/` | UniFFI layer over `clipsync-core`, and the `uniffi-bindgen` that generates its Kotlin bindings |
| `linux/crates/clipsyncd/` | The daemon binary |
| `android/` | Gradle build of the Android side: `bindings` (generated Kotlin), `session` (the protocol host in plain Kotlin), `app` (the Android app) |
| `linux/deny.toml` | `cargo-deny` policy: advisories, permissive licences, crate sources |
| `linux/dist/clipsyncd.service` | systemd user unit |
| `handbook/` | This file, the architecture decisions, development setup, user flows |
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
| `engine.rs` | `Engine`: the protocol state of every connection (spec §5–§7) — hello checks, sessions, clips and acks, pausing, keepalive, the duplicate-connection rule, token and SAS pairing, unpairing |
| `clip.rs` | `ClipTracker`: Lamport ordering, echo suppression and pausing (spec §6), deciding emit / apply / ignore |
| `pairing.rs` | SAS commitment and 6-digit code (spec §7.3), pairing URI parse/format (spec §8) |
| `identity.rs` | `DeviceId` (SHA-256 of the public key's SPKI) and device-name rules |
| `discovery.rs` | Discovery and transport constants (service type, default port, TXT keys, ALPN) and how to read a browsed service (spec §3) |
| `hex.rs` | Fixed-size byte arrays carried as hex strings |

The host drives `Engine` with plain calls (`connection_opened`, `bytes_received`, `connection_closed`,
`local_clipboard_changed`, `set_paused`, `confirm_pairing`, `tick`) and passes the current time in; it carries out
the `Output`s the engine queues: bytes to send, connections to close, text to put on the clipboard, and events to show
or persist (peers connecting, pairing codes, devices paired or unpaired). The engine trusts nothing but the device ID
the host reads from the peer's TLS certificate. Its API uses plain data and enums, with no generics, lifetimes or
callbacks, so the Android app can call it through UniFFI unchanged.

`tests/spec.rs` holds known-answer tests for every deterministic rule of the spec, one test per area;
`tests/engine.rs` drives two engines against each other through the public API.

## `clipsync-ffi`: crossing to Kotlin

A thin layer that exports `clipsync-core` through UniFFI for the Android app. It decides nothing itself:

- Device IDs and tokens cross as hex strings, addresses as `SocketAddress { ip, port }` records; malformed input is a
  `CoreError`.
- `Engine` wraps the core's engine in a mutex, so Kotlin can call it from any thread; `poll_outputs` drains every
  queued output in one call.
- Free functions cover what the app needs outside a session: `parse_pair_uri`, `format_pair_uri` (which parses what
  it builds, so it never returns a URI other devices reject), `format_sas`, `device_id_from_spki`, `short_id`,
  `is_valid_name`, `txt_properties`, `peer_from_service`, `instance_name`, `pairing_window_ms` and the discovery and
  transport constants.
- `uniffi.toml` puts the bindings in the Kotlin package `io.github.josevini.clipsync.core`; the crate's
  `uniffi-bindgen` binary generates them from the built library.

## `android/`: the phone's side

A Gradle build with JDK-only modules, so everything but the Android platform code runs and is tested on the JVM.

| Module | Responsibility |
|--------|----------------|
| `bindings` | The Kotlin bindings, generated at build time from the host build of `clipsync-ffi` (`cargoBuildHost`, `generateBindings`); no hand-written code |
| `session` | The protocol host in plain Kotlin, the counterpart of the daemon's runtime: `Node`, `Tls`, `Identity`, `FileStateStore` |
| `app` | The Android app: the Keystore identity, NSD, the foreground service, the clipboard and the Compose UI around a `Node`; the native library, built by `cargoNdkBuild` |

`session`:

- **`Node`** owns the `Engine`, the saved state and the connections. Like the daemon's actor, one thread (the actor)
  makes every engine call and state change; socket threads hand their work to it. It carries out the engine's
  outputs (bytes to write, connections to close, text for the clipboard callback) and reports events to a listener.
- **Connections**: a server socket accepts, each dial tries the addresses in order (TCP connect 5 s, TLS handshake
  10 s), and a dial for a known device (a QR code's, or a saved address's) is dropped when another device answers.
  One thread reads each connection, and a per-connection writer thread writes in order and closes after the pending
  writes.
- **Reconnecting**: where a paired device was reached or found by discovery is a target, dialed while that device
  is not connected, with the daemon's backoff (1 s to 60 s, restarted by a disconnection). The targets of paired
  devices are saved with them, so a restarted node dials them again without discovery.
- **`Tls`**: TLS 1.3 through JSSE, mutual certificates, ALPN `clipsync/1`, any certificate accepted and the peer's
  device ID read from its public key. The key manager takes the private key as a handle, so an Android Keystore key
  is used without leaving the Keystore.
- **`FileStateStore`**: paired devices (with their addresses) and the Lamport counter in one JSON file, replaced
  atomically; a damaged file is an error.

`app`:

| File | Responsibility |
|------|----------------|
| `SyncService.kt` | Foreground service (`connectedDevice`): owns the `Node` and `Discovery`, writes received text to the clipboard on the main thread, keeps the notification's connected count and its Send clipboard and Stop actions |
| `KeystoreIdentity.kt` | The EC P-256 key in the Android Keystore (alias `clipsync-identity`) and the self-signed certificate the Keystore issues for it |
| `Discovery.kt` | `NsdManager`: advertises `_clipsync._tcp` with the core's instance name and TXT properties, and reports resolved services to the node through the core's `peer_from_service` (Android 14+ follows each service; older versions resolve one at a time) |
| `Sync.kt` | The running node as the UI sees it: a status `StateFlow` and an event `SharedFlow` |
| `SendActivity.kt` | "Send to devices" in the text-selection menu (`ACTION_PROCESS_TEXT`) and the share sheet (`ACTION_SEND` of `text/plain`): an activity with no window that hands the text to the node on a worker thread and reports the outcome in a toast |
| `ClipboardSendActivity.kt` | "Send clipboard": a transparent activity that reads the clipboard once its window has focus (Android lets only the focused app read it, D7), skips text marked `EXTRA_IS_SENSITIVE`, sends the rest like `SendActivity` and finishes. Started on a locked phone, it waits behind the lock screen for that focus, and sends nothing if it comes more than a minute after the tap |
| `ClipboardTileService.kt` | The "Send clipboard" Quick Settings tile: opens `ClipboardSendActivity`, and is lit while a paired device is connected |
| `SendOutcome.kt` | `send`: sends text through the running node, only while a paired device is connected, and maps the core's `LocalChange` to what the user is told; `reportInBackground` sends on a worker thread and shows the outcome in a toast |
| `Pairing.kt` | `PairingTracker`: follows one QR pairing through the node's events to success or a failure the user can act on |
| `PairingInvite.kt` | This phone's own pairing code: the Wi-Fi and Ethernet addresses to put in it (IPv4 first), its QR modules drawn with ZXing, and `InviteTracker`, which follows pairing mode to a pairing or its end |
| `QrDecoder.kt` | Reads a QR code from a camera frame's luminance plane with ZXing, dark on light or light on dark |
| `DeviceName.kt` | The device name: the user's choice, or the phone's model cut to 64 bytes |
| `ui/` | Compose screens: home (this device, battery optimisation, paired devices), scanning a pairing code (camera or pasted link), showing this phone's code, a device's page, about |

- The native library is `clipsync-ffi` built by `cargo-ndk` with the `android` Cargo profile, for `arm64-v8a` and
  `x86_64`, linked for 16 KB pages and packaged uncompressed. JNA, which the bindings call through, comes as its
  Android AAR.
- The Keystore key signs the TLS handshakes through the platform's JSSE provider (Conscrypt) without leaving the
  Keystore.
- Files: `state.json` (paired devices, their addresses, the Lamport counter) in the app's files directory; the device
  name in shared preferences. Neither is backed up or transferred to another device, since the key they belong to
  cannot be.
- The node listens on port 47823, or on any free port if that one is taken; discovery advertises the port in use.

## `clipsyncd`: doing

| Module | Responsibility |
|--------|----------------|
| `main.rs` | `clipsyncd` (or `clipsyncd run`) runs the daemon; `watch` and `set` exercise the clipboard backend alone |
| `daemon/mod.rs` | The runtime: the actor that owns the `Engine`, and `DaemonHandle` to drive it |
| `daemon/net.rs` | Accepting and dialing TLS connections, and moving bytes between sockets and the actor |
| `tls.rs` | TLS 1.3 client and server configurations; the peer's device ID from its certificate |
| `discovery.rs` | mDNS with `mdns-sd`: advertises `_clipsync._tcp` with the device ID, reports paired devices it finds to the daemon; the core's `discovery` module decides what a browsed service means |
| `ipc.rs` | The control socket: newline-delimited JSON requests and replies, server and client |
| `bin/clipsync.rs` | The `clipsync` CLI: status, devices, pairing, send, unpair |
| `notify.rs` | Desktop notifications for pairing, unpairing and pairing codes |
| `clipboard/mod.rs` | The `Clipboard` trait and backend-neutral events: `Text`, `Skipped { reason }`, `OwnershipLost`, `Closed` |
| `clipboard/wayland.rs` | Wayland data-control backend |
| `clipboard/memory.rs` | In-memory backend, used by the tests |
| `storage/` | The files kept between runs (see [Files](#files)) |

### Wayland backend

- Connects to the compositor named by `WAYLAND_DISPLAY`; `WaylandClipboard::spawn_on` takes an explicit socket path
  instead, which the tests use to run against a private headless Sway.
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
- **State**: paired devices are saved when paired, renamed or unpaired; the Lamport counter whenever it moves; the
  pause when it is turned on or off.
- The clipboard content already there when the daemon starts is not sent; later changes are (see D10 in
  [Architecture decisions](ARCHITECTURE_DECISIONS.md)).
- When the clipboard backend stops (the compositor went away), the daemon stops with an error, so its supervisor can
  restart it. `SIGINT` and `SIGTERM` stop it cleanly.
- It listens on every IPv6 and IPv4 address (`[::]`), or on IPv4 only where IPv6 is unavailable.

### Control socket

- `$XDG_RUNTIME_DIR/clipsync.sock`, mode `0600`. A daemon refuses to start while another one answers there, and
  replaces a socket file left by one that died.
- One JSON object per line. Requests carry `cmd` (`status`, `pair_start`, `pair_stop`, `pair_uri`, `pair_address`,
  `confirm`, `send`, `pause`, `resume`, `unpair`); replies carry `type`. Each request gets one reply.
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
| `$XDG_DATA_HOME/clipsync/state.json` | Paired devices, the Lamport counter and whether sharing is paused |
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
