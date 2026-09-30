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
the paired devices with whether each is connected. Without paired devices, it shows the command that pairs a phone.
Both follow the daemon as it changes, such as a device connecting or a pause made from the terminal.

When the daemon is not running, the window says so and shows the command that starts it; the app finds the daemon
once it starts.
