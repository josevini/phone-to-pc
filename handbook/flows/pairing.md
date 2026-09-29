# Pairing

Two devices sync only after they are paired: each one stores the other's device ID
(the hash of its public key) and refuses everything else. There are two ways to pair.

## With a QR code or its URI

For a phone that can scan, or a PC where you can paste the URI.

1. On the device to pair **with**, run `clipsync pair`. It opens pairing mode for two
   minutes and shows a QR code with the pairing URI printed under it:

   ```
   $ clipsync pair
   Scan this code on the other device, or run there: clipsync pair '<the URI below>'
   ██▀▀▀▀▀██ …
   clipsync://pair?v=1&id=86224755…&name=book2&addr=192.168.0.10:47823&token=…
   Waiting for a device… (pairing mode closes in 2 minutes; Ctrl-C to stop)
   ```

2. On the other device, scan the code or run `clipsync pair '<URI>'`. On a phone, tap
   **Pair with a PC** in the app (see [The Android app](android.md)).
3. Both sides print `Paired with <name> (<short id>).` and start syncing.

The URI carries this device's ID, so the other device checks during the TLS handshake
that it reached the right one; the one-time token proves the other device saw the code.
A wrong token is refused (`the QR code is no longer valid`), and three wrong tokens
close pairing mode.

## By comparing codes

For two PCs, when neither can scan.

1. On one device, run `clipsync pair` to open pairing mode, and note an address from
   `clipsync status` (`Reachable at …`).
2. On the other, run `clipsync pair <ip:port>`.
3. Both show the same six-digit code and ask whether it matches:

   ```
   book2 (86224755) wants to pair.
   Code: 037 725
   Does the other device show the same code? [y/N]
   ```

4. Answer `y` on both. Pairing completes only when both accept; a `n` on either side
   stops it on both (`the code was rejected on the other device`).

The code is derived from both device IDs and from random values each side committed to
before seeing the other's (spec §7.3), so a device in the middle cannot make the codes
match.

## Pairing mode

- It closes after two minutes, after one successful pairing, or when the `clipsync pair`
  that opened it exits (Ctrl-C).
- While it is closed, unknown devices are refused (`the other device is not in pairing
  mode`).
