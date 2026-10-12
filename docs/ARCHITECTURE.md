# Architecture and Rust reading guide

This is a desktop application for Windows and Linux (Wayland) with a small
pure-Rust core. The executable owns a native window and a USB worker; it does not load into a game or require the
official Audeze application. Dependencies point toward data and domain rules.

## File map

| Area | Responsibility | Start here |
| --- | --- | --- |
| Domain | Sound readings, change detection, optimistic mix reconciliation | `src/model.rs` |
| Equalizer | Validated band data, wire encoding, local drafts and pin preferences | `src/eq.rs` |
| Layout | Grid placement, minimum sizes, swaps, compaction | `src/layout.rs` |
| Input | Editable numeric drafts and validation | `src/input.rs` |
| Motion | Time-based easing and directional selection transitions | `src/motion.rs` |
| Preferences | Defaults, migrations, bounds, persistence | `src/config.rs` |
| Device commands | Typed commands, bounded/coalesced queue, error interface | `src/device/command.rs` |
| Device profiles | Explicit USB identities, model capabilities and control routing | `src/device/profiles.rs` |
| Wire protocol | Query types, response framing and frame parsing; pure, no device access | `src/device/protocol.rs` |
| Device selection | Choosing the one control interface to use; ambiguous models or duplicates are rejected | `src/device/selection.rs` |
| Device session | Sound commands, polling and reply handling over one HID interface | `src/device/usb.rs` |
| HID transport | Per-platform enumeration and report I/O: Win32, and Linux hidraw with hot-plug | `src/device/hid.rs`, `src/device/hid/` |
| Report descriptors | Finding the vendor control collection in a HID report descriptor | `src/device/descriptor.rs` |
| USB scheduling | Worker thread, reads, acknowledgments, reconnects, tracing | `src/device/worker.rs` |
| Raster drawing | Pixels, cached font glyphs, vector logo and primitives | `src/ui/draw.rs` |
| Views | Read-only state to pixels and hit regions | `src/ui/view/` |
| Presentation | Per-card and per-control animation state | `src/ui/presentation.rs` |
| Application logic | What every input and event does, written against the `Shell` trait; portable | `src/ui/app.rs`, `src/ui/app/` |
| Shell interface | The window, timers, menus, dialogs, tray and shortcut the logic may ask for | `src/ui/shell.rs` |
| Windows shell | Win32 window, messages, GDI presentation, golden and smoke tests | `src/ui/win32.rs`, `src/ui/win32/` |
| Wayland shell | Layer-shell window, input, popups, portals, tray, single instance | `src/ui/wayland.rs`, `src/ui/wayland/` |
| Windows services | Startup registry entry and composition effects | `src/platform/` |
| Linux packaging | Install script, desktop entry, icons, udev rule, Flatpak manifest | `packaging/linux/`, `packaging/flatpak/` |
| Diagnostics | Explicit opt-in executables, previews and hardware probes | `src/bin/` |
| Legacy comparison | Official-app accessibility probes, feature-gated | `src/legacy/` |

`src/main.rs` is the executable entry point. `src/lib.rs` exposes reusable core
modules to the executable and diagnostics. There is no need for one crate per
folder: modules provide useful boundaries without a workspace of tiny packages.

## Follow one interaction

1. A pointer message reaches `win32/events.rs` (or a pointer event reaches
   `wayland.rs`), which calls the shared input function. Hit testing produces an `Action` enum.
2. The application translates that action into a layout operation, preference
   change, or device `Command`.
3. A sound command enters `Controller`. A newer command of the same kind replaces
   an obsolete queued one, so dragging does not build a long backlog. EQ edits
   coalesce by slot and band ID, preserving edits to other bands.
4. The worker performs USB work and updates shared `SoundState`, then wakes the
   shell (a Windows message, or a Wayland loop ping). The UI never waits for USB inside its event callback.
5. Mix reconciliation keeps a new local target from being replaced by an older
   readback. A failure releases that target and shows an error.
6. `Presentation` computes a frame at the supplied time. `View` borrows the state;
   rendering creates pixels and hit regions, without writing to the headset.
7. A short animation timer requests the next frame only while motion is active.

Hardware changes enter at step 4. A newly visible popup starts at the received
value; it does not replay invisible intermediate animation frames.

## A suggested reading order

1. **`model.rs`**: structs, enums, `Option`, pattern matching and unit tests.
   `Option` means a reading may be unavailable; it is not automatically zero.
2. **`input.rs` and `motion.rs`**: small state machines and methods on owned
   structs. Tests pass explicit times, avoiding sleep-based animation tests.
3. **`layout.rs`**: copied value types, arrays, iterators, and invariants. Follow
   a swap test before reading the complete placement algorithm.
4. **`config.rs`**: Serde derives, defaults, migration, `Result`, and `?` error
   propagation. Settings are serialized before replacing the saved file.
5. **`ui/view/mod.rs`**: borrowed `&Config` and `&SoundState`. A borrowed view is
   temporary and cannot outlive the state it references.
6. **`device/command.rs` and `worker.rs`**: `Arc` shares ownership across threads;
   `Mutex` controls access; the bounded wake channel avoids accumulating wakes.
7. **`ui/shell.rs` and `ui/app/input.rs`**: the trait that hides the platform and
   the plain functions that turn input into application changes.
8. **`ui/win32/gdi.rs` and `ui/win32/events.rs`**: RAII for GDI handles and the
   unsafe Windows boundary. Read the lifetime comment and reentrancy guard before
   following the raw pointer conversion.
9. **`ui/wayland/shell.rs`**: the same `Shell` over a layer-shell surface, a shared
   memory frame and calloop timers.

Rust does not require a base class or interface for every component. Use enums
for a closed set of actions and commands, functions for pure transformations,
and traits where multiple implementations or a testing seam actually help.

## Platform shells

`App` owns a `Box<dyn Shell>` and never touches a window handle. The Windows shell
translates messages into the input functions and presents frames through GDI. The
Wayland shell draws into a shared-memory buffer on a wlr-layer-shell overlay
surface and uses portals for the file chooser, global shortcut and autostart in a
sandbox. Both render with the same CPU rasterizer. A few Windows-only behaviors
(acrylic, Win32 diagnostics) are reached through `win32` downcasts and do not
exist in the Linux build. Linux enumerates the headset through hidraw
(`src/device/hid/linux.rs`) and watches for plug and unplug with a netlink
socket; the protocol, selection and worker code is shared.

## Ownership and native callbacks (Windows)

`run` owns a `Box<WindowState>` for the HWND lifetime. Windows stores a pointer to
that stable allocation. The callback retrieves it only while the HWND is alive;
`WM_NCDESTROY` clears the pointer. `RefCell<App>` permits one mutable UI borrow.
Synchronous re-entry cannot create a second mutable reference. Status, result,
paint and resize signals are coalesced and posted after the borrow ends.
Stateless frame responses still run during re-entry to preserve borderless
geometry. The default window procedure runs without the UI borrow on the normal
path, allowing Windows' native move/resize message loop to work.

The worker owns the USB device. The UI owns rendering resources. Shared sound
state is exchanged through a short-held mutex, not through raw pointers to UI
objects. The worker lifetime is currently the process lifetime.

## Validation boundaries

Pure tests cover layout and state rules. Render previews cover compact layouts
and intermediate animation frames. Native smoke checks exercise actual HWND,
focus, click-through, resizing, hotkeys and callback re-entry using sample data.
USB diagnostics are separate because physical hardware and firmware timing
cannot be reproduced by the unit tests. Never run a control-changing diagnostic
as an ordinary CI test.

## EQ data ownership

On Wayland, file chooser portals run on a dedicated thread and return one result
to the UI loop. The application retains one pending import/export action and its
original tuning until completion or cancellation. Worker status and completion
notifications are coalesced into two flags, so a modal menu cannot accumulate an
unbounded queue. Menus and confirmations remain modal; the headset worker keeps
running and the latest snapshot is consumed when the menu closes.

The headset owns four custom EQ banks. `SoundState` carries the latest read bank,
while `eq::Editor` keeps up to four independent drafts in the UI. Loading another
bank never overwrites an unsaved draft. Live gestures emit coalesced band commands.
Save confirms the final curve, and Discard writes the baseline back. Both finish
only after an ordered bank readback matches; touched-band tracking makes failed
restoration retryable even when the displayed draft has returned to baseline.
Selection and band confirmations have separate sequence numbers. EQ
queries are on demand and excluded from routine polling. Reads and writes use
different gain/Q byte order, covered by protocol tests and hardware diagnostics.
