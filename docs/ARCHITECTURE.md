# Architecture

This document explains how StageSwap implements its product contract. For build and release commands, see [Development](DEVELOPMENT.md). For user-visible timing and limits, see the wiki [Technical reference](https://github.com/NatanSlvdr/StageSwap/wiki/Technical-reference).

## System boundaries

StageSwap is a local-only, native Windows 11 x64 application. It visually observes a selected display; it does not integrate with or control JW Library. Frames are never recorded or uploaded.

The workspace keeps platform-independent policy separate from Windows adapters:

| Crate | Responsibility |
| --- | --- |
| `stageswap-core` | Immutable frames, configuration, detection, source decisions, and transitions |
| `stageswap` | Runtime ownership, UI, setup, settings, composition, diagnostics, and update orchestration |
| `stageswap-windows` | Media Foundation webcam input, Windows Graphics Capture, deployment, IPC, and native helpers |
| `stageswap-media-source` | Media Foundation virtual-camera source DLL |
| `stageswap-i18n` | User-interface translations |
| `xtask` | Packaging, PE validation, release checks, checksums, and publishing |

Direct Windows APIs, COM, and unavoidable unsafe code belong in the Windows and media-source crates. The runtime uses synchronous bounded channels and no Tokio.

## Video and decision flow

```text
Webcam (Media Foundation) ─┐
                           ├─ normalize to immutable BGRA 1280×720 ─┐
Screen (Graphics Capture) ─┘                                        │
                                                                    ├─ compose/blend ─ publish at 30 fps
Screen thumbnail ─ compare with reference ─ debounce ─ source choice ┘
```

The internal and published contract is immutable CPU BGRA at 1280×720 and 30 fps.

1. Windows Graphics Capture copies each transient D3D texture into a staging texture and then into a CPU `Frame`.
2. Media Foundation ranks progressive RGB32, NV12, YUY2, and MJPEG webcam modes, comparing rational frame rates without truncation. It prefers RGB32 1280×720 at 30 fps, honors actual row pitch, and decimates higher-rate callbacks before buffer access so normalization never intentionally exceeds 30 fps.
3. Optional webcam cropping uses the native display aspect ratio to center-crop and normalize directly into the fixed 16:9 frame during capture. Native 16:9 and unknown-aspect input uses aspect-fit instead. Preview and output therefore consume the same capture-normalized frame without a second crop pass.
4. Detection derives a 160×90 grayscale image from the screen every 250 ms. Five matches select the webcam; three mismatches select the screen. When enabled, a second detector times exact, consecutive non-reference thumbnails; motion, reference return, near-black input, source loss, or manual mode resets it.
5. Composition uses aspect-fit scaling, black letterboxing, configurable missing-source fallback, and reversible 500 ms source and PIP blends. Automatic or forced PIP uses a rounded bottom-left inset with a 16 px margin, supports Mini 320×180, Medium 384×216, and Large 448×252 sizes, and allows either source as the main view. Per-format scale plans and per-size corner masks are cached; the inset is sampled directly into the pooled final-output buffer after its single background copy.
6. A monotonic deadline pacer publishes at 30 fps without catch-up bursts or accumulated drift.

## Runtime ownership and backpressure

`RuntimeEngine::step(now)` owns timing and state transitions. Production supplies a monotonic clock and Windows adapters; deterministic tests use a virtual clock and scripted component ports.

The runtime mailbox is bounded. It processes at most eight ordered commands per output cycle, coalesces settings and output-mode changes as last-wins values, and gives shutdown an independent signal. Settings supersede older mode commands; device batches preserve discard-before-capture ordering and cancel captures superseded by a discard. The reference worker retains completed results under bounded queue pressure and remains interruptible during shutdown. The named-pipe publisher retains only the newest header and shared pixel buffer, so a slow consumer cannot build a queue or force another full-frame clone.

Lifecycle snapshots distinguish stopped, starting, waiting for first frame, ready, stale, restarting, and failed states. Webcam readiness requires a fresh frame from the current generation. Screen readiness requires a session-valid frame and a fresh device-worker heartbeat, even if the screen pixels have not changed. A stalled worker makes the screen unavailable to composition without expiring static content from a responsive worker. The first two consecutive screen-frame processing failures retain the last valid frame and keep the WGC session alive; a successful frame resets that streak, while the third failure is terminal. Monitor closure remains immediately terminal, and replacement invalidates the relevant generation.

Webcam and screen normalization normally recycle four immutable CPU buffers per capture. Uncropped native 720p webcam rows are copied directly into the output pool, including padded and negative-stride input, without an intermediate full-frame copy. If all four are temporarily retained, capture uses an unpooled one-frame fallback instead of stalling; pressure and true-drop counters remain separate and are surfaced in Diagnostics.

Dashboard rendering does not own the output clock. Embedded previews use dedicated latest-only conversion workers capped at 240×135 and 30 refreshes per second. Only one explicitly enlarged preview may request 1280×720 at 30 refreshes per second. Converted textures retain lightweight frame identities rather than source frames. Closing to the tray suspends and drains all preview work while runtime-owned capture, detection, FPS measurement, composition, and publication continue independently.

## Capture, discovery, and recovery

### Webcam

Configuration stores one Media Foundation symbolic link. Startup and settings changes open that exact device; StageSwap does not continuously enumerate webcams or silently choose a replacement.

Device invalidation, driver failure, and stale capture retry the saved webcam after 500 ms, 1 second, and 2 seconds. Recovery succeeds only after a fresh frame arrives. Privacy denial, contention, unsupported formats, incompatible media-type changes, and exhausted retries wait for explicit user action. Compatible media-type changes are fully revalidated.

### Screen

Monitor descriptors include the GDI display name, friendly label, geometry, and runtime-only monitor handle. Startup restores the first exact friendly-label match, otherwise the first non-primary display, or the sole primary display when no secondary exists.

Reference discovery scans without pausing output. It runs at startup, when Settings opens, after reference changes, every 30 seconds by default, and on explicit rescan. The same highest-scoring display above the threshold must win twice. Disabling automatic scans cancels unfinished automatic results but leaves explicit rescans available.

Selected-screen recovery is independent of discovery. Every 30 seconds it checks only the selected capture. Two consecutive missing or near-black checks restart that capture; any valid non-black check clears the confirmation. Recovery retains the stored display identity and never enumerates or selects a replacement monitor.

Capture sessions are generation-tagged. Each webcam restart creates independent callback state so an in-flight callback from the retired reader cannot publish or clear replacement state. Imported references use the same 720p aspect-fit normalization and grayscale sampling as screen capture, including non-16:9 letterboxing. Windows Graphics Capture capability and the selected display’s advanced-color state are checked before capture. HDR and greater-than-8-bit displays are warn-only: there is no tone mapping, and reference capture and automatic matching remain unavailable until the display is SDR.

## Virtual camera and stopped state

The app publishes frames through a strict bounded local named-pipe protocol. Frame and control pipe names include the owning Windows account SID, so separate user profiles can run concurrently while each account retains only one StageSwap instance across its signed-in sessions. Sequence monotonicity is enforced within each physical pipe connection; a successful reconnect starts a new sequence epoch and clears the previous cached frame. Stream shutdown marks the stream stopped, cancels and joins its synchronous pipe reader, releases cached frames and allocator state, and then shuts down events without waiting for COM destruction. The activation object's `ShutdownObject` hook remains a no-op for Windows Frame Server compatibility. Instance-control shutdown repeats synchronous I/O cancellation until the worker exits, covering operations started after the initial cancellation. Virtual-camera and all-component restarts recreate a missing or failed frame publisher before restarting the camera controller. Source startup announces its stream before queuing `MESourceStarted`.

The Media Foundation source prefers RGB32 1280×720 at 30 fps and exposes NV12 720p for Windows Camera and Zoom compatibility. NV12 uses limited-range BT.601 metadata and caches one conversion per immutable publisher sequence. 1080p is intentionally excluded.

Requested samples are timestamped from the current QPC-correlated Media Foundation clock. When the publisher is disconnected, invalid, or stale, the source generates the canonical black off frame with the centered StageSwap icon. The running app publishes that same frame at 30 fps while automatic switching is stopped.

The device worker checks its own Windows session connection state every 250 ms. When the session is inactive, it releases webcam and selected-screen capture, cancels pending and in-flight reference-monitor scans, and suspends capture recovery. Output continues at 30 fps with the canonical off frame, without changing the user's Start/Stop state or output mode. When the session becomes active again, it reopens the selected sources using the current settings and resumes normal discovery and recovery; source readiness requires new frames. Session-query failures also suspend capture until a successful active-session query. This is connection-state handling, not general sleep/resume or display hot-plug recovery.

## Deployment and updates

`StageSwap_win64_vX.Y.Z.exe` can run once or install itself per user at `%LocalAppData%\Programs\StageSwap\StageSwap.exe`. The managed copy owns the Start Menu and Desktop shortcuts and is the only executable eligible for Windows startup.

The executable embeds the x64 Media Foundation source DLL. First managed or run-once launch validates native architecture and elevates only when machine-wide camera registration must change; the tray application then runs unelevated. Machine-wide source changes are serialized across users. The registered component records its semantic version and supported frame-protocol range: older apps accept a newer compatible source and never downgrade it, while an incompatible newer source requires that user to update. Payloads use content-versioned DLL filenames so a locked older DLL cannot block an update. Unlocked stale copies are removed immediately; locked copies are scheduled for deletion at reboot.

An account-scoped, owner-only control pipe lets duplicate launches show that account's existing dashboard and lets an approved replacement request graceful shutdown without contacting another signed-in user. Replacement stages and verifies the candidate, waits up to ten seconds without force-termination, retains the previous executable until the new instance reports ready, and rolls back startup failure.

The Updates page checks the public GitHub Releases API once at startup and on demand. Stable ignores prereleases; Beta accepts both tracks and exposes an Install a specific version button below the main update action, opening a modal version picker for manual rollback. Its loading, error/retry, empty-list, and selected-version states are independent of the latest-update offer; cancelling preserves that offer. The picker lists installable versions from the latest 100 published releases returned by GitHub, newest first, and disables the running version. Choosing a version does not download it; the user must explicitly select a version and click Install this version. The channel and picker are disabled while a check or installation is in progress. Downloads never start automatically. User-approved candidates are bounded, staged locally, and accepted only when release metadata, the versioned filename, the SHA-256 sidecar, and any GitHub digest agree. Installation reuses the replacement and rollback transaction.

`--uninstall` removes the current user's startup entry, virtual-camera registration, managed executable, and shortcuts without removing the shared machine source used by other profiles. The explicit administrative `--cleanup` path additionally removes the machine-wide source registration and payloads. Both preserve user data and never modify Automatic Screen Camera resources.

## Persistence, diagnostics, and administration

Configuration schema 1, `reference.png`, update-notification state, and JSONL logs live below `%LocalAppData%\StageSwap`. Configuration and reference replacement use validation, backup, writable flushes, and atomic replacement. Only a valid primary configuration can replace the configuration backup, preserving recovery after a failed save of settings loaded from that backup. Logs retain errors, warnings, recovery outcomes, and infrequent lifecycle states for 14 days by default. Verbose logging adds recurring telemetry only for new activity.

Critical runtime and configuration notifications are structured at the runtime boundary, delivered to a session-only in-app notification center, and shown as brief in-app toasts. They are not persisted or sent through native Windows notifications. Update availability remains separately deduplicated per channel in `update-state.json` and is presented as an informational in-app notification when enabled.

Schema 1 stores still-image PIP enablement, one of five validated delays, the selected main view, and the inset size. Missing fields retain the enabled, 45-second, webcam-main, Medium defaults.

A concealed per-user admin profile can store a validated configuration, optional immutable reference snapshot, and an independently controlled auto-restore policy. Startup validates the complete baseline before replacement and rolls back the reference if configuration replacement fails. Invalid admin data fails open with a warning and leaves the working configuration unchanged.

## Deliberate exclusions

StageSwap does not implement:

- continuous webcam discovery or replacement-device selection;
- general display hot-plug discovery or automatic monitor reselection;
- sleep/resume or docking recovery;
- D3D device recreation;
- HDR tone mapping;
- automatic camera-contention resolution;
- persisted physical-monitor identity;
- dynamic output formats, 1080p, audio, OBS integration, or a kernel driver.

These exclusions are product boundaries, not undocumented fallback behavior. Webcam and screen recovery retry only their stored identities.

The deterministic version-picker preview is available with `cargo run -p stageswap --bin StageSwap -- --ui-preview updates-versions --ui-language fr-FR`. It uses mock releases and never downloads or launches an installer.
