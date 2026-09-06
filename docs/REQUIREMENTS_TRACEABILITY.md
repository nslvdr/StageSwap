# Requirements traceability

This index connects product contracts to their primary implementation areas and verification evidence. It complements the narrative [architecture](ARCHITECTURE.md); it is not a substitute for native Windows release validation.

## Video contract and source selection

| Contract | Primary implementation | Verification evidence |
| --- | --- | --- |
| Immutable CPU BGRA frames at 1280×720 and 30 fps | `stageswap-core::Frame`; capture normalization, compositor, and pacers | Invalid-frame, aspect-fit/fill, blend, immutable-storage, and pacing tests |
| Visual comparison every 250 ms with five-match/three-mismatch debounce | Runtime detector; `DebouncedDetector` | Detector threshold/debounce tests and runtime switching flows |
| Optional Auto-only still-image PIP requires exact continuous non-reference samples and both live sources | `StillImageDetector`; runtime PIP policy | Stillness reset/timing, forced-mode, source-loss, and paused-content flows |
| Missing reference or screen falls back to webcam; missing webcam uses the configured placeholder | `decide`; compositor fallback policy | Decision-table and composition contract tests |
| Reversible 500 ms source transition with black letterboxing | `TransitionController`; `Frame::blend`; aspect-fit cache | Transition reversal, blend boundary, and fit tests |
| Rounded bottom-left PIP supports Mini 320×180, Medium 384×216, and Large 448×252 sizes, either main source, and automatic or forced activation without changing output format | `FrameCompositor`; cached direct-sampling PIP plan; PIP mix state | Per-size geometry, rounded-mask, endpoint, midpoint, missing-source, rotating-source performance, immutability, and dual-layout tests |
| Stopped automation publishes the canonical StageSwap off frame at 30 fps | Runtime stopped state; shared off-frame generator | Stopped-output and publisher fallback tests |

## Capture and recovery

| Contract | Primary implementation | Verification evidence |
| --- | --- | --- |
| Saved webcam identity, tiered RGB32/NV12/YUY2/MJPEG negotiation, row-aware capture normalization, optional 16:9 aspect-fill, and 30 fps processing ceiling | `MediaFoundationVideoInput`; rational format ranking, callback pacer, and fit/fill primitives | Rational-ranking, 30/60/59.94/120 admission, decimation-counter, crop-toggle, aspect, stride, conversion, x64-build, and native format checks |
| Compatible media-type changes are revalidated; eligible failure retries the same webcam three times | Capture callback; `WebcamRecovery` | Media-type-change, circuit-breaker, generation, and recovery-state tests |
| Windows Graphics Capture with cursor option, capability/HDR preflight, session-valid readiness, and two transient processing failures before terminal failure | `WindowsGraphicsScreenInput`; per-generation failure streak; `DevicePlatform` | Retained-frame, streak-reset, third-failure, closure, old-generation, SDR/HDR, and native capture checks |
| Four-slot capture pools cannot stall when consumers temporarily retain every slot | `FrameBufferPool`; webcam and screen normalization | Pool-exhaustion fallback, recovery/reuse, pressure-counter, and cross-target checks |
| Reference discovery is bounded, nonblocking, and requires the same winning display twice | `MonitorTracker`; monitor-scan worker | Ranking, confirmation, coalescing, shutdown, and multi-monitor tests |
| Inactive Windows sessions release capture; active sessions reopen selected sources without changing user intent | `current_session_active`; device-worker session polling; runtime suspension | Host flow test for input release, cleared readiness/deadlines, paced off frames, and preserved Start/Stop and mode; Windows cross-target lint (no physical session test) |
| Selected-screen recovery checks only the stored display and restarts after two black/missing samples | Runtime recovery scheduler; screen lifecycle | Interval, threshold, confirmation-reset, retry, and stored-identity tests |

## Runtime, IPC, and virtual camera

| Contract | Primary implementation | Verification evidence |
| --- | --- | --- |
| Deterministic lifecycle, bounded mailbox, ordered actions, coalesced settings, independent shutdown | `RuntimeEngine`; runtime mailbox | Virtual-clock lifecycle, command-flood, ordering, coalescing, and shutdown tests |
| Deadline-based output pacing without drift or catch-up bursts | `FramePacer` | Long-gap and missed-deadline tests |
| Strict 40-byte bounded frame IPC with two-second expiry, latest-frame retention, reconnect-scoped sequence epochs, and per-account pipe identities | `FrameHeader`; `SharedFrameCache`; `FramePublisher`; SID-derived instance scope | Header, validation, invalidation ordering, reconnect reset, account-scope naming, duplicate rejection, publisher-restart, expiry, and slow-consumer IPC tests |
| Rust Media Foundation COM source implements the required source and stream interfaces and synchronously stops its pipe worker during stream shutdown | `stageswap-media-source::com_server` | Repeated-shutdown, worker-join, retained-sample, Windows COM/source-state tests, and PE validation |
| RGB32 and NV12 1280×720 at 30 fps; limited-range BT.601 NV12 metadata; no 1080p | `MediaStream` media types and NV12 sequence cache | Color/grayscale conversion and metadata tests; Media Foundation probe; Windows Camera and Zoom checks |

## Configuration, UI, and diagnostics

| Contract | Primary implementation | Verification evidence |
| --- | --- | --- |
| Schema 1 configuration and atomic, rollback-safe configuration/reference persistence | `ConfigStore`; `persist_rgba_atomic`; `save_config_atomic` | Round-trip, migration, corruption, writable-flush, and rollback tests |
| Hidden admin baseline supports validated manual or startup restore | `AdminProfileStore`; Settings admin window | Profile, gesture, policy, transactional restore, rollback, and invalid-data tests |
| Dashboard, six settings categories, setup, tray controls, notifications, previews, three focused restarts, and restart-all | `stageswap` application and tray modules | Focused `smoke_`, `contract_`, and `flow_` tests; deterministic previews; native review |
| Embedded previews are 240×135/30 fps, enlarged previews are 1280×720/30 fps, and all preview work is suspended in the tray without retaining source frames | Runtime metrics; preview workers; window visibility state | Cadence/profile, non-retention, suspension, mailbox, hidden-state, and UI flow tests |
| Fourteen-day JSONL logs and runtime-applied verbose logging | Diagnostics/logging modules | Retention, serialization, malformed-entry, preference, and activity-stream tests |

## Deployment and updates

| Contract | Primary implementation | Verification evidence |
| --- | --- | --- |
| Native self-deploying Windows x64 executable with content-versioned source DLL and machine-wide, no-downgrade component arbitration | Deployment module; embedded resources; `xtask` | PE/payload validation, architecture checks, component compatibility decisions, first-run, registration, and native installation evidence |
| Managed startup is transactional; run-once mode preserves installed startup state | Deployment startup helpers; General Settings | Registry status, transition, rollback, reconciliation, and native Windows checks |
| Per-user uninstall preserves the shared machine source; explicit administrative cleanup removes machine deployment resources while preserving user data | Deployment cleanup/uninstall paths | Ownership-boundary, argument, filesystem, registry, and native removal checks |
| Stable/Beta GitHub updates are manual, verified, replaceable, and rollback-safe | Update worker; WinHTTP adapter; replacement bootstrap | Release selection, configuration, checksum/digest, staged replacement, readiness, and rollback tests |

## Host-executed evidence and release boundaries

The canonical host gate is `sh scripts/check-host.sh`. It runs formatting, host Clippy, Windows-target Clippy, and both debug and release workspace test suites. The deterministic `contract_` and `flow_` evidence for frames, detection, decisions, transitions, composition, pacing, IPC, configuration, reference persistence, and worker coordination is therefore host-executed evidence.

Host tests and cross-target linting cannot prove physical capture, native UI, COM registration, virtual-camera enumeration, Windows Camera, Zoom compatibility, or deployment behavior. Those contracts remain explicitly Windows-native evidence and require the native Windows checks described in [Development](DEVELOPMENT.md) and the repository release process. A passing host gate is not a substitute for that validation.
