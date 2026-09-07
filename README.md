<p align="center">
  <img src="crates/app/assets/app-icon.png" width="120" alt="StageSwap app icon">
</p>

<h1 align="center">StageSwap</h1>

<p align="center">
  A local Windows virtual camera that automatically shows either your webcam or the secondary screen used for JW Library presentations in Zoom.
</p>

<p align="center">
  <a href="https://github.com/NatanSlvdr/StageSwap/releases/latest"><strong>Download StageSwap</strong></a>
  ·
  <a href="https://github.com/NatanSlvdr/StageSwap/wiki">User guide</a>
  ·
  <a href="https://github.com/NatanSlvdr/StageSwap/wiki/Troubleshooting">Troubleshooting</a>
</p>

![StageSwap dashboard showing webcam, secondary-screen, reference-image, and Zoom-output previews](https://raw.githubusercontent.com/wiki/NatanSlvdr/StageSwap/assets/dashboard-auto-camera.png)

## What StageSwap does

StageSwap gives Zoom one camera to select: **Stageswap Camera**. In the normal **Auto** mode, it compares the selected secondary screen with a saved picture of the screen JW Library shows when no media is playing.

- When the screen matches the reference image, Zoom receives the webcam.
- When the screen changes enough to indicate media, Zoom receives the secondary screen.
- When presentation content stays unchanged, StageSwap can show it as an inset over the webcam. Picture-in-picture is configurable and can also be selected manually.
- When automatic switching is stopped, Zoom receives a fixed StageSwap off screen.

Transitions are blended, and several consistent comparisons are required before the source changes. The published video is fixed at 1280×720 and 30 fps.

> [!IMPORTANT]
> StageSwap is an independent, unofficial project. It is not affiliated with or endorsed by the publisher of JW Library. The name JW Library is used only to describe compatibility.

## Privacy by design

StageSwap watches the selected screen visually. Webcam and screen frames stay on the computer: they are not recorded or uploaded. StageSwap does not connect to or control JW Library, read its text or media metadata, transmit audio, or start Zoom screen sharing.

Anything visible on the selected secondary screen can appear in Zoom, including in picture-in-picture. Always check the **Zoom output** preview before a meeting.

## Requirements

- 64-bit Windows 11
- A webcam
- A secondary screen used for JW Library presentations
- Zoom with permission to use a camera

Current releases are unsigned. Download only from the [official releases page](https://github.com/NatanSlvdr/StageSwap/releases/latest).

## Get started

1. Download `StageSwap_win64_vX.Y.Z.exe` from the official releases page.
2. Open it and choose **Install StageSwap**. Use **Run once** only for a temporary trial.
3. Complete the five-step guided setup: choose the webcam and secondary screen, then capture the normal JW Library idle view as the reference image.
4. In Zoom, select **Stageswap Camera**.
5. In StageSwap, choose **Auto**, select **Start automatic switching**, and verify **Zoom output**.

Windows may request administrator approval when the virtual camera must be installed or updated. Normal managed launches run without administrator access.

For screenshots, detailed instructions, and a setup success checklist, follow the [complete setup guide](https://github.com/NatanSlvdr/StageSwap/wiki/Setup).

## Documentation

| I want to… | Read… |
| --- | --- |
| Install and configure StageSwap | [Setup](https://github.com/NatanSlvdr/StageSwap/wiki/Setup) |
| Prepare for and run a meeting | [Meetings](https://github.com/NatanSlvdr/StageSwap/wiki/Meetings) |
| Understand previews and controls | [Dashboard](https://github.com/NatanSlvdr/StageSwap/wiki/Dashboard) |
| Change devices or preferences | [Settings](https://github.com/NatanSlvdr/StageSwap/wiki/Settings) |
| Fix a problem | [Troubleshooting](https://github.com/NatanSlvdr/StageSwap/wiki/Troubleshooting) |
| Update, clean up, or uninstall | [Updates](https://github.com/NatanSlvdr/StageSwap/wiki/Updates) |
| Understand local data and privacy | [Privacy](https://github.com/NatanSlvdr/StageSwap/wiki/Privacy) |
| Check exact behavior and limits | [Technical reference](https://github.com/NatanSlvdr/StageSwap/wiki/Technical-reference) |

## Deliberate limits

StageSwap provides video only, at 720p, with no OBS integration or HDR tone mapping. Recovery retries saved devices; it does not automatically choose replacements or handle docking and sleep/resume. See the [technical reference](https://github.com/NatanSlvdr/StageSwap/wiki/Technical-reference) for detailed limits.

## Development

StageSwap is written in Rust. Start with the developer guide for the pinned toolchain and validation requirements.

- [Developer guide](docs/DEVELOPMENT.md) — workspace, commands, previews, platform boundaries, and releases
- [Architecture](docs/ARCHITECTURE.md) — runtime, capture, composition, publishing, recovery, and deployment
- [Requirements traceability](docs/REQUIREMENTS_TRACEABILITY.md) — product contracts mapped to implementation and evidence
- [Localization](docs/LOCALIZATION.md) — approved English, French, and Spanish terminology

The project is source-available under the license in `Cargo.toml`. Pull requests are not currently accepted; use [issues](https://github.com/NatanSlvdr/StageSwap/issues) for reproducible bug reports.
