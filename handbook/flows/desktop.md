# The desktop app

`clipsync-desktop` shows and controls the clipsync daemon from the desktop: a tray icon and a small window. The
daemon does the syncing and keeps running without the app (see [Running the daemon](running.md)).

## Starting it

Run `clipsync-desktop`: it opens its window and adds its icon to the tray. With `--hidden` it starts in the tray
only, as when it is started with the session. Closing the window keeps it in the tray; **Quit** in the tray menu
closes it.

The tray icon appears in bars and desktops that show StatusNotifierItem icons: Waybar (Omarchy, Hyprland, Sway),
KDE Plasma, and GNOME with the AppIndicator extension.

## The tray

The icon is coloured while this device shares the clipboard with a connected device, and grey otherwise. Its menu:

- a first line saying what this device is doing: connected to how many devices, waiting for them, sharing paused, no
  paired devices yet, or the daemon not running;
- **Share the clipboard**: pauses and resumes sharing, as `clipsync pause` and `clipsync resume` do (see
  [Pausing](syncing.md#pausing));
- **Open clipsync**: shows the window;
- **Quit**.

## The window

The window shows this device (its name and the first characters of its ID), the **Share the clipboard** switch, and
the paired devices with whether each is connected. Both follow the daemon as it changes, such as a device connecting or
a pause made from the terminal.

When the daemon is not running, the window says so and shows the command that starts it; the app finds the daemon
once it starts.

## Pairing

Under the paired devices, two actions pair another device, as `clipsync pair` does (see [Pairing](pairing.md)):

- **Show a pairing code** shows this PC's QR code, with how long it still works (two minutes, for one pairing). Scan
  it with the clipsync app on a phone (**Scan a pairing code**). Under **Pair another PC instead** is the same code as
  a link, for another PC to paste. When a PC pairs by address instead (`clipsync pair <address>`), the window shows
  its name and the code to compare: **Same code, pair** pairs, **Different code** refuses.
- **Pair with a link** pairs with a PC that shows its code: paste the link shown under its code or printed by
  `clipsync pair`.

The window then says **Paired with *name***, or why pairing failed. When the code expires, **Show a new code** opens
pairing mode again. **Cancel**, or Esc, goes back without pairing and closes pairing mode; so does closing the window.
