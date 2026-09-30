# The desktop app

`clipsync-desktop` shows and controls the clipsync daemon from the desktop: a tray icon and a small window. The
daemon does the syncing and keeps running without the app (see [Running the daemon](running.md)).

## Installing it

Build it (see [Development](../DEVELOPMENT.md#desktop-app)), then install the program, its menu entry and its icon for
your user:

```sh
install -Dm755 desktop/src-tauri/target/release/clipsync-desktop ~/.local/bin/clipsync-desktop
install -Dm644 desktop/dist/clipsync-desktop.desktop ~/.local/share/applications/clipsync-desktop.desktop
install -Dm644 desktop/src-tauri/icons/icon.svg ~/.local/share/icons/hicolor/scalable/apps/clipsync.svg
```

## Starting it

Open **clipsync** from the applications menu, or run `clipsync-desktop`: it opens its window and adds its icon to the
tray. Starting it again while it runs shows its window instead of starting a second one. With `--hidden` it starts in
the tray only. Closing the window keeps it in the tray; **Quit** in the tray menu closes it.

**Start with the session**, under **This app** in the window, starts it in the tray at login: it adds an entry to
`~/.config/autostart` (`$XDG_CONFIG_HOME/autostart`) that runs it with `--hidden`, and turning it off removes the
entry. Desktops that run these entries include GNOME, KDE Plasma, and Hyprland or Sway sessions started with `uwsm`
(as Omarchy does).

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
the paired devices with whether each is connected. **Rename**, on this device's row, changes its name as
`clipsync rename` does (see [Renaming](devices.md#renaming)). Both follow the daemon as it changes, such as a device connecting or
a pause made from the terminal.

When the daemon is not running, the window says so and shows the command that starts it; the app finds the daemon
once it starts.

## A device's page

Clicking a paired device opens its page: its full device ID (the device shows its first characters under its own
name) and whether it is connected, and **Unpair**, which asks for confirmation, tells the device if it is connected,
and forgets it, as `clipsync unpair` does (see [Unpairing](devices.md#unpairing)). **Back**, or Esc, returns to the
list. When the device is unpaired from its side meanwhile, its page closes.

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
