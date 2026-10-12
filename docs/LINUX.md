# Linux

[Back to documentation](README.md)

> **Status: experimental.** The Linux build is a native Wayland application with
> the same overlay, controls and EQ as Windows. Contributor testing covers one
> headset and one desktop (below). There is no Linux release download yet; CI builds a test
> archive for each push.

## What you get

- **The overlay** is a wlr-layer-shell surface on the overlay layer, so it shows
  above other windows, including fullscreen games, on compositors that support
  that protocol: KDE Plasma, Sway, Hyprland, niri, COSMIC, labwc and others.
  **GNOME's Mutter does not offer layer-shell**, so the overlay cannot be drawn
  there. X11 sessions are not supported.
- **Same controls as Windows:** game/chat mix, noise cancellation, sidetone,
  microphone state, battery and EQ.
- **Desktop integration:** tray icon (StatusNotifierItem), a global shortcut
  through the GlobalShortcuts portal, launch at login, EQ profile import/export
  with the portal's file chooser, a single running instance, fractional display
  scales and blur behind the window (`ext-background-effect`; the compositor picks
  the strength, so the setting is on/off).
- **No game access.** The overlay is its own window. It does not inject into
  games, hook graphics or read other programs' input.

The headset protocol, device selection and polling worker are shared with
Windows. Linux adds a transport that talks to the kernel's **hidraw** interface
directly: no driver, no daemon and no libudev.

## Install

From the experimental CI archive, or a source checkout after
`cargo build --release`:

```sh
./install.sh --udev          # or packaging/linux/install.sh --udev from a checkout
```

This installs the program, desktop entry and icons under `~/.local` and, with
`--udev`, the udev rule below (it asks for `sudo`). `./install.sh --uninstall`
removes what the script installed. Start Audeze Overlay from the application
menu. Starting it again shows the window; `audeze-overlay --toggle` shows or
hides it.

### Global shortcut

The shortcut is owned by your desktop. On first start the app asks the
GlobalShortcuts portal for **Alt + Home**; KDE shows a dialog where you can keep
or change it, and afterwards lists it in System Settings → Shortcuts. If your
desktop has no such portal, bind `audeze-overlay --toggle` to a key in its
shortcut settings (on Sway or Hyprland, a keybinding that runs the command).

### Flatpak

`packaging/flatpak/` holds a manifest built the way Flathub requires: offline,
from vendored Cargo sources (`tools/sync-cargo-sources.sh` regenerates them and CI
checks they match `Cargo.lock`). The generator uses Python 3.11+ and the committed
lockfile; it does not download or execute an external generator. Both Linux
packages include the application and dependency license notices. Build and run it locally:

```sh
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/io.github.temdah.AudezeOverlay.yml
flatpak run io.github.temdah.AudezeOverlay
```

It needs the Freedesktop 25.08 platform, SDK and Rust extension. The sandbox has
Wayland, `--device=all` (the only way to reach `/dev/hidraw`) and the tray's
D-Bus name. Autostart goes through the Background portal. The headset's udev rule
still has to be installed on the host. This manifest has not been submitted to
Flathub, and Flathub's policy on AI-written submissions would need reviewing
before anyone did.

## Hardware evidence

Only one device has been tested on Linux:

| Device | Firmware | Result |
| --- | --- | --- |
| Audeze Maxwell, Xbox dongle (`3329:4b18`) | headset and dongle v1.0.1.74 | Status reads, game/chat mix, noise filter, sidetone on/off, sidetone level and EQ preset selection were each changed, read back and restored. An unplug and replug was detected and the dongle was found again. |

The contributor reports testing on Fedora 44 (kernel 7.2) under KDE Plasma 6 on Wayland, with a person at the
headset: the overlay, popups, menus, tray, global shortcut, launch at login,
blur and the Flatpak build were used by hand on that machine. Additional scaling
checks used headless Sway; this does not verify other compositors or physical
monitor hot-plug. Maintainer review ran automated Linux CI and Windows UI checks,
not a second Linux hardware validation. GNOME is untested and lacks the required
layer-shell protocol.
The unit tests use synthetic fixtures and are not hardware evidence. Nothing here
establishes support for other models, wired use, PlayStation dongles, other
firmware, or Maxwell 2 and Maxwell 2 ANC (including ANC and custom EQ editing) on
Linux. The same identities remain experimental on Windows; see the
[validation matrix](DEVICE_PROTOCOL.md).

## Requirements

- **Linux 5.11 or newer.** Input reports are fetched on demand with the
  `HIDIOCGINPUT` request, which older kernels lack. An older kernel is reported as
  such rather than as a lost connection.
- **A udev rule** that lets your user open the headset's `/dev/hidraw` node. Most
  distributions make these nodes root-only.
- A graphical session managed by systemd-logind (the rule grants access to the
  user who is logged in at the seat).

## Allowing access to the headset

Install the rule from this repository once, then reconnect the dongle:

```sh
sudo install -m 0644 packaging/linux/70-audeze-overlay.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=hidraw --action=change
```

The rule lists only the twelve USB product IDs the application recognizes
(`docs/DEVICE_PROTOCOL.md`). It uses `TAG+="uaccess"`, so access goes to the
logged-in user through an ACL and ends with the session; it does not make the
device world-writable. To undo it, delete the file and run
`sudo udevadm control --reload`.

Without the rule the application reports "Permission denied opening the
headset's /dev/hidraw device" rather than behaving as if no headset were
connected.

## Building and checking

```sh
cargo test --locked --lib
cargo build --locked --release --bin verify-direct
```

`verify-direct` is a diagnostic. With no arguments it only **reads**:

```sh
target/release/verify-direct              # one read-only snapshot
target/release/verify-direct --worker     # run the real polling worker for 4 s
```

`--controls-and-restore [--eq-select]` **changes your headset's settings** (mix,
noise filter, sidetone, sidetone level and, with the flag, the EQ preset), reads
each back and restores it, then compares the final state with the original. The
changes are audible, and changing the sidetone level switches sidetone on; the
diagnostic switches it back off. Run it only while you are at the headset.

## Behavior of the original Maxwell worth knowing

These are observations from the hardware above, not documented guarantees:

- **Status replies take about 130 ms**, longer than the worker's 100 ms timeout
  for most queries. Late replies are still parsed, but when the first sidetone
  range query misses its deadline the worker stops asking for the sidetone level,
  so that value is often blank. The identical trace was recorded on Windows. It is
  an existing behavior and has not been changed here.
- **Changing the sidetone level turns sidetone on.** Code that sets the level
  must not assume the on/off state is unchanged.
- Switching sidetone on can take up to about a second to read back as changed,
  while the headset applies it. The worker asks again after 50 ms, and a sidetone
  change is reported as unconfirmed only after 2 s.
