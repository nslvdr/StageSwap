# Development

This guide covers local engineering, deterministic UI review, Windows validation, packaging, and releases. Product usage belongs in the [StageSwap wiki](https://github.com/NatanSlvdr/StageSwap/wiki); agent-specific operating guidance remains in [`AGENTS.md`](../AGENTS.md).

## Prerequisites and workspace

Use the pinned toolchain in [`rust-toolchain.toml`](../rust-toolchain.toml), which includes `rustfmt`, Clippy, and the `x86_64-pc-windows-msvc` target. The workspace uses Rust edition 2024.

| Path | Purpose |
| --- | --- |
| `crates/core` | Platform-independent frames, configuration, detection, and transitions |
| `crates/app` | Application runtime, UI, orchestration, composition, and updates |
| `crates/i18n` | English, French, and Spanish UI strings |
| `crates/windows` | Windows capture, deployment, IPC, and native adapters |
| `crates/media-source` | Media Foundation virtual-camera source DLL |
| `xtask` | Packaging, PE validation, checksums, and release publishing |
| `scripts` | Local packaging and release-evidence helpers |

The default workspace members are `stageswap-core` and `xtask`, allowing useful host development on non-Windows systems.

## Local checks

Before pushing to `main`, run the host gate:

```bash
sh scripts/check-host.sh
```

This runs formatting, host Clippy, Windows-target Clippy, the debug workspace suite, and the release workspace suite. Run an individual command directly when iterating on one class of failure.

Cross-target linting catches Windows-only compilation issues available to the host toolchain:

```bash
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

## Test design

Tests use the prefixes `contract_`, `flow_`, `smoke_`, and `native_`. Add coverage for stable contracts, cross-component transitions, deterministic boundaries, migrations, malformed input, and costly failure paths. Prefer extending a focused existing test over adding near-duplicate coverage.

Keep environment-dependent Windows, COM, camera, display, tray, and virtual-camera checks separate from the host-visible suite. UI tests should protect meaningful state, containment, accessibility-relevant behavior, and representative interactions—not exact pixels or exhaustive layout variants.

## Deterministic UI previews

Launch the real Settings UI with mock devices, displays, imagery, and healthy runtime state:

```bash
cargo run -p stageswap --bin StageSwap -- --ui-preview
```

General opens by default. Append a preview name to open another state:

| Preview | Names / options |
| --- | --- |
| Settings | `general`, `webcam`, `screen`, `matching`, `updates`, `diagnostics` |
| Version picker | `updates-versions` (mock releases; no download or installer) |
| Notifications | `notifications`, `notifications-empty`, `notifications-critical`, `notifications-updates` |
| Guided setup | `setup-1` through `setup-5` |
| Reference state on `setup-4` | `--ui-setup-reference-state captured`, `empty`, `review`, or `missing-screen` |
| Dialogs | `dialog-exit`, `dialog-clear-logs`, `dialog-admin`, `dialog-replace-baseline`, `dialog-load-admin-config`, `dialog-remove-baseline` |
| Language on any preview | `--ui-language en-US`, `fr-FR`, or `es` |

For example:

```bash
cargo run -p stageswap --bin StageSwap -- --ui-preview setup-4 --ui-setup-reference-state review --ui-language fr-FR
```

The `notifications` preview starts with two critical alerts and an update, then adds an informational notification every 30 seconds of activity. The other notification previews remain fixed.

Preview mode uses temporary configuration and never writes the normal `reference.png`. The setup reference controls modify only in-memory mock frames.

## Platform validation

macOS supports host checks, deterministic UI previews, and cross-compilation. It cannot validate:

- an interactive Windows desktop or exact Windows font rendering;
- physical webcam or Windows Graphics Capture behavior;
- HDR detection and hardware-specific media-type negotiation;
- virtual-camera enumeration in Windows Camera and Zoom;
- native dialogs, tray behavior, registry startup, elevation, replacement, cleanup, or uninstall.

Use a native Windows 11 x64 machine for those checks. Release evidence should identify which results are host, cross-target, deterministic-preview, or native.

## Packaging and releases

The supported interactive release entry point is:

```bash
./scripts/publish-release.sh
```

The wrapper supplies the pinned macOS cross-compilation environment and invokes `cargo run --quiet --release -p xtask -- publish-release`. It checks the repository, builds Windows x64 artifacts, validates PE files and embedded payloads, commits and pushes the selected version, and publishes the executable with its SHA-256 sidecar.

The publisher defaults to the Development/Beta track. Stable releases require explicit confirmation and must come from `main`; development releases may come from another synchronized branch. The branch must be clean and match its pushed upstream. Every `vX.Y.Z` must be newer than all existing StageSwap releases.

Never use the publisher merely to test a documentation or packaging change: it commits, pushes, and creates a GitHub release.

## Contribution and reporting policy

See the [README](../README.md#development) for contribution policy. Report reproducible problems through [GitHub issues](https://github.com/NatanSlvdr/StageSwap/issues) with the StageSwap and Windows versions, selected device types, reproduction steps, expected and actual behavior, and reviewed diagnostic logs when relevant. Do not attach camera or display frames without checking them for private content.
