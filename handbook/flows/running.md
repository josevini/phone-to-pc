# Running the daemon

`clipsyncd` must run in your Wayland session: it watches the session's clipboard.

## At login, with systemd

`linux/dist/clipsyncd.service` is a user unit tied to `graphical-session.target`: it
starts with the graphical session, stops with it, and restarts after a failure (for
example when the compositor restarts).

```sh
cp linux/dist/clipsyncd.service ~/.config/systemd/user/
systemctl --user enable --now clipsyncd
journalctl --user -u clipsyncd -f        # its log
```

The unit runs `/usr/bin/clipsyncd`, where packages install it. After
`cargo install --path linux/crates/clipsyncd`, point it at `~/.cargo/bin` instead:

```sh
systemctl --user edit clipsyncd
# [Service]
# ExecStart=
# ExecStart=%h/.cargo/bin/clipsyncd
```

## By hand

Run `clipsyncd` in a terminal of the session. Ctrl-C stops it. Only one daemon runs per
user session: a second one refuses to start while the first answers on the control
socket.

## Notifications

The daemon shows a desktop notification when a device is paired or unpaired, and when a
device asks to pair by comparing codes (answer in the terminal running `clipsync pair`).
