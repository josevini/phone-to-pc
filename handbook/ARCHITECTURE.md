# Architecture

What the code does **today**. The reasoning behind choices lives in
[Architecture decisions](ARCHITECTURE_DECISIONS.md), and the wire protocol in [`spec/protocol.md`](../spec/protocol.md).

Current state: the Linux side can watch and set the clipboard and decide what it would send, but
there is no networking, pairing or Android app yet.

## Big picture

```
                spec/protocol.md
     (the contract clipsync-core implements)
                       │
                       ▼
   ┌───────────────────────────────────────┐
   │ linux/ (Rust)                         │
   │                                       │
   │ clipsyncd (daemon)                    │
   │   clipboard/wayland.rs ◄──────────────┼── Hyprland / Sway / KDE
   │   main.rs (watch, set)                │   (data-control)
   │        │ asks "what now?"             │
   │        ▼                              │
   │ clipsync-core (sans-IO logic)         │
   └───────────────────────────────────────┘
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
| `main.rs` | CLI. `watch` logs what would be sent to peers; `set` puts text on the clipboard and serves it |
| `clipboard/mod.rs` | Backend-neutral events: `Text`, `Skipped { reason }`, `OwnershipLost`, `Closed` |
| `clipboard/wayland.rs` | Wayland data-control backend |

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

## Flow of one copy

What happens today when you copy text while `clipsyncd watch` runs:

1. The compositor sends a new selection offer to the data-control device.
2. `wayland.rs` checks its MIME types (own? secret? text?), reads the text on a helper thread and emits
   `ClipboardEvent::Text`.
3. `main.rs` passes it to `ClipTracker::local_change`, which returns `Emit(clip)` with the next sequence number, or
   `Unchanged` if it is the same content as the last known clip (an echo or a re-copy).
4. `watch` logs "would send clip". Nothing is sent over the network yet.
