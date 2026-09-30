# Handbook

A documented mirror of clipsync as it exists today. Everything here describes
shipped behaviour; the wire protocol is specified in
[`spec/protocol.md`](../spec/protocol.md).

The handbook is updated in the same change as the behaviour it describes.
When it contradicts the code, don't assume either side is right: investigate
which one is wrong. The handbook may be out of date, or it may be describing
the behaviour that was intended, and the code is the bug.

## Contents

- [Architecture](ARCHITECTURE.md) — how the code is put together: the
  protocol core, the daemon, the Wayland backend and its threads, the desktop
  app, the flow of one copy.
- [Architecture decisions](ARCHITECTURE_DECISIONS.md) — the technical
  decisions in force and why they were made, including those that shape parts
  not built yet.
- [Development](DEVELOPMENT.md) — toolchain, building and testing, trying the
  daemon on a real clipboard, and reproducing CI locally.

## Flows

- [Pairing](flows/pairing.md) — with a QR code or its URI, or by comparing codes.
- [Syncing the clipboard](flows/syncing.md) — what is synced, and sending text from
  the terminal.
- [Devices](flows/devices.md) — status, connecting, unpairing.
- [Running the daemon](flows/running.md) — at login with systemd, by hand, and its
  notifications.
- [The Android app](flows/android.md) — first run, pairing with a PC's QR code, receiving
  the clipboard, sending text, devices.
- [The desktop app](flows/desktop.md) — the tray icon and window over the daemon on Linux.

New user-facing flows get a page here as they ship.
