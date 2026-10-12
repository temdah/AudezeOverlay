# Documentation

Start with the [project overview and downloads](../README.md).

| Guide | What it covers |
| --- | --- |
| [User guide](USER_GUIDE.md) | Installation, controls, tray, layout, settings, shortcuts and live EQ |
| [Performance measurements](PERFORMANCE.md) | Measured comparison, methodology, trial variability and reproduction |
| [Device integration and compatibility](DEVICE_AND_COMPATIBILITY.md) | USB scheduling, latency limits, rendering, anti-cheat boundaries and visual assets |
| [Device profiles and validation](DEVICE_PROTOCOL.md) | Experimental model identities, protocol evidence, wired connections and remaining hardware checks |
| [Linux](LINUX.md) | Wayland overlay, install, Flatpak, hardware evidence, the udev rule and diagnostics |
| [Build, diagnostics and releases](DEVELOPMENT.md) | Local builds, diagnostic modes, packaging and GitHub releases |
| [Architecture and Rust reading guide](ARCHITECTURE.md) | Module responsibilities, ownership and interaction flow |
| [Contributing](../CONTRIBUTING.md) | Development checks and commit conventions |
| [Agent instructions](../AGENTS.md) | Coding baseline and repository workflow for coding agents |

## Screenshots

Images in `screenshots/` were captured on 10 October 2026 using the application's
own preview/rendering tools and the public Windows font fallback. They show real
application views with sample headset data, not measured live headset readings.
The PNGs contain window pixels; they do not capture Windows compositor blur or
demonstrate game compatibility. No headset settings were changed to produce them.

The sound and appearance images use `--snapshot-preview --render-only`; layout
and EQ views use `preview-layouts`. See the development guide for these tools.
