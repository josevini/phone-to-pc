# The Android app

The app (Android 10 or newer) receives the clipboard of the PCs it is paired with: text
copied on a PC appears in the phone's clipboard, ready to paste. Sending from the phone is
not part of this version.

## First run

Opening the app starts a background service that stays connected to the paired devices,
shown by an ongoing notification ("Sharing the clipboard", with the number of connected
devices and a Stop action). The app asks:

- to show notifications (Android 13+), for that notification;
- to be exempt from battery optimisation, from a card on the main screen: without it,
  Android can cut the connections while the phone sleeps.

The phone gets its own identity on first run: a key in the Android Keystore that never
leaves it. Its name, shown to other devices, defaults to the phone's model; tap the
pencil next to it to change it.

## Pairing with a PC

1. On the PC, run `clipsync pair`; it shows a QR code for two minutes.
2. In the app, tap **Pair with a PC** and point the camera at the code. Without a
   camera, paste the `clipsync://pair?…` link printed under the code instead.
3. The app shows **Paired with *name***, and the PC prints `Paired with <phone name>`.

When pairing fails, the app says why and what to do: the code was already used or
expired (run `clipsync pair` again), the PC left pairing mode, or the PC could not be
reached (both devices must be on the same network, and the PC's firewall must allow TCP
port 47823). Back or Cancel leaves the screen without pairing.

## Syncing

Text copied on a paired PC is written to the phone's clipboard while the service runs,
in the foreground or not (see [Syncing the clipboard](syncing.md) for what is synced).

## Devices

The main screen lists the paired devices and whether each is connected. The phone
reconnects on its own: it finds paired devices with mDNS and also dials the addresses it
reached them at before, retrying from 1 s up to every 60 s. The PC also dials the phone
when it finds it with mDNS.

Tapping a device opens its page: its full device ID (the PC's `clipsync status` shows its
first eight characters) and **Unpair**, which asks for confirmation, tells the PC if it is
connected, and forgets it.
