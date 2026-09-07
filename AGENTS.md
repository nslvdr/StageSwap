# StageSwap contributor guide

## Working rules

- Keep changes simple; apply YAGNI and reduce complexity where practical.
- Prefer inferred types where useful; avoid `any`.
- Answer questions without editing files unless the user requests implementation.
- Be careful with destructive actions the user has not requested.
- Use concise comments to explain functionality and usage; keep them synchronized with code.
- Propose larger changes when they would meaningfully improve the result.

## Essential constraints

- StageSwap is a local-only Windows 11 x64 virtual camera. It observes a display visually without integrating with or controlling JW Library. Never record or upload frames.
- Frames are immutable CPU BGRA at 1280×720, with output fixed at 30 fps.
- Use synchronous bounded channels, not Tokio.
- Keep direct Windows APIs, COM, and unavoidable unsafe code in `crates/windows` and `crates/media-source`.
- Recovery retries saved devices. Do not silently add replacement-device selection, general hot-plug handling, or sleep/resume recovery.

Read the relevant [architecture section](docs/ARCHITECTURE.md) before changing runtime behavior. It owns detailed timing, capture, recovery, deployment, and persistence contracts.

## Validation and commands

Use focused tests for important contracts, cross-component behavior, and costly failure paths. Extend existing coverage when possible. Avoid tests for exact UI geometry, exhaustive layout variants, or intentionally deleted features. Follow the [test guidance](docs/DEVELOPMENT.md#test-design).

Before pushing to `main`, run the host gate (formatting, host and Windows Clippy, debug and release tests):

```bash
sh scripts/check-host.sh
```

For visual review:

```bash
cargo run -p stageswap --bin StageSwap -- --ui-preview
```

Preview options, individual checks, and native Windows validation are in [Development](docs/DEVELOPMENT.md). macOS previews and cross-target checks do not validate native capture, COM, deployment, or Windows UI behavior.

For an authorized release, use `./scripts/publish-release.sh`. It commits, pushes, and publishes; read the [release prerequisites](docs/DEVELOPMENT.md#packaging-and-releases) first.

## Documentation map

| Document | Owns |
| --- | --- |
| [README](README.md) | Product overview, quick start, and user-guide links |
| [Development](docs/DEVELOPMENT.md) | Workspace map, toolchain, checks, previews, and release procedure |
| [Architecture](docs/ARCHITECTURE.md) | Implementation behavior and product boundaries |
| [Requirements traceability](docs/REQUIREMENTS_TRACEABILITY.md) | Contracts mapped to implementation and verification coverage |
| [Localization](docs/LOCALIZATION.md) | Approved terminology and translation rules |

Keep detailed facts in their owning document and link to them elsewhere. Update the relevant contract and verification mapping when behavior changes.
