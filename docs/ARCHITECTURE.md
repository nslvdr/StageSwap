# Architecture

This document explains how StageSwap implements its product contract. For build and release commands, see [Development](DEVELOPMENT.md). This is the repository reference for implementation behavior and limits; the [user wiki](https://github.com/nslvdr/StageSwap/wiki) explains how to operate the app.

## System boundaries

StageSwap is a local-only, native Windows 11 x64 application. It visually observes a selected display; it does not integrate with or control JW Library. Frames are never recorded or uploaded.

Platform-independent policy lives in the core and app crates. Direct Windows APIs, COM, and unavoidable unsafe code belong in the Windows and media-source crates. The runtime uses synchronous bounded channels and no Tokio. See the [workspace map](DEVELOPMENT.md#prerequisites-and-workspace) for crate responsibilities.

## Video and decision flow

```text
Webcam (Media Foundation) ─┐
                           ├─ normalize to immutable BGRA 1280×720 ─┐
Screen (Graphics Capture) ─┘                                        │
                                                                    ├─ compose/blend ─ publish at 30 fps
Screen thumbnail ─ compare with reference ─ debounce ─ source choice ┘
```

The internal and published contract is immutable CPU BGRA at 1280×720 and 30 fps.

1. Windows Graphics Capture reuses one staging texture per capture geometry/format. It reads mapped pitched rows directly into the normalized CPU `Frame`, without a full-resolution scratch copy, and unmaps on every exit path. Aspect-fit normalization initializes only letterbox pixels before sampling the image area.
2. Media Foundation ranks progressive RGB32, NV12, YUY2, and MJPEG webcam modes, comparing rational frame rates without truncation. It prefers RGB32 1280×720 at 30 fps, honors actual row pitch, and decimates higher-rate callbacks before buffer access so normalization never intentionally exceeds 30 fps.
3. Optional webcam cropping uses the native display aspect ratio to center-crop and normalize directly into the fixed 16:9 frame during capture. Native 16:9 and unknown-aspect input uses aspect-fit instead. Preview and output therefore consume the same capture-normalized frame without a second crop pass.
4. Detection derives a 160×90 grayscale image from the screen every 250 ms. Five matches select the webcam; three mismatches select the screen. Still-image PIP is enabled by default: in Auto, it requires both sources and exact consecutive non-reference thumbnails for the configured delay (30/45/60/120/300 seconds; default 45). Paused video counts as still content. Motion, reference return, near-black input, source loss, or manual mode resets the detector.
5. Composition uses aspect-fit scaling, black letterboxing, configurable missing-source fallback, and reversible 500 ms source and PIP blends. Automatic or forced PIP uses a rounded bottom-left inset with a 16 px margin and 12 px corner radius, supports Mini 320×180, Medium 384×216 (default), and Large 448×252 sizes, and allows either source as the main view. Per-format scale plans and per-size corner masks are cached; the inset is sampled directly into the pooled final-output buffer after its single background copy.
6. A monotonic deadline pacer publishes at 30 fps without catch-up bursts or accumulated drift.

## Runtime ownership and backpressure

`RuntimeEngine::step(now)` owns timing and state transitions. Production supplies a monotonic clock and Windows adapters; deterministic tests use a virtual clock and scripted component ports.

The runtime mailbox is bounded. It processes at most eight ordered commands per output cycle, coalesces settings and output-mode changes as last-wins values, and gives shutdown an independent signal. Settings supersede older mode commands; device batches preserve discard-before-capture ordering and cancel captures superseded by a discard. The reference worker retains completed results under bounded queue pressure and remains interruptible during shutdown. Discovery-worker shutdown changes its stop predicate under the condition-variable mutex so an idle worker cannot miss the wake-up.

The named-pipe publisher retains only the newest header and shared pixel buffer, so a slow consumer cannot build a queue or force another full-frame clone.

Lifecycle snapshots distinguish stopped, starting, waiting for first frame, ready, stale, restarting, and failed states. Webcam readiness requires a fresh frame from the current generation. Screen readiness requires a session-valid frame and a fresh device-worker heartbeat, even if the screen pixels have not changed. A stalled worker makes the screen unavailable to composition without expiring static content from a responsive worker.

The first two consecutive screen-frame processing failures retain the last valid frame and keep the WGC session alive; a successful frame resets that streak, while the third failure is terminal. Monitor closure remains immediately terminal, and replacement invalidates the relevant generation.

Webcam and screen normalization normally recycle four immutable CPU buffers per capture. Uncropped native 720p webcam rows are copied directly into the output pool, including padded and negative-stride input, without an intermediate full-frame copy. If all four are temporarily retained, capture uses an unpooled one-frame fallback instead of stalling; pressure and true-drop counters remain separate and are surfaced in Diagnostics.

Dashboard rendering does not own the output clock. Embedded previews use dedicated latest-only conversion workers capped at 240×135 and 30 refreshes per second. Only one explicitly enlarged preview may request 1280×720 at 30 refreshes per second. Converted textures retain lightweight frame identities rather than source frames. Closing to the tray suspends and drains all preview work while runtime-owned capture, detection, FPS measurement, composition, and publication continue independently.

## Capture, discovery, and recovery

Capture adapters return typed failure categories alongside diagnostic messages. Recovery decisions use those categories and native status codes, never message wording; the runtime owns the retry policy.

### Webcam

Configuration stores one Media Foundation symbolic link. Startup and settings changes open that exact device; StageSwap does not continuously enumerate webcams or silently choose a replacement.

Device invalidation, driver failure, and stale capture retry the saved webcam after 500 ms, 1 second, and 2 seconds. Recovery succeeds only after a fresh frame arrives. Privacy denial, contention, unsupported formats, incompatible media-type changes, and exhausted retries wait for explicit user action. Compatible media-type changes are fully revalidated.

### Screen

Monitor descriptors include the GDI display name, friendly label, geometry, and runtime-only monitor handle. Startup restores the first exact friendly-label match, otherwise the first non-primary display, or the sole primary display when no secondary exists.

Reference discovery scans without pausing output. It runs at startup, when Settings opens, after reference changes, every 30 seconds by default, and on explicit rescan. Only displays verified as SDR/8-bit participate in reference scoring; unsupported displays and failed color-capability queries are excluded. The same highest-scoring eligible display above the threshold must win twice. Disabling automatic scans cancels unfinished automatic results but leaves explicit rescans available.

Selected-screen recovery is independent of discovery. Every 30 seconds it checks only the selected capture. Two consecutive missing or near-black checks restart that capture; any valid non-black check clears the confirmation. Recovery retains the stored display identity and never enumerates or selects a replacement monitor.

Capture sessions are generation-tagged. Each webcam restart creates independent callback state so an in-flight callback from the retired reader cannot publish or clear replacement state. Imported references use the same 720p aspect-fit normalization and grayscale sampling as screen capture, including non-16:9 letterboxing. Windows Graphics Capture capability and the selected display’s advanced-color state are checked before capture. HDR and greater-than-8-bit displays are warn-only: there is no tone mapping, and reference capture and automatic matching remain unavailable until the display is SDR.

## Virtual camera and stopped state

The app publishes frames through a bounded local named-pipe protocol with a strict 40-byte header and two-second frame expiry. Frame and control pipe names include the owning Windows account SID, so separate user profiles can run concurrently while each account retains only one StageSwap instance across its signed-in sessions. Sequence monotonicity is enforced within each physical pipe connection; a successful reconnect starts a new sequence epoch and clears the previous cached frame.

Stream shutdown marks the stream stopped, cancels and joins its synchronous pipe reader, releases cached frames and allocator state, and then shuts down events without waiting for COM destruction. The activation object's `ShutdownObject` hook remains a no-op for Windows Frame Server compatibility. Instance-control shutdown repeats synchronous I/O cancellation until the worker exits, covering operations started after the initial cancellation.

Virtual-camera and all-component restarts recreate a missing or failed frame publisher before restarting the camera controller. Source startup announces its stream before queuing `MESourceStarted`.

The Media Foundation source prefers RGB32 1280×720 at 30 fps and exposes NV12 720p for Windows Camera and Zoom compatibility. NV12 uses limited-range BT.601 metadata and caches one conversion per immutable received frame object, so reused sequence numbers after reconnect cannot reuse old pixels. Conversion uses a bounded four-slot buffer pool with a temporary fallback under pressure; the cache and pool are released at stream shutdown. 1080p is intentionally excluded.

Requested samples are timestamped from the current QPC-correlated Media Foundation clock. When the publisher is disconnected, invalid, or stale, the source generates the canonical black off frame with the centered StageSwap icon. The running app publishes that same frame at 30 fps while automatic switching is stopped.

The device worker checks its own Windows session connection state every 250 ms. When the session is inactive, it releases webcam and selected-screen capture, cancels pending and in-flight reference-monitor scans, and suspends capture recovery. Output continues at 30 fps with the canonical off frame, without changing the user's Start/Stop state or output mode. When the session becomes active again, it reopens the selected sources using the current settings and resumes normal discovery and recovery; source readiness requires new frames. Session-query failures also suspend capture until a successful active-session query. This is connection-state handling, not general sleep/resume or display hot-plug recovery.

## Deployment and updates

`StageSwap_win64_vX.Y.Z.exe` can run once or install itself per user at `%LocalAppData%\Programs\StageSwap\StageSwap.exe`. The managed copy owns the Start Menu and Desktop shortcuts and is the only executable eligible for Windows startup.

The executable embeds the x64 Media Foundation source DLL. First managed or run-once launch validates native architecture and elevates only when machine-wide camera registration must change; the tray application then runs unelevated. Machine-wide source changes are serialized across users. The registered component records its semantic version and supported frame-protocol range: older apps accept a newer compatible source and never downgrade it, while an incompatible newer source requires that user to update. Payloads use content-versioned DLL filenames so a locked older DLL cannot block an update. Unlocked stale copies are removed immediately; locked copies are scheduled for deletion at reboot.

An account-scoped, owner-only control pipe lets duplicate launches show that account's existing dashboard and lets an approved replacement request graceful shutdown without contacting another signed-in user. Replacement stages and verifies the candidate, waits up to ten seconds without force-termination, retains the previous executable until the new instance reports ready, and rolls back startup failure.

Updates use the public GitHub Releases API, checked once at startup and on demand:

- Stable ignores prereleases; Beta accepts both tracks.
- Beta also offers a manual version picker for the latest 100 published releases, newest first, with the running version disabled. Its loading, retry, empty, and selection states are independent of the latest-update offer; cancellation preserves that offer.
- Selecting a version does not download it. The user must click **Install this version**. Channel and picker controls are disabled during checks or installation; downloads never start automatically.
- Candidates are size-bounded, staged locally, and accepted only when release metadata, the versioned filename, SHA-256 sidecar, and any GitHub digest agree. Installation reuses the replacement and rollback transaction.

`--uninstall` removes the current user's startup entry, virtual-camera registration, managed executable, and shortcuts without removing the shared machine source used by other profiles. The explicit administrative `--cleanup` path additionally removes the machine-wide source registration and payloads. Both preserve user data and never modify Automatic Screen Camera resources.

## Persistence, diagnostics, and administration

Configuration schema 1, `reference.png`, update-notification state, and JSONL logs live below `%LocalAppData%\StageSwap`. Configuration and reference replacement use validation, backup, writable flushes, and atomic replacement. Only a valid primary configuration can replace the configuration backup, preserving recovery after a failed save of settings loaded from that backup. Routine settings saves run on a dedicated writer with one last-wins pending configuration and one completion slot; UI refreshes never wait for its disk I/O. Explicit admin restore first drains older settings writes (up to two seconds, aborting restore on timeout), and exit drains the final settings within the same bounded timeout. A timed-out writer is detached without starting a competing synchronous write. Reference cancellation and final replacement compete for an atomic commit claim. Cancellation before the claim leaves the saved reference untouched. Once replacement is claimed, successful completion for the current reference path updates the runtime even if the review candidate has since been discarded or superseded, keeping disk and memory consistent.

Logs retain errors, warnings, recovery outcomes, and infrequent lifecycle states for 14 days by default. Verbose logging adds recurring telemetry only for new activity.

Critical runtime and configuration notifications are structured at the runtime boundary, delivered to a session-only in-app notification center, and shown as brief in-app toasts. They are not persisted or sent through native Windows notifications. Update availability remains separately deduplicated per channel in `update-state.json` and is presented as an informational in-app notification when enabled.

Missing PIP configuration fields retain the defaults described in [Video and decision flow](#video-and-decision-flow), with the webcam as the main view.

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
