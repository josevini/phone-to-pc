# The Android app

The app (Android 10 or newer) receives the clipboard of the PCs it is paired with: text
copied on a PC appears in the phone's clipboard, ready to paste. Sending goes the other
way when you ask, from the text-selection menu, the share sheet or **Send clipboard** (see
[Sending text](#sending-text)): Android lets only the app in the foreground read the
clipboard, so the phone does not send what is copied on it by itself.

## First run

Opening the app starts a background service that stays connected to the paired devices,
shown by an ongoing notification ("Sharing the clipboard", with the number of connected
devices, and the Send clipboard, Pause and Stop actions). The app asks:

- to show notifications (Android 13+), for that notification;
- to be exempt from battery optimisation, from a card on the main screen: without it,
  Android can cut the connections while the phone sleeps.

The phone gets its own identity on first run: a key in the Android Keystore that never
leaves it. Its name, shown to other devices, defaults to the phone's model; tap it
under **This device** to change it.

## Pairing with a PC

1. On the PC, run `clipsync pair`; it shows a QR code for two minutes.
2. In the app, tap **Scan a pairing code** and point the camera at the code. Without a
   camera, paste the `clipsync://pair?…` link printed under the code instead.
3. The app shows **Paired with *name***, and the PC prints `Paired with <phone name>`.

When pairing fails, the app says why and what to do: the code was already used or
expired (show a new one), the other device left pairing mode, or it could not be
reached (both devices must be on the same network, and a PC's firewall must allow TCP
port 47823). Back or Cancel leaves the screen without pairing.

## Pairing two phones

One phone shows a code and the other scans it:

1. On one phone, tap **Show a pairing code**. It shows a QR code with how long it still
   works (two minutes, for one pairing) and keeps the screen on meanwhile.
2. On the other, tap **Scan a pairing code** and point the camera at it.
3. Both show **Paired with *name***.

The code holds the phone's addresses on Wi-Fi (or Ethernet), not on mobile data, so
both phones must be on the same network; without Wi-Fi the app asks to connect to it.
When the code expires, **Show a new code** opens pairing mode again. Back or Cancel
closes pairing mode. While the code is shown, a PC that asks to pair by comparing codes
(`clipsync pair <address>`) is declined: scan the code instead.

## Syncing

Text copied on a paired PC is written to the phone's clipboard while the service runs,
in the foreground or not (see [Syncing the clipboard](syncing.md) for what is synced).

## Pausing

**Share the clipboard**, under **This device** on the main screen, pauses and resumes
sharing without unpairing, as the notification's **Pause** and **Resume** actions do. While
paused, the phone stays connected to the paired devices, but what they copy is not written
to its clipboard and it sends nothing. The notification then says "Clipboard sharing paused"
and offers only Resume and Stop, and the Send clipboard tile is not lit. The pause lasts
until you resume, also across restarts of the app.

**Stop** is different: it closes the connections and the background service until the app
is opened again.

## Sending text

Three ways send text to the paired devices that are connected, which put it on their
clipboard; the phone's own clipboard is left as it is:

- Select text in any app and choose **Send to devices** in the menu that pops up (it may
  be under ⋮).
- Tap **Share** in any app that shares text (a page's link, a note, a place) and choose
  **Send to devices**. When the app shares only a subject, such as a title, the subject is
  sent. Images and files are not offered this target.
- Copy text as usual, then tap **Send clipboard**: in the notification (expand it to see
  its actions) or as a Quick Settings tile, added by editing the Quick Settings panel. The
  tile is lit while sharing with a connected paired device. Android lets only the app on
  screen read the clipboard, so clipsync opens an invisible window for an instant to read
  it, and Android may say that clipsync pasted from the clipboard. On a locked phone the tile asks
  to unlock first; if the phone is not unlocked within a minute, nothing is sent.

Once a device is paired, the main screen shows this as a tip under the device list. A
short message says what happened:

- **Sent to N devices.**
- Not sent, because no paired device is connected. Nothing is queued: send it again once
  one is.
- Not sent, because it is the text last sent or received.
- Not sent, because it is empty or larger than 1 MiB.
- Not sent, because the app it was copied from marked it as sensitive, as password
  managers do with passwords.
- Not sent, because sharing is paused (see [Pausing](#pausing)). Send clipboard does not
  read the clipboard then.
- Not sent, because sharing is stopped (the notification's Stop action); open the app to
  start it again.

## Devices

The main screen lists the paired devices and whether each is connected. The phone
reconnects on its own: it finds paired devices with mDNS and also dials the addresses it
reached them at before, retrying from 1 s up to every 60 s. The PC also dials the phone
when it finds it with mDNS.

Tapping a device opens its page: its full device ID (the PC's `clipsync status` shows its
first eight characters) and **Unpair**, which asks for confirmation, tells the PC if it is
connected, and forgets it.
