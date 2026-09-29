# Syncing the clipboard

Once devices are paired and connected, copying text on one puts it on the clipboard of
the others.

## What is synced

- Text up to 1 MiB, copied after the daemon started. What is already on the clipboard
  when the daemon starts stays local (see D10 in
  [Architecture decisions](../ARCHITECTURE_DECISIONS.md)); copying it again sends it.
- Not synced: content that password managers mark as secret, images and other non-text
  content, text over 1 MiB, invalid UTF-8, and the middle-click (primary) selection.
- Devices that are offline when you copy do not receive it later: nothing is queued.
- When two devices copy at almost the same time, all devices end up with the same one
  of the two texts (spec §6).

## Sending text from the terminal

`clipsync send TEXT` sends TEXT to the connected devices as if it had been copied here,
without changing this device's clipboard. Without TEXT it reads standard input, as is:

```
$ clipsync send "hello"
Sent to 1 device.
$ git rev-parse HEAD | clipsync send
Sent to 1 device.
```

It fails, with a message, when no paired device is connected, when the text is empty or
larger than 1 MiB, or when it is the text that was last sent or received.
