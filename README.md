# Audeze Overlay

**See what your headset is doing. Keep the game in focus.**

A lightweight, native Rust overlay for Audeze Maxwell headset controls. Turn the
game/chat wheel and get a visual readout, or open the panel to adjust sound and EQ.
It talks directly to the USB dongle, so **Audeze App can stay closed**.

The app also includes experimental wired USB and additional Maxwell
profiles. See the [coverage and validation matrix](docs/DEVICE_PROTOCOL.md);
additional hardware is not yet verified and non-ANC custom EQ editing is disabled.

**[Download for Windows — installer](https://github.com/temdah/AudezeOverlay/releases/latest/download/Audeze-Overlay-Setup.exe)** ·
[Portable ZIP](https://github.com/temdah/AudezeOverlay/releases/latest/download/Audeze-Overlay-Portable.zip) ·
[What's new](https://github.com/temdah/AudezeOverlay/releases/latest) ·
[Documentation](docs/README.md)

**Linux (experimental, Wayland):** see [Linux](docs/LINUX.md). Development builds
are produced by CI as a test archive; there is no Linux release download yet.

## One overlay, your layout

Arrange it your way. Resize, rearrange, and hide cards, then adjust transparency
and blur to suit your screen. These are just two examples of how you can customize
the same overlay. The fixed status row keeps the headset model,
connection, battery and microphone state in view.

<table>
<tr>
<td valign="top"><img src="docs/screenshots/vertical.png" width="220" alt="Customized overlay with translucent cards and a Maxwell 2 ANC wireless status row"></td>
<td valign="top"><img src="docs/screenshots/sound.png" width="570" alt="Customized overlay with a large mix dial, sound controls beside it and a headset status row"><p>Your cards, your spacing, your appearance. Use a narrow column, spread controls across the screen, or keep only what you need.</p><p>Text and icons stay solid as the panels become more transparent. Both examples include the optional status row and have layout editing turned off.</p></td>
</tr>
</table>

*Rendered by the application with sample readings and the public font fallback.
These previews use a neutral backdrop to show panel opacity; live Windows blur
depends on what is behind the overlay.*

## Built to stay small

**About 80% less resident memory than Audeze App in our latest local test.**
The overlay averaged **21.51 MiB** while visible, compared with **105.95 MiB** for
the official app. It runs independently, with no web runtime or permanent render
loop.

| Visible, connected application | Audeze Overlay | Audeze App |
| --- | ---: | ---: |
| Resident memory | **21.51 MiB** | 105.95 MiB |
| Private committed memory | **7.09 MiB** | 150.53 MiB |
| CPU, percentage of one logical core | **0.193%** | 0.965% |

Measured on 11 October 2026 using the final Overlay 0.4.1 build and Audeze App
1.2.48 on Windows 11 with a Ryzen 7 9800X3D. Each figure averages two 60-second
idle samples, with the apps run separately and the headset connected. Results
depend on your layout and system; this test measures process resources, not FPS
or physical input latency. The official app also includes firmware management.

[Read the measurements, limitations and reproduction steps →](docs/PERFORMANCE.md)

## Your sound, within reach

- **Game/chat balance:** circular dial with visual feedback for physical wheel
  changes. Drag, scroll, or double-click to return to 50/50.
- **Sound controls:** sidetone, ANC/Transparency, and microphone noise removal.
- **Live EQ:** tune ten bands, switch presets, and pin your favorites. Save keeps
  your curve; Discard restores the curve from before editing. Import profiles,
  export portable files, and save a new copy in your local preset library.
- **Useful status:** headset model, connection, battery, and microphone mute.
- **Your layout:** move the overlay, resize and rearrange cards, or hide what you
  do not need.
- **Stay out of the way:** configurable popups, show/hide shortcut, tray controls,
  background/card transparency, and Windows blur.

| Arrange your controls | Tune your EQ |
| --- | --- |
| ![Grid layout editing with card resize handles](docs/screenshots/layout.png) | ![Custom ten-band EQ editor and preset pins](docs/screenshots/equalizer.png) |

Live EQ writes to the headset immediately. Keep the app open until you Save or
Discard; exiting before Discard leaves the latest applied curve on the headset.

<details>
<summary>Appearance and settings preview</summary>

![Appearance settings for scale, text, opacity, blur and animations](docs/screenshots/appearance.png)

Choose the information you want, how long notifications remain visible, and the
shortcut that opens the panel. [Full settings and controls guide](docs/USER_GUIDE.md).

</details>

## Get started

1. Install with **Audeze-Overlay-Setup.exe**, or extract the portable ZIP and run
   **Audeze Overlay.exe**.
2. Connect your supported headset through its USB dongle.
3. Press **Alt + Home** to show or hide the panel. Open Settings to record your
   preferred shortcut; the tray also provides Open, Reset position, and Quit.

**Hardware tested:** Maxwell 2 ANC PlayStation dongle (`3329:4BA9`) on Windows 11,
plus wired status reads and mix recovery across cable changes (`3329:4BAA`).
Other Maxwell identities and wired EQ editing remain experimental. Bluetooth is
limited to Windows connection status, with no Bluetooth sound controls.
See the [validation matrix](docs/DEVICE_PROTOCOL.md) before trying another model.
Use borderless/windowed games;
exclusive fullscreen can prevent ordinary desktop overlays from appearing.
Packages are currently unsigned.

The overlay uses a separate desktop window, with no game injection, graphics
hooks, game-memory access, or custom driver. **Anti-cheat vendor approval and
universal compatibility are not claimed.**

## Learn more and contribute

[User guide](docs/USER_GUIDE.md) · [Performance](docs/PERFORMANCE.md) ·
[Device and compatibility details](docs/DEVICE_AND_COMPATIBILITY.md) ·
[Build and release guide](docs/DEVELOPMENT.md) ·
[Architecture](docs/ARCHITECTURE.md) · [Contributing](CONTRIBUTING.md)

Independent community project, not affiliated with or endorsed by Audeze.
Code is licensed under [MIT](LICENSE). Audeze branding retains its original
ownership; see [third-party notices](THIRD-PARTY-NOTICES.md).
