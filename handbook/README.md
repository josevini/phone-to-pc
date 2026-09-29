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
  protocol core, the daemon, the Wayland backend and its threads, the flow of
  one copy.
- [Architecture decisions](ARCHITECTURE_DECISIONS.md) — the technical
  decisions in force and why they were made, including those that shape parts
  not built yet.
- [Development](DEVELOPMENT.md) — toolchain, building and testing, trying the
  daemon on a real clipboard, and reproducing CI locally.

User-facing flows (pairing, syncing a copy, unpairing) get a page each under
`flows/` as they ship.
