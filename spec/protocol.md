# clipsync protocol — version 1

Status: **draft** (changes allowed until the first release; bump `proto` after that).

This document is the source of truth for the clipsync protocol. It is
implemented by `clipsync-core` (Rust), which the Linux daemon and the Android
app share, and it is meant to allow other implementations. Behaviour that is
observable on the wire must be specified here and, when it is deterministic,
pinned by a known-answer test (§9). The key words MUST, SHOULD and MAY are used
as in RFC 2119.

## 1. Overview

Devices that are *paired* share text clipboard contents directly over the local
network. There is no server and no relaying: a device sends each clip it
originates to every paired device it is currently connected to.

```
 ┌────────┐  TLS 1.3 (mutual, pinned)  ┌────────┐
 │ laptop │◄──────────────────────────►│ phone  │
 └───┬────┘                            └───┬────┘
     │            ┌─────────┐              │
     └───────────►│ desktop │◄─────────────┘
                  └─────────┘
```

A device is identified only by its public key. Pairing is how two devices learn
and pin each other's keys.

## 2. Identity

- Each device generates one **EC P-256** key pair on first run and keeps it for
  its lifetime. On Android it SHOULD live in the Android Keystore.
- The device wraps the public key in a **self-signed X.509 certificate**. Subject,
  issuer, serial and validity are irrelevant and MUST NOT be checked by peers.
- **Device ID** = `SHA-256(SubjectPublicKeyInfo DER)` of that key: 32 bytes,
  written as 64 **lowercase** hex characters. Because it hashes the key and not
  the certificate, re-issuing the certificate does not change the identity.
- Receivers MUST accept uppercase hex in any hex field and normalise it.
- UIs SHOULD show the first 8 hex characters as a short ID.
- **Device name**: a human label chosen by the user, 1–64 bytes of UTF-8.

## 3. Discovery

Devices advertise a DNS-SD service on the LAN via mDNS:

| Field         | Value                                    |
|---------------|------------------------------------------|
| Service type  | `_clipsync._tcp.local.`                  |
| Instance name | the device name (mDNS may add a suffix)  |
| Port          | the TCP port the device listens on       |
| TXT `v`       | `1` (protocol version)                   |
| TXT `id`      | device ID (64 hex chars)                 |

The default port is **47823**; implementations MAY use any port, since it is
advertised. Networks with client isolation block multicast, so every
implementation MUST also allow connecting to a manually entered `host:port`.

## 4. Transport

- **TCP + TLS 1.3 only**, ALPN protocol ID `clipsync/1`.
- Both sides present their certificate (mutual TLS). The acceptor MUST request
  and require a client certificate.
- Certificate verification ignores CA chains, hostnames and validity periods.
  The handshake signatures MUST still be verified. The peer's identity is the
  device ID computed from its certificate's SPKI.
- A peer is trusted if its device ID is in the local paired-devices list. An
  unknown peer is only allowed to proceed with pairing (§7).

### 4.1 Framing

Each message is one frame:

```
+----------------------+---------------------------------+
| length: u32, big-end | payload: `length` bytes          |
+----------------------+---------------------------------+
```

- The payload is a UTF-8 JSON **object**.
- `length` MUST be ≥ 1 and ≤ **8 388 608** (8 MiB). A receiver MUST reject a
  larger length as soon as it reads the header, without buffering the body.
- Any framing or decoding error is fatal: send `error` (if possible) and close.

Decoding outcome categories (used in errors and tests):

| Outcome            | Meaning                                                      |
|--------------------|--------------------------------------------------------------|
| `empty_frame`      | length is 0                                                   |
| `frame_too_large`  | length > 8 MiB                                                |
| `invalid_json`     | payload is not valid UTF-8 JSON                               |
| `invalid_message`  | not an object, no string `type`, or a known type fails §5 rules |
| `ignored`          | valid JSON object with an unknown `type`: skip it (forward compat) |

Unknown **fields** in known messages MUST be ignored.

## 5. Messages

Every message has a string field `type`. Hex fields are sized in bytes:
`hex16` = 32 hex chars, `hex32` = 64 hex chars. Integers are unsigned and fit
in 53 bits (safe for JSON parsers that use doubles).

### 5.1 Session

| `type`   | Fields | Notes |
|----------|--------|-------|
| `hello`  | `proto`: int, `id`: hex32, `name`: string (1–64 B), `platform`: string, `caps`: string[], `seq`: int | First message on every connection, sent by both sides immediately after the TLS handshake. `platform` is informative (`linux`, `android`, …). v1 `caps` = `["text"]`. `seq` is the sender's Lamport counter (§6). |
| `ping`   | —      | Keepalive. |
| `pong`   | —      | Reply to `ping`. |
| `unpair` | —      | The sender has deleted the pairing. The receiver SHOULD delete it too. Both close. |
| `error`  | `code`: string, `message`?: string | Sent right before closing because of a failure. |

Error codes: `unsupported_version`, `identity_mismatch`, `not_paired`,
`pairing_closed`, `bad_token`, `protocol_error`.

### 5.2 Clipboard

| `type` | Fields | Notes |
|--------|--------|-------|
| `clip` | `id`: hex16, `origin`: hex32, `seq`: int ≥ 1, `ts`: int, `mime`: string, `text`: string, `sha256`: hex32 | One clipboard change. `id` is random. `origin` is the device ID that copied it. `ts` is Unix time in ms, informative only. v1 `mime` is `text/plain;charset=utf-8`. `text` is 1 B–1 MiB of UTF-8. `sha256` is the SHA-256 of the UTF-8 bytes of `text` and MUST match. |
| `ack`  | `id`: hex16, `applied`: bool | Reply to a `clip`. `applied` is true when the text is now the receiver's clipboard content. |

A receiver MUST ignore (`ack` with `applied: false`) a `clip` whose `mime` it
does not support; it is not a decoding error. The text limit is
**1 048 576 bytes** of UTF-8.

### 5.3 Pairing

| `type`         | Fields | Notes |
|----------------|--------|-------|
| `pair_request` | `method`: `"token"` or `"sas"`; `token`: hex16 (required for `token`); `commit`: hex32 (required for `sas`) | Sent by the dialer. |
| `pair_nonce`   | `nonce`: hex32 | SAS only, acceptor → dialer. |
| `pair_reveal`  | `nonce`: hex32 | SAS only, dialer → acceptor. |
| `pair_result`  | `ok`: bool, `reason`?: string | Outcome of the pairing, see §7. |

## 6. Clip ordering and echo suppression

Without ordering, two devices copying at nearly the same time end up with each
other's text. Every device keeps:

- `lamport`: u64 counter, starts at 0. It SHOULD be persisted across restarts.
- `current`: the `(seq, origin, sha256)` of what the device believes is in its
  clipboard, or none.

Orders are compared as `(seq, origin)`: numerically by `seq`, then by the raw
bytes of `origin` (equivalent to comparing the lowercase hex strings).

**On `hello` received:** `lamport = max(lamport, hello.seq)`.

**On a local clipboard change** with text `t`:

1. If `t` is empty → outcome `empty`, nothing is sent.
2. If `t` is longer than 1 MiB → outcome `too_large`, nothing is sent.
3. If `current` exists and `sha256(t) == current.sha256` → outcome `unchanged`.
   This suppresses the echo of a clip that was just applied from a peer.
4. Otherwise `lamport += 1`, `current = (lamport, self, sha256(t))`, and a
   `clip` with `seq = lamport` is sent to every connected paired peer →
   outcome `emit`.

**On a `clip` received:**

1. `lamport = max(lamport, clip.seq)`.
2. If `current` exists and `(clip.seq, clip.origin) <= (current.seq, current.origin)`
   → outcome `stale`, the clip is dropped (`ack applied: false`).
3. Otherwise `current = (clip.seq, clip.origin, clip.sha256)`. If the hash
   equals the previous `current.sha256` → outcome `same_content` (no write).
   Otherwise → outcome `apply`: write `text` to the local clipboard.
   Both reply `ack applied: true`.

A device MUST NOT forward clips whose `origin` is another device (no relaying in
v1), and a receiver MUST treat a clip whose `origin` is not the sender's device
ID as a protocol error. Clips are not queued for peers that are offline.

Clipboard backends SHOULD also recognise their own writes directly (e.g. by
offering a private MIME type) so that echo suppression does not rely only on
the hash check.

**Sensitive content** MUST NOT be sent: on Linux, a selection that offers the
`x-kde-passwordManagerHint` MIME type (set by password managers); on Android,
a clip with `ClipDescription.EXTRA_IS_SENSITIVE`.

## 7. Connection lifecycle

```
dialer                                        acceptor
  │── TCP + TLS 1.3 (mutual, ALPN clipsync/1) ──│
  │── hello ───────────────────────────────────►│
  │◄─────────────────────────────────── hello ──│
  │   (paired)   clip / ack / ping / pong / unpair
  │   (unpaired) pair_* messages, see §7.2 / §7.3
```

1. After the handshake each side sends `hello` without waiting.
2. If `hello.proto != 1` → `error unsupported_version`, close.
3. If `hello.id` differs from the device ID of the TLS certificate →
   `error identity_mismatch`, close.
4. If the peer is paired, the session is **established**. If it is not, only
   pairing messages are valid; an acceptor that is not in pairing mode sends
   `error not_paired` and closes.

### 7.1 Connection management

- Either side MAY dial. If two established connections exist for the same
  pair, keep the one whose TLS **client** has the smaller device ID and close
  the other without an `error`. Between two connections from the same client,
  keep the newer one.
- Dial triggers: the peer appears in mDNS, a manual address is configured, or
  the previous connection dropped. Retry with exponential backoff from 1 s up
  to 60 s.
- Send `ping` after 30 s without sending anything. Close the connection after
  90 s without receiving anything.

### 7.2 Pairing with a QR code (`token`)

Used when one device can scan a code shown by the other (phone ↔ anything).

1. The **acceptor** enters pairing mode for 120 s and creates a random 16-byte
   token. It shows a QR code containing a pairing URI (§8).
2. The **dialer** scans it and connects to one of the URI addresses. During
   the TLS handshake it MUST check that the acceptor's device ID equals the
   URI's `id`; otherwise it aborts.
3. After `hello`s, the dialer sends `pair_request{method:"token", token}`.
4. The acceptor compares the token in constant time. On success it stores the
   dialer (ID from the certificate, name from `hello`), invalidates the token
   and sends `pair_result{ok:true}`. The dialer then stores the acceptor.
   The connection continues as an established session.
5. On mismatch: `error bad_token` and close. After 3 failures the acceptor
   leaves pairing mode. When not in pairing mode: `error pairing_closed`.
6. Pairing mode ends after one successful pairing, by either method. When it
   ends, connections from unknown devices still waiting to send `pair_request`
   get `error pairing_closed` and are closed.

The QR code is the authenticated channel for the acceptor's key; the token
proves that the dialer saw the QR code. The acceptor MUST tell its user which
device was paired.

### 7.3 Pairing by comparing codes (`sas`)

Used when neither device can scan (PC ↔ PC). Both users open pairing mode; one
of them picks the other device from the discovered list and becomes the dialer.

1. Dialer: random 32-byte `nonce_d`; sends
   `pair_request{method:"sas", commit}` with
   `commit = SHA-256("clipsync-commit-v1" ‖ nonce_d)`.
2. Acceptor (must be in pairing mode, else `error pairing_closed`): random
   32-byte `nonce_a`; sends `pair_nonce{nonce: nonce_a}`.
3. Dialer sends `pair_reveal{nonce: nonce_d}`. The acceptor MUST check the
   commitment and send `error protocol_error` and close on mismatch.
4. Both compute the 6-digit code:

   ```
   h   = SHA-256("clipsync-sas-v1" ‖ id_d ‖ id_a ‖ nonce_d ‖ nonce_a)
   sas = u32_big_endian(h[0..4]) mod 1 000 000     // zero-padded to 6 digits
   ```

   where `id_d` / `id_a` are the raw 32-byte device IDs of dialer and acceptor
   and the ASCII labels have no terminator. UIs show it as `123 456`.
5. Each user checks that both screens show the same code and accepts or
   rejects. Each side sends `pair_result{ok}` with its user's decision.
6. A side completes pairing once it has **sent and received** `ok:true`; it
   stores the peer and the connection continues as an established session. Any
   `ok:false` aborts and closes.

The commitment stops a man in the middle from choosing its nonce after seeing
the dialer's, which would let it brute-force a matching code (10⁶ tries).

## 8. Pairing URI

```
clipsync://pair?v=1&id=<device id>&name=<name>&addr=<ip:port>[&addr=…]&token=<hex16>
```

- The query is `application/x-www-form-urlencoded` (`+` decodes to a space).
- `v`: MUST be `1`; another value → `unsupported_version`.
- `id`: device ID of the device showing the code (acceptor).
- `name`: its device name (1–64 bytes after decoding).
- `addr`: one or more socket addresses to try, as IP literals (IPv6 in
  brackets). Hostnames are not allowed.
- `token`: the pairing token.
- `v`, `id`, `name` and `token` MUST appear exactly once; `addr` at least once.
- Unknown parameters MUST be ignored. Any rule violation → `invalid_uri`.

## 9. Known-answer tests

Every deterministic rule of this document (device ID, clip hash, framing and
message validation, clip ordering, SAS, pairing URI) has known-answer tests in
`clipsync-core`: [`tests/spec.rs`](../linux/crates/clipsync-core/tests/spec.rs).
Their expected values were computed by an independent implementation of this
document and are kept as literals. A rule change updates this document and
those tests in the same change.

## 10. Out of scope for v1

Images and files, relaying/internet transport, clipboard history, clip queueing
for offline peers, and automatic background reading on Android.
