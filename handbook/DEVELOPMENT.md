# Development

Building, testing and trying clipsync locally. For how the code is organised,
see [Architecture](ARCHITECTURE.md).

## Requirements

- Rust stable through `rustup` (`linux/` needs 1.88 or newer, `desktop/` 1.90).
- To run the daemon: a Wayland compositor with data-control (Hyprland, Sway,
  KDE Plasma).
- To run the tests: `sway` and `wl-clipboard` (`wl-copy`, `wl-paste`). The
  Wayland tests start a private headless Sway per test, with its own runtime
  directory and clipboard, so they never touch your desktop session.
- For `android/`: JDK 21, the Android SDK with platform 37 and NDK 29.0.14206865, the Rust targets
  `aarch64-linux-android` and `x86_64-linux-android`, and `cargo-ndk` (see [Android](#android)).
- For `desktop/`: Node.js 24 with npm, and the libraries Tauri builds against: WebKitGTK 4.1, GTK 3,
  libayatana-appindicator and librsvg (Arch: `webkit2gtk-4.1 libayatana-appindicator librsvg`; Debian and Ubuntu:
  `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`). See [Desktop app](#desktop-app).
- Tools CI also runs, needed only to reproduce its checks locally:
  - `cargo-llvm-cov`, plus `rustup component add llvm-tools-preview`;
  - `cargo-deny`;
  - `diff-cover`, a Python tool (`pipx run diff-cover` needs no install).

## Build and test

From `linux/`:

```sh
cargo build
cargo test                                   # every suite below
cargo clippy --all-targets -- -D warnings
cargo fmt                                    # rustfmt.toml: max width 120
cargo llvm-cov --workspace                   # coverage table; --open for the HTML report
cargo deny check                             # advisories, licences, sources (deny.toml)
```

One test is ignored by default because it needs mDNS multicast on the local network, which CI runners may not
allow. Run it with `cargo test -p clipsyncd --test daemon -- --ignored`.

The suites:

| Suite | What it runs |
|-------|--------------|
| `clipsync-core` unit tests, `tests/spec.rs`, `tests/engine.rs` | The protocol core: known-answer tests for the spec, and two engines driven against each other |
| `clipsync-ffi` unit tests, `tests/engine.rs`, `tests/helpers.rs`, `tests/bindgen.rs` | The UniFFI layer: every type crossing the boundary, two engines driven through the exported API, and Kotlin bindings generated from the built library |
| `clipsyncd` unit tests | Storage, TLS (including impersonation), the control protocol, notifications |
| `tests/daemon.rs`, `tests/ipc.rs`, `tests/cli.rs` | Real daemons over loopback TLS with in-memory clipboards, driven through their handle, the control socket and the `clipsync` binary |
| `tests/wayland.rs` | The Wayland backend against headless Sway |
| `tests/binary.rs` | The `clipsyncd` binary on headless Sway, including two daemons on two compositors paired from the CLI |

Diff coverage, as CI computes it:

```sh
cargo llvm-cov --workspace --cobertura --output-path coverage.xml   # ignored by git
pipx run diff-cover coverage.xml --compare-branch=origin/main --fail-under=100 --exclude '*/clipsyncd/*'
```

Diff coverage applies to `clipsync-core` and `clipsync-ffi`; `clipsyncd`'s coverage is
measured but not held to a threshold, which does not exempt it from tests.

## Android

`android/` is a Gradle build; run it with JDK 21 (`JAVA_HOME`) and the Android SDK in `ANDROID_HOME` or
`android/local.properties` (`sdk.dir=…`, ignored by git). One-time setup, with the SDK's `sdkmanager`:

```sh
sdkmanager "platforms;android-37.0" "build-tools;36.0.0" "platform-tools" "ndk;29.0.14206865"
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked
```

From `android/`:

```sh
./gradlew :session:test               # JVM tests, including the interop test against clipsyncd
./gradlew :app:testDebugUnitTest      # the app's JVM tests
./gradlew lintKotlin :app:lintDebug   # ktlint (formatKotlin fixes what it can) and Android Lint
./gradlew :app:assembleDebug          # app/build/outputs/apk/debug/app-debug.apk
./gradlew :app:installDebug           # onto the device adb sees
ANDROID_SERIAL=emulator-5554 ./gradlew :app:connectedDebugAndroidTest   # the instrumented tests, on that device
```

Gradle builds the Rust parts through Cargo: `clipsync-ffi` for this machine (the bindings are generated from it and
the JVM tests load it), `clipsyncd` for the interop test, and `clipsync-ffi` for each Android ABI with `cargo-ndk`
(the `android` Cargo profile). `cargo-ndk` finds the NDK through the SDK.

The suites of `session`:

| Suite | What it runs |
|-------|--------------|
| `TlsTest` | Mutual TLS between two identities; refusing a client without a certificate, a peer without ALPN, an impostor |
| `FileStateStoreTest` | Saving and loading the state |
| `NodeTest` | Two nodes on loopback: QR pairing, syncing both ways, pausing, reconnecting after a restart, unpairing, refused tokens and forged QR codes |
| `InteropTest` | A node against the real `clipsyncd` on a private headless Sway: pairing with `clipsync pair`'s URI, then a Wayland copy reaching the node and text from the node reaching the Wayland clipboard |

The suites of `app`:

| Suite | What it runs |
|-------|--------------|
| `DeviceNameTest`, `QrDecoderTest`, `PairingTrackerTest`, `PairingInviteTest` (JVM) | The default device name; reading pairing QR codes from camera frames, including light-on-dark ones; how pairing events map to success or a failure; this phone's own code: its addresses, its QR modules read back, and how pairing mode ends |
| `SendOutcomeTest`, `SendIntentTest`, `ClipboardSendTest` (JVM) | What sending tells the user; the text a selection or a share carries; sending the clipboard (skipping sensitive text, giving up on a tap made on a locked phone that was not unlocked within a minute) and the tile's state |
| `KeystoreSyncTest` (on a device) | Two nodes with Keystore identities pair over loopback TLS and sync, through the native library |
| `SendFromDeviceTest` (on a device) | The activities that send, opened as the user opens them, sending through the running node to a second node on loopback: selected text, shared text, and the clipboard, except text marked sensitive. It wakes and unlocks the device (a PIN stops it) and replaces what the device's clipboard holds |

The instrumented tests need a phone or an emulator, so CI does not run them. `connectedDebugAndroidTest` uninstalls
the app when it finishes, which deletes its identity and pairings: run it on an emulator, and set `ANDROID_SERIAL` to
that emulator whenever a phone is also connected, since otherwise it runs on every device `adb` sees.

### Emulators

Clipboard access and the Quick Settings tile differ between Android versions: 10 is the oldest the app supports,
13 the last before `TileService` takes a `PendingIntent`, and 14 and newer take it. Emulators for 10 and 13, with the
SDK's `sdkmanager` and `avdmanager` (about 6 GB each):

```sh
sdkmanager "emulator" "system-images;android-29;google_apis;x86_64" "system-images;android-33;google_apis;x86_64"
avdmanager create avd -n clipsync-api29 -k "system-images;android-29;google_apis;x86_64" -d pixel_6
avdmanager create avd -n clipsync-api33 -k "system-images;android-33;google_apis;x86_64" -d pixel_6
emulator -avd clipsync-api33 -no-window -no-audio -no-snapshot-save   # headless; adb sees it as emulator-5554
```

To sync with an emulator without touching your clipboard, run a daemon with scratch directories (see
[Running the daemon](#running-the-daemon)) on a private headless Sway, like the tests do
(`WLR_BACKENDS=headless WLR_RENDERER=pixman sway -c /dev/null` with its own `XDG_RUNTIME_DIR`), and point the
daemon's `WAYLAND_DISPLAY` at that Sway. Keep the scratch paths short: Unix socket paths are limited to 108 bytes. The
emulator reaches the PC at its LAN addresses, so pairing with the link `clipsync pair` prints works; type it with
`adb shell input text` in chunks of about 30 characters (longer strings get cut) and with the emulator's keyboard
disabled (`adb shell ime disable …`), or autocorrection rewrites it.

The test identities are PKCS#12 files made with `keytool` (EC P-256, self-signed, password `testing`) in
`session/src/test/resources/identities/`; `TlsTest` pins their device IDs as computed by `openssl`.

## Desktop app

`desktop/` holds the Tauri app: `npm` runs the window's tools and the Tauri CLI, and `src-tauri/` is a Cargo
workspace of its own. From `desktop/`:

```sh
npm ci                                  # build and test tools only; the window ships no npm package
npm run build                           # tsc: src/ into ui/js/ (ignored by git)
npm run lint                            # tsc type check and eslint
npm test                                # node --test on src/**/*.test.ts
npm run licenses                        # every npm package under a permissive licence
npx tauri build --no-bundle             # src-tauri/target/release/clipsync-desktop
```

From `desktop/src-tauri/`:

```sh
cargo test                              # TrayView, and following a real daemon over its control socket
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo deny check                        # deny.toml: linux/'s policy plus Tauri's tree (D13)
```

| Suite | What it runs |
|-------|--------------|
| `src/view.test.ts` | What the window shows for a status: this device, the switch, the paired devices, a stopped daemon, a device's page, device names |
| `src/pairing.test.ts` | A pairing's states and events, the countdown, the QR path and pairing links |
| `clipsync-desktop` unit tests | `TrayView`: the tray's summary line, switch and icon for each status; the autostart entry, where it goes and how its path is quoted; the QR code's modules |
| `src-tauri/tests/daemon.rs` | `watch` and `request` against a real daemon with an in-memory clipboard: the status and its changes, a daemon that stops, one that starts later, unpairing, renaming |
| `src-tauri/tests/pairing.rs` | Pairing between two real daemons: a code shown and used, a pasted link, comparing codes (confirmed and refused), cancelling, pairing mode ending, a stale link |

The window and the tray are checked by hand. To do it without touching your desktop, run the app, and a daemon for
it, on a private headless Sway as described in [Emulators](#emulators): point both at that Sway's `WAYLAND_DISPLAY`
and `XDG_RUNTIME_DIR`, take screenshots with `grim`, and drive the window from the keyboard with `wtype` (headless
Sway has no pointer): give the window focus with `swaymsg`, send each step as one `wtype` run with pauses
(`wtype -s 300 -k Tab -s 300`), and press buttons with `space`. `zbarimg` reads the QR code back from a screenshot, and
`desktop-file-validate` checks the menu and autostart entries. To see that a second start shows the first one's
window, start both on one private bus (`dbus-daemon --session --fork --print-address=1`). Run the app under `dbus-run-session` to keep its tray icon off your bar, or on your session
bus to see it: its menu can then be read and clicked with `busctl` (`com.canonical.dbusmenu`). A private D-Bus
session mounts `gvfs` in the runtime directory; unmount it with `fusermount3 -u` before deleting the directory.

## Arch Linux package

`dist/aur/clipsync-git/PKGBUILD` builds a package from `main` with the daemon, the CLI and the desktop app, the
systemd user unit, the menu entry, the icon and the licence. The Rust and npm dependencies are fetched in
`prepare()`, from the lock files, and the builds run offline (`--frozen`). To build it without installing anything:

```sh
mkdir -p /tmp/cs-pkg && cp dist/aur/clipsync-git/PKGBUILD /tmp/cs-pkg/ && cd /tmp/cs-pkg
makepkg                        # add --nodeps when cargo or node come from rustup or mise, not pacman
tar -tf clipsync-git-*.pkg.tar.zst
```

Publishing to the AUR also needs the `.SRCINFO` that `makepkg --printsrcinfo` writes, generated when publishing. CI
does not build the package.

## Running the daemon

`cargo run -p clipsyncd` runs the daemon on your real clipboard, with your real identity and state under
`~/.local/share/clipsync`. To try it without touching those, point it at scratch directories, and give each extra
daemon its own port:

```sh
# The compositor's socket lives in the real runtime dir: pin it before moving XDG_RUNTIME_DIR.
export WAYLAND_DISPLAY="$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY"
export XDG_DATA_HOME=/tmp/cs-a/data XDG_CONFIG_HOME=/tmp/cs-a/config XDG_RUNTIME_DIR=/tmp/cs-a/run
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME/clipsync" && chmod 700 "$XDG_RUNTIME_DIR"
echo 'port = 47900' > "$XDG_CONFIG_HOME/clipsync/config.toml"
cargo run -p clipsyncd &
cargo run -p clipsyncd --bin clipsync -- status   # the CLI finds the daemon through the same variables
```

Two daemons on the same desktop share one clipboard: a copy reaches the other daemon both through the compositor and
over the network. That exercises echo suppression, but it is not how two machines behave.

## Trying the clipboard backend

The debug commands act on the **real clipboard** of the running session.
Use a separate compositor session with a disposable clipboard when one is
available. Otherwise, announce that the commands affect the desktop session's
clipboard and preserve its contents before testing.

`wl-paste -n` saves one representation of the selection. It does not preserve
all offered MIME types or metadata, and copying that data back cannot guarantee
a complete restoration. For rich content, multiple representations or content
that cannot be backed up, use an isolated session.

For a text selection, run this, then run the manual tests only if the backup
succeeds. `--type text` accepts any text representation the selection offers,
and `$XDG_RUNTIME_DIR` is private to your user and cleared at logout:

```sh
clipboard_backup=$(mktemp "${XDG_RUNTIME_DIR:?}/clipboard-backup.XXXXXX")
if wl-paste --no-newline --type text > "$clipboard_backup"; then
  printf 'Text backed up; keep this shell open for restoration.\n'
else
  rm -f -- "$clipboard_backup"
  unset clipboard_backup
  printf 'Backup failed; use an isolated clipboard session for testing.\n' >&2
fi
```

After testing, restore the text from the same shell. Delete the backup only
after restoration succeeds:

```sh
if [ -n "${clipboard_backup:-}" ]; then
  if wl-copy --type text/plain < "$clipboard_backup"; then
    rm -f -- "$clipboard_backup"
    unset clipboard_backup
  fi
fi
```

From `linux/`:

```sh
cargo run -p clipsyncd -- watch --show-text   # log clipboard changes and what would be sent
cargo run -p clipsyncd -- set "hello"         # own the clipboard until another app copies
RUST_LOG=debug cargo run -p clipsyncd -- watch  # also log the MIME types each app offers
```

With `watch` running, these exercise each outcome:

| Command | Expected log |
|---------|--------------|
| `wl-copy "olá 👋"` | `would send clip` |
| the same `wl-copy` again | `not sent outcome=Unchanged` |
| `printf '\x89PNG' \| wl-copy --type image/png` | `skipped reason=NotText` |
| `head -c 1100000 /dev/zero \| tr '\0' a \| wl-copy` | `skipped reason=TooLarge` |
| `printf 'ab\xffcd' \| wl-copy --type text/plain` | `skipped reason=InvalidUtf8` |
| `clipsyncd set …`, then copy anything else | `set` exits with `another app took the clipboard` |
