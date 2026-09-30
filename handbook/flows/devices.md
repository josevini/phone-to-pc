# Devices

## Status

`clipsync` (or `clipsync status`) shows this device, where other devices can reach it,
whether pairing mode is open, whether sharing is paused (see
[Pausing](syncing.md#pausing)), and the paired devices:

```
$ clipsync
book2 (86224755)
  Reachable at 192.168.0.10:47823

Paired devices:
  ● desktop (bdc09de0)  connected
  ○ phone (1234abcd)  offline
```

`clipsync devices` shows only the list.

## Connecting

The daemon connects to paired devices on its own: it finds them with mDNS on the local
network and also dials the addresses listed as `peers` in `config.toml`, retrying from
1 s up to every 60 s while a device is away.

## Renaming

`clipsync rename NAME` changes the name other devices see (1 to 64 bytes). It is saved as `name` in
`config.toml`, keeping the rest of the file as it is, and takes effect without restarting the daemon: the daemon
advertises the new name, and the connected devices reconnect to learn it. Without a `name` there, the daemon uses the
machine's hostname.

```
$ clipsync rename "Meu PC"
Renamed to Meu PC. Connected devices learn the new name as they reconnect.
```

## Unpairing

`clipsync unpair DEVICE` forgets a paired device, named by its name, its ID or an ID
prefix of at least four characters. If the device is connected, it is told and forgets
this one too. If it is offline, it keeps its record and its connection attempts are
refused; unpair it there as well.
