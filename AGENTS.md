# StageSwap repository guidance

Use this guide for engineering, build, test, packaging, deployment, and release work on StageSwap. The main [README](README.md) is the product presentation and end-user guide.

## Product contract

StageSwap is a local-only, native Windows 11 x64 virtual camera for automatic Zoom retransmission during congregation meetings using JW Library. It watches the second display visually and does not integrate with or control JW Library.

- Frames are immutable CPU BGRA and the internal/output contract is fixed at 1280×720, 30 fps.
- Webcam input uses Media Foundation tiered negotiation: it prefers progressive RGB32 1280×720 at 30 fps, safely falls back through compatible RGB32, NV12, YUY2, and MJPEG modes, and normalizes row-aware input into the fixed BGRA contract. Native-aspect-aware centered 16:9 cropping leaves native 16:9 and unknown-aspect signals unchanged. Screen input uses Windows Graphics Capture. Cropping feeds both preview and output.
- The runtime uses synchronous bounded channels and no Tokio.
- The device worker checks Windows session connection state every 250 ms. Inactive sessions release webcam and screen capture, suspend discovery/recovery, and publish the off frame; active sessions reopen selected sources while preserving Start/Stop and output mode. Session-query failures suspend capture until an active state is confirmed.
- Reference detection runs every 250 ms with five-match/three-mismatch debounce.
- Monitor selection is restored by friendly label with secondary-display fallback. Reference discovery requires the same winning display twice, does not pause output, runs at startup, when Settings opens, after reference changes, and every 30 seconds by default, and can be limited to explicit rescans.
- Independent black-or-unavailable recovery samples only the selected capture every 30 seconds. Two consecutive near-black or missing-frame checks restart screen capture; the setting can be disabled without changing reference discovery.
- Still-image PIP is enabled by default and compares exact 160×90 grayscale samples every 250 ms while Auto is showing non-reference content. It supports 30/45/60/120/300-second delays, requires both sources, treats paused video as still content, and never overrides manual modes. PIP can also be forced directly from the output-mode selector.
- Composition uses CPU aspect-fit, black letterboxing, configurable missing-source fallback, a reversible 500 ms blend, and a reversible bottom-left PIP. The inset supports Mini 320×180, Medium 384×216 (default), and Large 448×252 sizes with 16 px margins and 12 px rounded corners.
- The virtual camera prefers RGB32 1280×720 at 30 fps. Selectable NV12 720p is retained for Windows Camera and Zoom compatibility; 1080p is intentionally excluded.
- Output uses deadline-based 30 fps pacing. Visible dashboard previews use latest-only conversion workers and display-sized textures. FPS is runtime-owned and remains meaningful while the dashboard is hidden.
- While automation is stopped, a fixed black screen with the centered StageSwap icon is published at 30 fps. The virtual camera generates the same frame whenever the app publisher is absent.
- The app includes the dashboard, six settings categories, contextual previews, shared executable/window/tray icon, synchronized tray controls, warning and update notifications, exit confirmation, 14-day JSONL logs, and individual/all component restarts.
- Manual updates use the public GitHub Releases API. Stable ignores prereleases; Beta accepts both tracks. Availability is checked once at startup and on demand, while download, verified installation, and restart always require user action.
- There is no general hot-plug manager, monitor-reselection system, sleep/resume recovery, docking recovery, dynamic output-format management, OBS integration, or kernel driver. Compatible webcam media-type changes are revalidated; invalidation, driver failure, and stale capture retry the saved webcam up to three times without rediscovery, while other failures retain the explicit restart path. Screen recovery only retries a selected capture that is black or unavailable, using its stored display identity.

Configuration schema 1, references, and logs live under `%LocalAppData%\StageSwap`. Frames are never recorded or uploaded.

## Repository layout

```text
StageSwap/
├── crates/core/          state machine, frames, detector, and transitions
├── crates/app/           UI, configuration, orchestration, and composition
├── crates/windows/       webcam, screen, deployment, and IPC adapters
├── crates/media-source/  Media Foundation virtual-camera source DLL
├── xtask/                packaging, PE validation, and checksums
├── scripts/              local packaging and release-evidence helpers
└── docs/                 architecture, localization, requirements, and release-evidence references
```

The workspace uses Rust edition 2024 and is pinned to Rust 1.97.1 in `rust-toolchain.toml`. `stageswap-core` and `xtask` are the default members so platform-independent work can be built and tested on non-Windows hosts. Direct Windows APIs, COM, and unavoidable unsafe code belong in `stageswap-windows` and `stageswap-media-source`.

## Development workflow

Before pushing to `main`, run the host gate, which explicitly covers both debug and release tests:

```bash
sh scripts/check-host.sh
```

The gate runs formatting, host Clippy, Windows-target Clippy, the debug workspace suite, and the release workspace suite.

For cross-target linting, run:

```bash
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

### Test design

Create a test when it protects important product functionality, a documented contract, or a failure path that would be costly or difficult to detect manually. Good candidates include:

- stable frame, media, IPC, configuration, localization, update, timing, persistence, and recovery behavior;
- state transitions and user actions that cross component boundaries, especially startup, retries, rollback, bounded queues, and restart behavior; and
- deterministic boundary cases, invariants, migrations, malformed input, limits, and platform-adapter behavior that can regress without an obvious UI symptom.

Before adding a test, check whether an existing test can be extended or consolidated with the same behavior. Prefer one focused contract or flow test that covers complementary assertions over multiple near-duplicates.

UI tests should be limited to representative rendering, containment, accessibility-relevant state, and meaningful interaction transitions. Do not add tests solely for exact coordinates, colors, spacing, icon shape counts, animation geometry, exhaustive locale/DPI matrices, or other implementation details that are better validated through the deterministic UI preview and manual visual review. Avoid broad smoke tests that only prove that many unrelated screens do not panic; keep smoke coverage small and intentional.

Use the category prefixes `contract_`, `flow_`, `smoke_`, and `native_` so tests can be filtered without creating extra test binaries. Native or ignored tests are appropriate when they exercise real Windows, COM, camera, display, tray, or virtual-camera boundaries that cannot be meaningfully validated on macOS; keep those environment-dependent checks separate from the host-visible suite.

### UI preview

Launch the real StageSwap Settings interface with deterministic mock cameras, displays, reference imagery, and healthy runtime states:

```bash
cargo run -p stageswap --bin StageSwap -- --ui-preview
```

General opens by default. To open a particular page:

```bash
cargo run -p stageswap --bin StageSwap -- --ui-preview matching
```

Available page names are `general`, `webcam`, `screen`, `matching`, `updates`, and `diagnostics`. Notification previews are `notifications`, `notifications-empty`, `notifications-critical`, and `notifications-updates`. You can still navigate between every page after launch.

Setup-guide previews are `setup-1` through `setup-5`. Each name opens one deterministic full-window step for the JW Library-to-Zoom workflow.

For the idle-reference step, append `--ui-setup-reference-state captured`, `empty`, `review`, or `missing-screen` to inspect its deterministic saved, initial, candidate-review, and unavailable-display states:

```bash
cargo run -p stageswap --bin StageSwap -- --ui-preview setup-4 --ui-setup-reference-state review
```

The `setup-4` capture, retake, confirmation, and capture-again controls are interactive in preview mode. They update only the in-memory mock frames and never write `reference.png`.

Dialog previews are `dialog-exit`, `dialog-clear-logs`, `dialog-admin`, `dialog-replace-baseline`, `dialog-load-admin-config`, and `dialog-remove-baseline`.

Append `--ui-language en-US`, `--ui-language fr-FR`, or `--ui-language es` to render any preview in a supported language:

```bash
cargo run -p stageswap --bin StageSwap -- --ui-preview matching --ui-language fr-FR
```

Preview mode uses a temporary configuration directory and does not save changes to the normal StageSwap configuration.

## Platform limitations

Development is performed on macOS. Host checks, deterministic UI previews, and cross-compilation are available locally, but macOS cannot validate an interactive Windows desktop, physical webcam capture, virtual-camera enumeration, native Windows dialogs/tray behavior, exact Windows font rendering, or hardware-specific capture behavior. Use a native Windows machine for those checks.

## Packaging and release

### Publish a release

```bash
./scripts/publish-release.sh
```

The interactive publisher defaults to the Development/Beta track, suggests the next available version, requires explicit confirmation for a stable Release, runs the repository checks, cross-compiles the x64 Windows build, validates the PE files and embedded payload, commits and pushes the selected version, and publishes the executable and checksum to GitHub Releases.

The publisher requires a clean branch matching its pushed upstream. Stable releases must come from `main`; Development releases may come from another synchronized branch. Every published `vX.Y.Z` must be newer than all existing StageSwap releases.

### Release and deployment

Releases are unsigned versioned executables with SHA-256 sidecars. The underlying interactive command is `cargo run --quiet --release -p xtask -- publish-release`; the wrapper supplies the pinned macOS cross-compilation environment.

`StageSwap_win64_vX.Y.Z.exe` self-deploys per-user or can run once without installation. `--cleanup` removes startup and the virtual-camera deployment; `--uninstall` also removes the managed app and shortcuts. Both preserve user data, and deployment never modifies Automatic Screen Camera data.

## Technical references

- [Architecture](docs/ARCHITECTURE.md) — runtime pipeline, deployment, source selection, monitoring, and recovery boundaries
- [Requirements traceability](docs/REQUIREMENTS_TRACEABILITY.md) — requirements mapped to implementation and evidence
