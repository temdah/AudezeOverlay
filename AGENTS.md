# Agent instructions

These instructions apply throughout this repository. Follow more specific nested
instructions when present and the user's explicit directions when they differ.
Read `CONTRIBUTING.md` and `docs/ARCHITECTURE.md` before implementation. Check the
actual source and working tree; documentation and old handoffs can be stale.

## Product and scope

Audeze Overlay is a small Rust application for Windows 11 (Win32) and, experimentally,
Linux (Wayland). It reads and
controls supported headset settings directly through a USB dongle, displays a
native overlay, and works without Audeze App running. Keep the UI responsive and
the idle footprint small.

The verified integration at the time of writing is Maxwell 2 ANC through its
supported dongle. Check README and device profiles for current coverage. Never
describe additional models as supported based only on UI research or mocks.

## Before editing

- Inspect `git status --short`, the current branch, recent commits, and relevant
  diffs. Preserve user changes, including changes outside your immediate task.
- Search with `rg` and read the existing implementation before inventing another
  abstraction. Identify the owner of the behavior and the regression to prevent.
- Keep changes focused. Do not combine a feature with unrelated cleanup,
  dependency upgrades, formatting churn, or large architectural rewrites.
- Complete authorized work through verification. Ask only when missing
  information changes the outcome or an action needs authority not already given.
- Do not spawn agents, create tasks, contact vendors, or message other people
  unless the user or applicable instructions explicitly authorize it.

## Clean Rust baseline

Use Rust's ownership and type system to express the rules. Apply SOLID principles
through clear responsibilities and dependency boundaries, not through an
interface, trait, or service wrapper for every struct.

- Prefer small, cohesive modules with one clear reason to change. Split files by
  responsibility, not arbitrary line counts. Keep related behavior together.
- Dependencies should point toward data and domain rules. Domain code must not
  need a window handle, USB device, or global application object to be tested.
- Prefer functions for pure transformations, enums for a closed set of cases,
  and structs for owned state. Add traits when multiple implementations or a
  meaningful testing boundary justify them. Avoid speculative generic frameworks.
- Keep visibility narrow: private first, then `pub(crate)` where appropriate.
  Expose only what callers need. Avoid passing an entire application state when a
  narrower value or reference is sufficient.
- Use descriptive domain names and normal Rust conventions: snake_case functions
  and modules, PascalCase types, SCREAMING_SNAKE_CASE constants. Name units in
  fields/parameters when ambiguity matters. Avoid vague helpers such as `manager`,
  `utils`, or `process_data` without a specific responsibility.
- Model unavailable values with `Option`; do not silently turn them into zero,
  false, or Off. Use validated types/enums for new bounded values and modes rather
  than adding more loosely related strings or boolean flags.
- Use `Result` and useful context for expected failures. Do not add `unwrap` or
  `expect` to recoverable device, filesystem, configuration, or user-input paths.
  Document why an invariant is guaranteed if a panic is truly intentional.
- Prefer borrowing over cloning when practical. Do not add `Arc<Mutex<_>>` to
  avoid deciding ownership. Keep shared locks short; do not hold them during USB
  operations, rendering, sleeps, callbacks, or other potentially blocking work.
- Make resource lifetime explicit with RAII. Keep unsafe code at the native
  boundary, document its safety preconditions, and preserve handle ownership,
  callback lifetime, and reentrancy guarantees. Do not spread raw pointers through
  domain or view code.
- Comments explain intent, units, protocol evidence, and invariants. Avoid
  narrating obvious syntax. Public APIs should explain important failure behavior.
- Add dependencies only for a clear need that existing code cannot reasonably
  meet. Consider binary size, runtime cost, maintenance, and licensing. Keep
  `Cargo.lock` committed.

## Responsibility boundaries

| Location | Owns |
| --- | --- |
| `src/model.rs` | Sound snapshots, change detection, optimistic mix reconciliation |
| `src/eq.rs`, `src/eq/` | EQ data, drafts, confirmation rules and local name import |
| `src/layout.rs`, `src/input.rs`, `src/motion.rs` | Pure layout, input validation and time-based transitions |
| `src/config.rs` | Defaults, migration, bounds and preference persistence |
| `src/device/command.rs` | Typed commands, sequencing and bounded/coalesced dispatch |
| `src/device/usb.rs` | Device access, protocol encoding, framing and parsing |
| `src/device/worker.rs` | Scheduling, replies, readbacks and reconnection |
| `src/ui/draw.rs`, `src/ui/view/` | Drawing primitives and state-to-pixels/hit regions |
| `src/ui/presentation.rs` | Presentation and animation state |
| `src/ui/app.rs`, `src/ui/app/`, `src/ui/shell.rs` | Portable application coordination and the `Shell` interface |
| `src/ui/win32.rs`, `src/ui/win32/` | Windows window lifetime, events and presentation |
| `src/ui/wayland.rs`, `src/ui/wayland/` | Linux window, input, portals and tray |
| `src/platform/` | Windows-specific services such as startup and composition |
| `src/bin/` | Explicit diagnostic/development executables |

Views must not write headset settings, persist preferences, or perform blocking
I/O. Native event handlers translate actions; the USB worker owns device work.
Put model-specific protocol differences in the device layer, not scattered UI
conditionals. A refactor must preserve the existing behavioral contracts.

## Device and performance contracts

- Match supported identities and interfaces explicitly. Unknown or ambiguous
  devices must not fall back to the current ANC protocol. Add capabilities for
  model differences and reject unsupported commands at dispatch as well as in UI.
- Use only understood sound-setting commands. Do not expose arbitrary commands,
  firmware updates, bootloader operations, pairing changes, or device resets.
- Record protocol provenance. Synthetic fixtures test code boundaries; they are
  not captured hardware evidence. Real support requires model/variant/firmware
  validation, including restoration after control tests.
- Preserve the current 50 ms mix target, approximately 1 s background sound
  queries, 30 s battery queries, and on-demand EQ. Read the current scheduler
  before changing it. Waiting-read experiments remain deferred unless requested.
- Avoid busy loops, unbounded queues, unnecessary allocations, repeated idle
  rendering, and unsupported queries that repeatedly time out. Keep bounded
  retries, response framing, timeouts and coalescing. Preserve visual quality and
  responsiveness when optimizing; measure before and after meaningful changes.
- Older readbacks must not rewind a newer local mix target. An EQ ACK does not
  prove the active index or curve has settled. Preserve separate selection/bank
  revisions and ordered readback verification. Discard restores the original EQ
  curve; remember that live preview writes to the headset immediately.
- Disconnect or identity changes clear device-derived readings and pending state.
  Unsupported, disconnected, and temporarily unavailable are different states.
- Keep native fonts/glyph caching and event-driven painting. Animation timers
  should stop when idle. Do not add a web runtime or permanent frame loop.
- No game injection, graphics hooks, game-memory access, input automation, custom
  driver, or global keyboard hook. Use Windows `RegisterHotKey`. Do not disable
  anti-cheat or claim vendor approval/universal compatibility without evidence.

## Validation

Use checks appropriate to the change. Documentation-only changes need link/path,
accuracy and diff checks, not a Rust rebuild. For code, run formatting, Clippy,
meaningful tests and a release build. Add regression tests for important state,
protocol, scheduling and layout behavior; avoid tests that mirror implementation
details or merely assert constants. Use explicit time in animation tests.

On the existing Windows workspace, `build.ps1` configures the optional local
Rust/LLVM toolchain in the current PowerShell process. Subsequent cargo commands
must run in that same process. CI uses stable Rust with MSVC independently.

```powershell
& .\build.ps1 -Test
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
& .\build.ps1
```

Check every command's exit status and fix failures. PowerShell does not
automatically stop on every native-command failure. Do not suppress warnings or
weaken assertions to obtain a green result.

For visual changes, run `cargo run --locked --release --bin preview-layouts` and
inspect the generated images at relevant widths/scales. For window/input changes,
quit the exact running overlay instance, run the new executable with
`--smoke-test`, inspect all results in `diagnostics/smoke-v3.json`, and confirm the
file was written after the test started. The singleton can otherwise redirect to
an old process and leave stale results. Restore the normal app afterward.

Inspect hardware diagnostic source before running it. Some verify tools change
settings. Hardware tests require deliberate authorization for their effects and
must restore original settings. Unit tests and sample-data smoke checks do not
establish hardware support, physical latency, or anti-cheat compatibility.

For packaging, use `tools/package.ps1`; do not hand-assemble release downloads.
With the local LLVM binary, pass `-BinaryPath target\release\audeze-overlay.exe`
and `-Target x86_64-pc-windows-gnullvm`. Inspect archive contents and checksums.

## Git, commits and publishing

- Confirm the remote and branch before pushing. Expected remote is
  `https://github.com/temdah/AudezeOverlay`. Work on a descriptive `codex/` branch
  for substantial changes; do not create a worktree merely to follow convention.
- Stage explicit files, review `git diff --cached`, and run `git diff --check`.
  Do not stage or commit unrelated user edits. Never use destructive clean/reset
  or force-push to resolve a routine problem.
- Use conventional commits describing the result, such as
  `fix(layout): preserve card size while dragging` or
  `refactor(device): separate profile matching from transport`. No coauthor
  trailers. Keep commits cohesive and do not imply unverified hardware support.
- Push when the user authorizes publication; authorization may already exist in
  the conversation. A routine edit alone does not require an unsolicited push.
  If credentials fail, report the failure; do not print tokens or change accounts.
- Branch pushes run checks and create artifacts. After integration, a successful
  push to `main` publishes a new **Latest** release under `v<Cargo.toml version>`
  when that version has not already been released. Increment the package version
  for release changes (for example 1.0.0 -> 1.0.1), update Cargo.lock, and write
  `docs/releases/<version>.md` with user-facing highlights before pushing.
  Previous releases and assets remain in history; never move their tags or replace
  their downloads. CI publishes the reviewed highlights automatically; never use
  commit subjects or generated technical changelogs as What's new. Release titles
  are `Audeze Overlay <version>`; the installed Windows name is `Audeze Overlay`.
  A main push is therefore a public release action. Integrate remote changes
  without overwriting others, and verify the CI run for the exact pushed SHA.
- Numbered tags must be `v` plus the exact version in `Cargo.toml`. Update
  `Cargo.lock` and rebuild after version changes. Create a numbered tag only for
  an intended versioned release; main publication creates the matching tag itself.
- Publication is complete only after build and release jobs succeed and
  `https://api.github.com/repos/temdah/AudezeOverlay/releases/latest` has the
  expected commit in its notes and the three intended assets:
  `Audeze-Overlay-Setup.exe`, `Audeze-Overlay-Portable.zip`, `SHA256SUMS.txt`.
  Verify downloads and What's new. A tag alone is not a published release.
- Preserve the clear recommended-installer and portable-option release format.
  Keep licenses inside packages. Do not upload the whole local output directory,
  where stale builds can remain.

## Privacy, assets and signing

Respect `.gitignore`. Never commit `.tools`, `target`, `dist`, diagnostics,
captures, settings, private fonts, credentials, signing keys, proprietary app
binaries/decompiled source, personal review files, or handoff documents. All handoff
filename variations are private and must remain ignored. Never force-add them,
rename them to evade the ignore rules, or include them in release packages. Public documentation
must be independently written and must distinguish inference from verified facts.

Signing status is determined by actual configuration and verified artifacts.
A signing-provider application is not approval. Do not use an employer's Azure
identity/resources for this personal project without their explicit authority.
Never ask users to paste private keys, tokens, or identity documents into chat.

## Completion report

Report the behavior changed, why, checks actually run, and material limitations.
For hardware work identify models/variants tested and what remains unverified.
For publication include commit and release/CI status. Say when checks or pushes
were blocked; never report intended work as completed. Keep handoffs and private
continuation notes local, and provide concrete next steps when evidence is missing.
