# Build, diagnostics and releases

[Back to the project](../README.md)

Run commands from the repository root. Read [CONTRIBUTING](../CONTRIBUTING.md) and [AGENTS.md](../AGENTS.md) before making changes.

## Build and verify

On Linux, install Rust, `libxkbcommon-devel` (or your distribution's equivalent)
and a C toolchain, then:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release --bin audeze-overlay
target/release/audeze-overlay --demo      # sample data, no headset needed
tools/package-linux.sh                    # dist/Audeze-Overlay-Linux-x86_64.tar.gz
```

Use a release build to try the overlay: debug builds are too slow for software
rendering. Windows steps follow. See [Linux](LINUX.md) for the compositor,
Flatpak and hardware notes.


```powershell
cargo test
cargo build --release --bin audeze-overlay
```

`build.ps1` uses project-local Rust / LLVM-MinGW tools when available:

```powershell
.\build.ps1 -Test
.\build.ps1
target\release\audeze-overlay.exe --smoke-test
cargo run --bin verify-direct -- --worker
```

Smoke checks use sample data, leave headset values and saved settings alone,
and write `diagnostics/smoke-v3.json`. The worker check reads live state without
changing sound. The separate opt-in `verify-direct --controls-and-restore`
diagnostic temporarily changes and restores sound settings.

For a refactor that must not change behavior, build with
`--features golden-checks` and run `--smoke-test --smoke-golden`. That adds twenty
more smoke checks and writes `diagnostics/ui-golden.txt`, a hash of the full
application state after each scripted step. Run it before and after the change
and compare the files line by line. The feature is left out of release builds.

For installed-app startup tests, verify preferences and the Run entry from the
normal Windows user session as well as the diagnostic shell. Isolated tooling
can expose a redirected settings or registry view even when the printed path
and user name match. Do not replace live user settings with a diagnostic copy;
back up and change only the intended preference in the actual user context.

`--demo` shows sample data. `--snapshot-preview --render-only` writes a PNG;
add `--settings --section 0` (0–5) for settings pages or `--preview-layout stacked`
for a tall layout. PNGs contain window pixels; real compositor blur is checked
by the separate `check-blur` diagnostic against a controlled desktop pattern.
Legacy accessibility diagnostics remain for comparison; the active app uses
`src/device/`. Build the old diagnostics with `--features legacy-uia`.

`cargo run --locked --release --bin bench-render` exercises 600 rasterized
animation frames after warm-up, with simulated wheel updates about every 50 ms.
It reports mean and 95th-percentile frame time and checks reused pixels against
fresh renders during warm-up. It opens no window and makes no headset calls.
This measures CPU rasterization, not Windows composition, physical input latency,
or game FPS. Keep the release profile consistent when comparing runs; the drawing
path is substantially slower with size optimization than speed optimization.

For intermittent latency diagnostics, launch with `--trace-usb`. A bounded
`usb-timing.csv` beside the executable records scheduled queries, raw replies,
parsed values, timeouts, and receipt-to-presentation time. Sequence numbers link
received values to visible frames, including the actual window opacity. Logging
is off by default and stops after 30,000 records. These timings do not measure
the physical wheel-to-device delay. Keep the panel open for counted single-step
tests so disappearance and fade-out cannot mask a value.

For an input-worker footprint measurement, quit the overlay and run:

```powershell
cargo run --release --features perf-stats --bin bench-worker -- --seconds=30
```

This read-only diagnostic warms up the real headset connection, then reports
worker queries, report reads, loop iterations, allocations and process CPU time.
Run it by itself, without `--trace-usb`, for comparable measurements. CPU time can
be quantized on Windows; a small or zero result does not prove zero CPU work.
Counters and the counting allocator are absent from ordinary app builds. This
measures USB-worker overhead, not physical wheel-to-screen latency.

`check-blur --corners` compares the real composed window with a contrasting
background baseline; a region-only test cannot detect acrylic spilling into
transparent corners.

The waiting-read probe found no sound-query replies on the interrupt input
stream. The observed report 4 is used by the installed app for head-tracking
re-centering, not sound state. `listen-headset` is a diagnostic only; the
production backend retains scheduled sound queries rather than assuming
unsupported push notifications.

## GitHub builds and releases


`.github/workflows/windows.yml` runs on every push, pull request and manual
workflow run. It checks formatting and Clippy (warnings are errors), runs Rust tests, verifies rendered layouts,
and creates an x64 portable ZIP, per-user installer and SHA-256
checksums. Open **Actions → Windows build and release → Artifacts → windows-x64**
for development builds. Layout preview images are a separate artifact.

Successful pushes to `main` automatically publish a new package version as the
[latest release](https://github.com/temdah/AudezeOverlay/releases/latest).
The title is **Audeze Overlay 1.0.0**, with a matching `v1.0.0` tag. Future
releases use versions such as 1.0.1 or 1.1.0. There are no build-number titles.
Older releases and downloads remain in history.

Before shipping changes:

1. Increment `version` in `Cargo.toml`, update `Cargo.lock` with Cargo and rebuild.
2. Write `docs/releases/<version>.md` describing features, fixes and relevant
   limitations in language intended for users. These reviewed highlights become
   **What's new** automatically. Commit messages are not release notes.
3. Commit and push to `main`. CI builds, tests, packages and publishes the version.

A push with an already released version runs the checks and produces CI artifacts
but does not publish another release or replace existing downloads. This lets
maintenance and documentation changes land without duplicating a release.
For a release from another branch, push a `v` tag matching its package version;
its matching release-notes file is also required. Hyphenated versions are prereleases.

`tools/publish-release.ps1` uploads the installer, portable ZIP and checksums to a
draft before publishing. A retry can repair a draft but cannot overwrite a public
release. Superseded main builds do not replace Latest. Run
`tools/test-publish-release.ps1` locally to check this behavior without credentials
or network access.

Windows displays **Audeze Overlay** in the installer, Start menu and installed
apps list. The separate version metadata identifies the package version. The
installer's application ID stays the same so updates replace the existing app.
Release publishing uses the repository's built-in
`GITHUB_TOKEN`; no personal token or extra repository secret is required.
Publishing permissions are limited to the release job. Actions are pinned to
commit hashes, and the Inno Setup compiler download has a pinned SHA-256 hash.

Build the same packages locally with Rust/MSVC installed:

```powershell
.\tools\package.ps1
```

With the project's local LLVM tools:

```powershell
.\build.ps1
.\tools\package.ps1 -BinaryPath target\release\audeze-overlay.exe -Target x86_64-pc-windows-gnullvm
```

Output is in `dist/release`. Packages contain only the app, documentation and
license notices; personal fonts, logs and settings are excluded. They are
currently unsigned. A signing certificate would be needed for signed releases.
