//! Deterministic output engine and public runtime handle.

#[cfg(any(windows, test))]
mod device;
mod diagnostics;
mod disco;
#[cfg(any(windows, test))]
mod discovery;
mod fps;
mod recovery;
mod reference;
mod warnings;

use crate::runtime_mailbox::{CommandInbox, CommandMailbox};
use crate::{CommandDispatch, RuntimeClock, SystemRuntimeClock};
#[cfg(any(windows, test))]
use device::*;
use diagnostics::*;
use disco::*;
#[cfg(any(windows, test))]
use discovery::*;
use fps::*;
use recovery::*;
use reference::*;
use stageswap_core::{
    AppConfig, AppSnapshot, CAPTURE_FRAME_POOL_CAPACITY, Command, DebouncedDetector,
    DetectorSettings, DeviceState, Frame, FrameBufferPool, FrameCompositor, FrameMetadata,
    FramePacer, GrayImage, OutputLayout, OutputMode, PIPELINE_FPS, PIPELINE_SIZE, PipComposition,
    RunState, RuntimeAlert, RuntimeAlertSource, RuntimeWarning, Size, Source, SourceAvailability,
    StillImageDetector, StillImagePipLayout, StillImagePipSize, TransitionController,
    aspect_fit_bgra_into, decide, image_similarity, off_frame, resize_bgra_to_gray,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use warnings::*;

#[cfg(any(windows, test))]
use stageswap_core::{MonitorDescriptor, RestartTarget};
#[cfg(any(windows, test))]
use std::collections::HashSet;
#[cfg(any(windows, test))]
use std::sync::{Condvar, Mutex};

const COMMAND_CAPACITY: usize = 32;
const MAX_COMMANDS_PER_OUTPUT_CYCLE: usize = 8;
const ACTIVITY_LIMIT: usize = 20;
const ALERT_LIMIT: usize = 20;
const REFERENCE_JOB_CAPACITY: usize = 4;
const REFERENCE_MAX_DIMENSION: u32 = 8192;
const REFERENCE_MAX_ALLOCATION: u64 = 256 * 1024 * 1024;
const REFERENCE_PREVIEW_SIZE: Size = Size::new(1280, 720);
const WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

fn finish_worker_shutdown(
    done: &Receiver<()>,
    worker: &mut Option<JoinHandle<()>>,
    timeout: Duration,
) -> bool {
    let completed = done.recv_timeout(timeout).is_ok();
    if completed {
        if let Some(worker) = worker.take() {
            let _ = worker.join();
        }
    } else {
        // Dropping JoinHandle deliberately detaches a worker stuck in an OS
        // driver, filesystem, or decoder call. Callers use this only on exit.
        worker.take();
    }
    completed
}
const FPS_TRACKING_WINDOW: Duration = Duration::from_secs(1);
#[cfg(any(windows, test))]
use stageswap_core::ComponentLifecycle;
#[cfg(any(windows, test))]
use stageswap_core::FRAME_STALE_AFTER;
#[cfg(any(windows, test))]
use stageswap_core::WebcamFailureKind;
#[cfg(windows)]
use stageswap_core::{ComponentFailureKind, ScreenFailureKind};
#[cfg(any(windows, test))]
const WEBCAM_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(any(windows, test))]
const SCREEN_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(any(windows, test))]
const SCREEN_RESTART_BACKOFF_BASE: Duration = Duration::from_secs(5);
#[cfg(any(windows, test))]
const SCREEN_RESTART_BACKOFF_MAX: Duration = Duration::from_secs(60);
#[cfg(any(windows, test))]
const WEBCAM_RECOVERY_DELAYS: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];
const BLACK_LUMA_THRESHOLD: u8 = 16;
const BLACK_PIXEL_PERCENT: usize = 99;
#[cfg(any(windows, test))]
const SCREEN_CAPTURE_FAILURE_CONFIRMATIONS: u8 = 2;
#[cfg(any(windows, test))]
const AUTOMATIC_SCREEN_CHECK_INTERVAL: Duration = Duration::from_secs(30);

pub struct RuntimeHandle {
    commands: CommandMailbox,
    snapshot: Arc<RwLock<AppSnapshot>>,
    worker: Option<JoinHandle<()>>,
}

impl RuntimeHandle {
    pub fn spawn(config: AppConfig) -> Self {
        Self::spawn_with_clock(config, SystemRuntimeClock)
    }

    pub fn spawn_with_clock<C: RuntimeClock>(config: AppConfig, clock: C) -> Self {
        let (commands, receiver) = CommandMailbox::bounded(COMMAND_CAPACITY);
        let now = clock.now();
        let mut transition = TransitionController::default();
        let mut initial_snapshot = AppSnapshot {
            mode: config.output_mode,
            ..AppSnapshot::default()
        };
        initial_snapshot.actual_output = Source::Placeholder;
        initial_snapshot.automatic_target = Source::Placeholder;
        initial_snapshot.output_layout = OutputLayout::Placeholder;
        initial_snapshot.transition = transition.request(Source::Placeholder, now);
        initial_snapshot.previews.final_output = Some(Arc::new(off_frame(FrameMetadata {
            sequence: 1,
            timestamp_100ns: 0,
            received_at: now,
        })));
        let snapshot = Arc::new(RwLock::new(initial_snapshot));
        let worker_snapshot = Arc::clone(&snapshot);
        let worker = thread::Builder::new()
            .name("stageswap-runtime".into())
            .spawn(move || run(config, receiver, worker_snapshot, clock))
            .expect("runtime thread can be created");
        Self {
            commands,
            snapshot,
            worker: Some(worker),
        }
    }

    pub fn send(&self, command: Command) -> CommandDispatch {
        self.commands.dispatch(command)
    }

    pub fn try_send(&self, command: Command) -> CommandDispatch {
        self.commands.dispatch(command)
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.snapshot
            .read()
            .expect("runtime snapshot lock is not poisoned")
            .clone()
    }
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        self.commands.request_shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Debug)]
struct ReversibleMix {
    duration: Duration,
    mix: f64,
    target: f64,
    last_update: Option<Instant>,
}

impl ReversibleMix {
    fn new(duration: Duration) -> Self {
        Self {
            duration,
            mix: 0.0,
            target: 0.0,
            last_update: None,
        }
    }

    fn advance(&mut self, now: Instant) {
        let Some(previous) = self.last_update.replace(now) else {
            return;
        };
        let delta =
            now.saturating_duration_since(previous).as_secs_f64() / self.duration.as_secs_f64();
        if self.target > self.mix {
            self.mix = (self.mix + delta).min(self.target);
        } else {
            self.mix = (self.mix - delta).max(self.target);
        }
    }

    fn request(&mut self, active: bool, now: Instant) -> f64 {
        self.advance(now);
        self.target = if active { 1.0 } else { 0.0 };
        self.mix
    }

    fn reset(&mut self, now: Instant) {
        self.mix = 0.0;
        self.target = 0.0;
        self.last_update = Some(now);
    }
}

struct RuntimeState {
    capture_suspended: bool,
    config: AppConfig,
    snapshot: AppSnapshot,
    transition: TransitionController,
    compositor: FrameCompositor,
    disco: Option<DiscoEffect>,
    webcam_fps: SourceFpsTracker,
    screen_fps: SourceFpsTracker,
    output_fps: OutputFpsTracker,
    warnings: WarningRegistry,
    alerts: VecDeque<RuntimeAlert>,
    next_alert_id: u64,
    activity: VecDeque<String>,
    next_activity_id: u64,
    sequence: u64,
    started_at: Instant,
    reference: Option<GrayImage>,
    detector: DebouncedDetector,
    still_image_detector: StillImageDetector,
    pip_transition: ReversibleMix,
    pip_render_layout: StillImagePipLayout,
    pip_render_size: StillImagePipSize,
    last_detection: Instant,
    #[cfg(windows)]
    pending_reference_capture: Option<Arc<Frame>>,
    #[cfg(any(windows, test))]
    publisher_diagnostics: PublisherDiagnosticsState,
}

impl RuntimeState {
    #[cfg(test)]
    fn new(config: AppConfig) -> Self {
        Self::new_at(config, Instant::now())
    }

    fn new_at(config: AppConfig, now: Instant) -> Self {
        let mode = config.output_mode;
        let pip_render_layout = config.still_image_pip_layout;
        let pip_render_size = config.still_image_pip_size;
        let reference = None;
        let reference_preview = None;
        let detector = DebouncedDetector::new(DetectorSettings {
            threshold: config.similarity_threshold,
            ..DetectorSettings::default()
        });
        let sequence = 1;
        let mut transition = TransitionController::default();
        let mut snapshot = AppSnapshot {
            mode,
            selected_video_device_id: config.selected_video_device_id.clone(),
            ..AppSnapshot::default()
        };
        snapshot.actual_output = Source::Placeholder;
        snapshot.automatic_target = Source::Placeholder;
        snapshot.output_layout = OutputLayout::Placeholder;
        snapshot.transition = transition.request(Source::Placeholder, now);
        snapshot.previews.final_output = Some(Arc::new(off_frame(FrameMetadata {
            sequence,
            timestamp_100ns: 0,
            received_at: now,
        })));
        snapshot.previews.reference = reference_preview;
        Self {
            capture_suspended: false,
            snapshot,
            config,
            transition,
            compositor: FrameCompositor::default(),
            disco: None,
            webcam_fps: SourceFpsTracker::default(),
            screen_fps: SourceFpsTracker::default(),
            output_fps: OutputFpsTracker::default(),
            warnings: WarningRegistry::default(),
            alerts: VecDeque::with_capacity(ALERT_LIMIT),
            next_alert_id: 0,
            activity: VecDeque::with_capacity(ACTIVITY_LIMIT),
            next_activity_id: 1,
            sequence,
            started_at: now,
            reference,
            detector,
            still_image_detector: StillImageDetector::default(),
            pip_transition: ReversibleMix::new(Duration::from_millis(500)),
            pip_render_layout,
            pip_render_size,
            last_detection: now - Duration::from_millis(250),
            #[cfg(windows)]
            pending_reference_capture: None,
            #[cfg(any(windows, test))]
            publisher_diagnostics: PublisherDiagnosticsState::default(),
        }
    }

    fn record(&mut self, message: impl Into<String>) {
        self.record_with_verbosity(ActivityVerbosity::Normal, message);
    }

    fn reset_still_image_detection(&mut self) {
        let was_active = self.still_image_detector.active();
        self.still_image_detector.reset();
        if was_active {
            self.record("Still-image picture-in-picture deactivated");
        }
    }

    #[cfg(any(windows, test))]
    fn record_verbose(&mut self, message: impl Into<String>) {
        self.record_with_verbosity(ActivityVerbosity::Verbose, message);
    }

    fn record_with_verbosity(&mut self, verbosity: ActivityVerbosity, message: impl Into<String>) {
        if verbosity == ActivityVerbosity::Verbose && !self.config.verbose_logging {
            return;
        }
        if self.activity.len() == ACTIVITY_LIMIT {
            self.activity.pop_front();
        }
        self.activity.push_back(message.into());
        self.next_activity_id = self.next_activity_id.saturating_add(1);
        self.snapshot.recent_activity_first_id = self
            .next_activity_id
            .saturating_sub(self.activity.len() as u64);
        self.snapshot.recent_activity = self.activity.iter().cloned().collect::<Vec<_>>().into();
    }

    fn record_alert(&mut self, source: RuntimeAlertSource, message: String) {
        self.next_alert_id = self.next_alert_id.saturating_add(1).max(1);
        if self.alerts.len() == ALERT_LIMIT {
            self.alerts.pop_front();
        }
        self.alerts.push_back(RuntimeAlert {
            id: self.next_alert_id,
            source,
            message,
            created_at: Instant::now(),
        });
        self.snapshot.recent_alerts_first_id = self
            .alerts
            .front()
            .map(|alert| alert.id)
            .unwrap_or_default();
        self.snapshot.recent_alerts = self.alerts.iter().cloned().collect::<Vec<_>>().into();
    }

    fn sync_warning_snapshot(&mut self) {
        self.snapshot.warning = self.warnings.top();
        self.snapshot.active_warnings = self.warnings.active_alerts().into();
    }

    fn set_warning(&mut self, source: WarningSource, message: impl Into<String>) {
        let message = message.into();
        if self.warnings.set(source, message.clone()) {
            self.sync_warning_snapshot();
            self.record_alert(source.alert_source(), message);
        }
    }

    fn clear_warning(&mut self, source: WarningSource) {
        if self.warnings.clear(source) {
            self.sync_warning_snapshot();
        }
    }

    #[cfg(any(windows, test))]
    fn record_publisher_diagnostics(
        &mut self,
        diagnostics: PublisherDiagnosticsSnapshot,
        now: Instant,
    ) {
        let previous = &self.publisher_diagnostics;
        let state_changed = previous.last_connected != Some(diagnostics.connected)
            || diagnostics.write_failures != previous.last_write_failures
            || diagnostics.disconnect_count != previous.last_disconnect_count
            || diagnostics.last_write_error != previous.last_write_error
            || diagnostics.last_disconnect_error != previous.last_disconnect_error
            || diagnostics.connection_count != previous.last_connection_count
            || diagnostics.connection_failures != previous.last_connection_failures;
        let progress_changed = diagnostics.published_sequence != previous.last_published_sequence
            || diagnostics.transmitted_sequence != previous.last_transmitted_sequence;
        let heartbeat_due = previous
            .last_reported_at
            .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(5));
        let should_report = state_changed || (progress_changed && heartbeat_due);
        let last_reported_at = if should_report {
            Some(now)
        } else {
            previous.last_reported_at
        };
        self.publisher_diagnostics = PublisherDiagnosticsState {
            last_published_sequence: diagnostics.published_sequence,
            last_transmitted_sequence: diagnostics.transmitted_sequence,
            last_connected: Some(diagnostics.connected),
            last_write_failures: diagnostics.write_failures,
            last_disconnect_count: diagnostics.disconnect_count,
            last_write_error: diagnostics.last_write_error,
            last_disconnect_error: diagnostics.last_disconnect_error,
            last_connection_count: diagnostics.connection_count,
            last_connection_failures: diagnostics.connection_failures,
            last_reported_at,
        };
        if should_report {
            let message = format!(
                "Virtual camera pipe status: connected={}, published_sequence={}, transmitted_sequence={}, disconnect_count={}, last_disconnect_error={:?}, write_failures={}, last_write_error={:?}, connection_count={}, connection_failures={}",
                diagnostics.connected,
                diagnostics.published_sequence,
                diagnostics.transmitted_sequence,
                diagnostics.disconnect_count,
                diagnostics.last_disconnect_error,
                diagnostics.write_failures,
                diagnostics.last_write_error,
                diagnostics.connection_count,
                diagnostics.connection_failures,
            );
            if state_changed {
                self.record(message);
            } else {
                self.record_verbose(message);
            }
        }
    }

    #[cfg(windows)]
    fn merge_device_warnings(&mut self, warnings: &WarningRegistry) {
        let changed = self.warnings.copy_device_sources_from(warnings);
        if !changed.is_empty() {
            self.sync_warning_snapshot();
            for (source, message) in changed {
                self.record_alert(source.alert_source(), message);
            }
        }
    }

    #[cfg(any(windows, test))]
    fn stage_reference_candidate(&mut self, frame: Arc<Frame>) {
        self.snapshot.previews.reference_candidate = Some(frame);
    }

    fn take_reference_candidate(&mut self) -> Option<Arc<Frame>> {
        self.snapshot.previews.reference_candidate.take()
    }

    #[cfg(test)]
    fn confirm_reference_candidate(&mut self) -> Result<(), String> {
        let frame = self
            .snapshot
            .previews
            .reference_candidate
            .as_ref()
            .cloned()
            .ok_or_else(|| "no reference candidate".to_owned())?;
        let data = reference_data_from_frame(&frame)?;
        save_reference(&frame, &self.config.reference_image_path)?;
        self.take_reference_candidate();
        self.install_reference(data.detector, &data.preview, Instant::now());
        Ok(())
    }

    fn discard_reference_candidate(&mut self) {
        self.snapshot.previews.reference_candidate = None;
    }

    fn command(&mut self, command: Command) -> bool {
        self.apply_command(command, true)
    }

    fn apply_command(&mut self, command: Command, record_activity: bool) -> bool {
        match command {
            Command::Start => {
                self.snapshot.run_state = RunState::Running;
                self.reset_still_image_detection();
                self.record_command(record_activity, "Automation started");
            }
            Command::Stop => {
                self.snapshot.run_state = RunState::Stopped;
                self.show_off_output(Instant::now());
                self.record_command(record_activity, "Automation stopped");
            }
            Command::ToggleDisco => {
                if self.disco.is_some() {
                    self.disco = None;
                    self.snapshot.disco_enabled = false;
                    self.record_command(record_activity, "Disco mode disabled");
                } else {
                    self.disco = Some(DiscoEffect::new(PIPELINE_SIZE));
                    self.snapshot.disco_enabled = true;
                    self.record_command(record_activity, "Disco mode enabled");
                }
            }
            Command::SetMode(mode) => {
                if self.snapshot.mode != mode {
                    self.reset_still_image_detection();
                }
                self.snapshot.mode = mode;
                self.config.output_mode = mode;
                self.record_command(record_activity, format!("Output mode changed to {mode:?}"));
            }
            Command::UpdateSettings(config) | Command::ReloadSettings(config) => {
                let threshold_changed =
                    self.config.similarity_threshold != config.similarity_threshold;
                let reference_path_changed =
                    self.config.reference_image_path != config.reference_image_path;
                let pip_settings_changed = self.config.still_image_pip_enabled
                    != config.still_image_pip_enabled
                    || self.config.still_image_pip_delay_seconds
                        != config.still_image_pip_delay_seconds
                    || self.config.still_image_pip_layout != config.still_image_pip_layout
                    || self.config.still_image_pip_size != config.still_image_pip_size;
                self.config = *config;
                self.snapshot.mode = self.config.output_mode;
                self.snapshot.selected_video_device_id =
                    self.config.selected_video_device_id.clone();
                if reference_path_changed {
                    self.discard_reference_candidate();
                    self.reset_still_image_detection();
                }
                if pip_settings_changed {
                    self.reset_still_image_detection();
                }
                if threshold_changed {
                    self.detector = DebouncedDetector::new(DetectorSettings {
                        threshold: self.config.similarity_threshold,
                        ..DetectorSettings::default()
                    });
                    self.snapshot.detection = stageswap_core::DetectionState::Unknown;
                }
                self.record_command(record_activity, "Settings updated");
            }
            Command::CaptureReference => {
                self.record_command(record_activity, "Reference capture requested");
            }
            Command::CaptureReferenceCandidate => {
                self.record_command(record_activity, "Reference candidate capture requested")
            }
            Command::ConfirmReferenceCandidate => self.record_command(
                record_activity,
                "Reference candidate confirmation requested",
            ),
            Command::DiscardReferenceCandidate => {
                self.discard_reference_candidate();
                self.record_command(record_activity, "Reference candidate discarded")
            }
            Command::ImportReference(path) => {
                let _ = path;
                self.record_command(record_activity, "Reference import requested");
            }
            Command::SelectMonitor(_) => {
                self.record_command(record_activity, "Tracked monitor selection requested")
            }
            Command::RefreshVideoDevices => {
                self.record_command(record_activity, "Video device list refreshed")
            }
            Command::Rescan => self.record_command(record_activity, "Monitor rescan requested"),
            Command::Restart(target) => {
                self.record_command(record_activity, format!("Restart requested: {target:?}"))
            }
            Command::Exit => return false,
        }
        true
    }

    fn record_command(&mut self, enabled: bool, message: impl Into<String>) {
        if enabled {
            let message = message.into();
            let is_coalesced_device_request = matches!(
                message.as_str(),
                "Video device list refreshed" | "Monitor rescan requested"
            ) && self
                .activity
                .back()
                .is_some_and(|previous| previous == &message);
            if !is_coalesced_device_request {
                self.record(message);
            }
        }
    }

    fn show_off_output(&mut self, now: Instant) {
        self.sequence = self.sequence.wrapping_add(1).max(1);
        let timestamp = now.saturating_duration_since(self.started_at).as_nanos() / 100;
        self.snapshot.actual_output = Source::Placeholder;
        self.snapshot.automatic_target = Source::Placeholder;
        self.snapshot.output_layout = OutputLayout::Placeholder;
        self.snapshot.still_image_pip_active = false;
        self.snapshot.still_image_pip_mix = 0.0;
        self.reset_still_image_detection();
        self.pip_transition.reset(now);
        self.snapshot.transition = self.transition.request(Source::Placeholder, now);
        self.snapshot.previews.final_output = Some(Arc::new(off_frame(FrameMetadata {
            sequence: self.sequence,
            timestamp_100ns: i64::try_from(timestamp).unwrap_or(i64::MAX),
            received_at: now,
        })));
    }

    #[cfg(any(windows, test))]
    /// Releases both native inputs on their owning thread before publishing suspension.
    fn release_session_capture(
        &mut self,
        webcam: &mut impl stageswap_windows::VideoInput,
        screen: &mut impl stageswap_windows::ScreenInput,
        now: Instant,
    ) {
        webcam.stop();
        screen.stop();
        self.set_capture_suspended(true, now);
    }

    #[cfg(any(windows, test))]
    /// Suspends live inputs without changing the user's automation or output-mode choice.
    fn set_capture_suspended(&mut self, suspended: bool, now: Instant) {
        if self.capture_suspended == suspended {
            return;
        }
        self.capture_suspended = suspended;
        self.detector.reset();
        self.reset_still_image_detection();
        self.snapshot.detection = stageswap_core::DetectionState::Unknown;
        self.snapshot.previews.webcam = None;
        self.snapshot.previews.screen = None;
        self.snapshot.availability = SourceAvailability::default();
        if suspended {
            self.snapshot.webcam_state = DeviceState::Unavailable;
            self.snapshot.screen_state = DeviceState::Unavailable;
            self.snapshot.webcam_native_format = None;
            self.snapshot.webcam_output_format = None;
            for component in [
                &mut self.snapshot.webcam_component,
                &mut self.snapshot.screen_component,
            ] {
                *component = stageswap_core::ComponentStatus {
                    state_since: Some(now),
                    ..stageswap_core::ComponentStatus::default()
                };
            }
            self.clear_warning(WarningSource::WebcamCapture);
            self.clear_warning(WarningSource::ScreenCapture);
            self.show_off_output(now);
        }
    }

    fn tick(&mut self, now: Instant) {
        if self.capture_suspended || self.snapshot.run_state != RunState::Running {
            self.show_off_output(now);
            return;
        }
        let decision = decide(
            self.snapshot.mode,
            self.snapshot.detection,
            self.snapshot.availability,
        );
        self.snapshot.automatic_target = decision.automatic_target;
        let automatic_pip = self.still_image_detector.active()
            && self.snapshot.mode == OutputMode::Automatic
            && self.config.still_image_pip_enabled;
        let pip_target = (self.snapshot.mode == OutputMode::ForcePip || automatic_pip)
            && self.snapshot.availability.camera_ready
            && self.snapshot.availability.screen_ready;
        if pip_target
            && (self.pip_transition.target == 0.0
                || (self.snapshot.mode == OutputMode::ForcePip
                    && (self.pip_render_layout != self.config.still_image_pip_layout
                        || self.pip_render_size != self.config.still_image_pip_size)))
        {
            self.pip_render_layout = self.config.still_image_pip_layout;
            self.pip_render_size = self.config.still_image_pip_size;
        }
        let pip_mix = self.pip_transition.request(pip_target, now);
        if !pip_target && pip_mix <= f64::EPSILON {
            self.pip_render_layout = self.config.still_image_pip_layout;
            self.pip_render_size = self.config.still_image_pip_size;
        }
        let desired_output = if pip_target {
            match self.pip_render_layout {
                StillImagePipLayout::WebcamMain => Source::Camera,
                StillImagePipLayout::ScreenMain => Source::Screen,
            }
        } else {
            decision.desired_output
        };
        if self.snapshot.actual_output != desired_output {
            self.snapshot.transition = self.transition.request(desired_output, now);
            self.snapshot.actual_output = desired_output;
        } else {
            self.snapshot.transition = self.transition.tick(now);
        }
        self.snapshot.still_image_pip_mix = pip_mix;
        self.snapshot.still_image_pip_active = pip_mix > f64::EPSILON;
        self.snapshot.output_layout = if pip_mix > f64::EPSILON {
            match self.pip_render_layout {
                StillImagePipLayout::WebcamMain => OutputLayout::WebcamMainScreenPip,
                StillImagePipLayout::ScreenMain => OutputLayout::ScreenMainWebcamPip,
            }
        } else {
            match desired_output {
                Source::Camera => OutputLayout::Camera,
                Source::Screen => OutputLayout::Screen,
                Source::Placeholder => OutputLayout::Placeholder,
            }
        };
        self.sequence = self.sequence.wrapping_add(1).max(1);
        let timestamp = now.saturating_duration_since(self.started_at).as_nanos() / 100;
        let output = self.compositor.compose_with_pip(
            self.snapshot.previews.webcam.as_ref(),
            self.snapshot.previews.screen.as_ref(),
            self.snapshot.transition.screen_mix,
            Some(PipComposition {
                layout: self.pip_render_layout,
                size: self.pip_render_size,
                mix: pip_mix,
            }),
            self.config.placeholder_color_bgra,
            FrameMetadata {
                sequence: self.sequence,
                timestamp_100ns: i64::try_from(timestamp).unwrap_or(i64::MAX),
                received_at: now,
            },
        );
        let output = if let Some(disco) = self.disco.as_mut() {
            disco.apply(&output, now.saturating_duration_since(self.started_at))
        } else {
            output
        };
        self.snapshot.previews.final_output = Some(Arc::new(output));
    }

    fn install_reference(&mut self, reference: GrayImage, preview: &Frame, now: Instant) {
        self.sequence = self.sequence.wrapping_add(1).max(1);
        self.reference = Some(reference);
        self.snapshot.previews.reference = Frame::new(
            preview.pixels_arc(),
            preview.size,
            preview.stride,
            self.sequence,
            preview.timestamp_100ns,
            now,
        )
        .ok()
        .map(Arc::new);
        self.detector.reset();
        self.reset_still_image_detection();
        self.snapshot.detection = stageswap_core::DetectionState::Unknown;
        self.last_detection = now - Duration::from_millis(250);
    }

    fn detect(&mut self, now: Instant) {
        if now.duration_since(self.last_detection) < Duration::from_millis(250) {
            return;
        }
        self.last_detection = now;
        let candidate = self
            .snapshot
            .availability
            .screen_ready
            .then_some(self.snapshot.previews.screen.as_deref())
            .flatten()
            .and_then(gray_thumbnail);
        let (similarity, valid) = match (&self.reference, candidate.as_ref()) {
            (Some(reference), Some(candidate)) => (image_similarity(reference, candidate), true),
            _ => (0.0, false),
        };
        self.snapshot.detection = self.detector.update(similarity, valid);
        let eligible = self.config.still_image_pip_enabled
            && self.snapshot.mode == OutputMode::Automatic
            && self.snapshot.availability.camera_ready
            && self.snapshot.availability.screen_ready
            && valid
            && similarity < self.config.similarity_threshold
            && candidate
                .as_ref()
                .is_some_and(|image| !gray_image_is_nearly_black(image));
        let was_active = self.still_image_detector.active();
        let active = self.still_image_detector.update(
            candidate.as_ref(),
            eligible,
            Duration::from_secs(u64::from(self.config.still_image_pip_delay_seconds)),
            now,
        );
        if active != was_active {
            self.record(if active {
                "Still-image picture-in-picture activated"
            } else {
                "Still-image picture-in-picture deactivated"
            });
        }
    }

    fn refresh_input_fps(&mut self, now: Instant) {
        let webcam = self
            .snapshot
            .availability
            .camera_ready
            .then_some(self.snapshot.previews.webcam.as_deref())
            .flatten();
        let screen = self
            .snapshot
            .availability
            .screen_ready
            .then_some(self.snapshot.previews.screen.as_deref())
            .flatten();
        self.snapshot.webcam_fps = self.webcam_fps.observe(webcam, now);
        self.snapshot.screen_fps = self.screen_fps.observe(screen, now);
    }
}

fn gray_thumbnail(frame: &Frame) -> Option<GrayImage> {
    resize_bgra_to_gray(
        frame.pixels(),
        frame.size,
        frame.stride as usize,
        Size::new(160, 90),
    )
    .ok()
}

trait RuntimePorts {
    fn command(&mut self, command: &Command, state: &mut RuntimeState);
    fn refresh_inputs(&mut self, state: &mut RuntimeState, now: Instant);
    fn publish(&self, state: &mut RuntimeState, now: Instant);
    fn reference_updated(&mut self, _state: &mut RuntimeState) {}
    fn signal_shutdown(&mut self) {}
    fn finish_shutdown(&mut self, _timeout: Duration) -> bool {
        true
    }
}

struct RuntimeEngine<P> {
    state: RuntimeState,
    platform: P,
    pacer: FramePacer,
    reference_worker: Option<ReferenceWorker>,
    reference_generation: u64,
    in_flight_candidate: Option<(u64, u64)>,
    reference_commit: Option<Arc<ReferenceCommit>>,
    last_deadline_log: Instant,
}

impl<P: RuntimePorts> RuntimeEngine<P> {
    fn from_parts(state: RuntimeState, platform: P, now: Instant) -> Self {
        let frame_interval = Duration::from_nanos(1_000_000_000 / u64::from(PIPELINE_FPS));
        let mut state = state;
        let reference_worker = match ReferenceWorker::start() {
            Ok(worker) => Some(worker),
            Err(error) => {
                state.record(error);
                None
            }
        };
        let mut engine = Self {
            state,
            platform,
            pacer: FramePacer::new(now, frame_interval),
            reference_worker,
            reference_generation: 0,
            in_flight_candidate: None,
            reference_commit: None,
            last_deadline_log: now - Duration::from_secs(30),
        };
        let path = std::path::PathBuf::from(engine.state.config.reference_image_path.clone());
        engine.submit_reference_job(ReferenceJobKind::Load { path });
        engine
    }

    fn wait_duration(&self, now: Instant) -> Duration {
        self.pacer.wait_duration(now)
    }

    fn command(&mut self, command: Command) -> bool {
        if matches!(
            command,
            Command::DiscardReferenceCandidate | Command::CaptureReferenceCandidate
        ) && self.in_flight_candidate.is_some()
        {
            if let Some(commit) = &self.reference_commit {
                commit.cancel();
            }
            self.reference_generation = self.reference_generation.wrapping_add(1).max(1);
            self.in_flight_candidate = None;
        }
        let previous_reference_path = self.state.config.reference_image_path.clone();
        self.platform.command(&command, &mut self.state);
        if !self.state.command(command.clone()) {
            return false;
        }
        match command {
            Command::UpdateSettings(config) => {
                if config.reference_image_path != previous_reference_path {
                    self.submit_reference_job(ReferenceJobKind::Load {
                        path: std::path::PathBuf::from(config.reference_image_path.clone()),
                    });
                }
            }
            Command::ReloadSettings(config) => {
                self.submit_reference_job(ReferenceJobKind::Load {
                    path: std::path::PathBuf::from(config.reference_image_path.clone()),
                });
            }
            Command::CaptureReference => {
                #[cfg(windows)]
                if let Some(frame) = self.state.pending_reference_capture.take() {
                    self.submit_reference_job(ReferenceJobKind::Persist {
                        frame,
                        path: std::path::PathBuf::from(
                            self.state.config.reference_image_path.clone(),
                        ),
                        confirmed: false,
                    });
                }
            }
            Command::ConfirmReferenceCandidate => {
                if let Some(frame) = self.state.snapshot.previews.reference_candidate.clone() {
                    if self
                        .in_flight_candidate
                        .is_some_and(|(sequence, _)| sequence == frame.sequence)
                    {
                        self.state
                            .record("Reference candidate confirmation is already pending");
                    } else {
                        self.submit_reference_job(ReferenceJobKind::Persist {
                            frame,
                            path: std::path::PathBuf::from(
                                self.state.config.reference_image_path.clone(),
                            ),
                            confirmed: true,
                        });
                    }
                } else {
                    self.state.set_warning(
                        WarningSource::Command,
                        "Reference candidate confirmation failed because no candidate is available",
                    );
                    self.state
                        .record("Reference candidate confirmation failed: no reference candidate");
                }
            }
            Command::ImportReference(source) => {
                self.submit_reference_job(ReferenceJobKind::Import {
                    source,
                    destination: std::path::PathBuf::from(
                        self.state.config.reference_image_path.clone(),
                    ),
                });
            }
            _ => {}
        }
        true
    }

    fn submit_reference_job(&mut self, kind: ReferenceJobKind) -> bool {
        let generation = self.reference_generation.wrapping_add(1).max(1);
        let candidate_sequence = match &kind {
            ReferenceJobKind::Persist {
                frame,
                confirmed: true,
                ..
            } => Some(frame.sequence),
            _ => None,
        };
        let Some(worker) = self.reference_worker.as_ref() else {
            self.state
                .set_warning(WarningSource::Command, "Reference worker is unavailable");
            self.state.record("Reference worker is unavailable");
            return false;
        };
        let commit = Arc::new(ReferenceCommit::default());
        match worker.submit(ReferenceJob {
            generation,
            kind,
            commit: Some(Arc::clone(&commit)),
        }) {
            Ok(()) => {
                if let Some(previous) = self.reference_commit.replace(commit) {
                    previous.cancel();
                }
                self.reference_generation = generation;
                self.state.clear_warning(WarningSource::Command);
                self.in_flight_candidate =
                    candidate_sequence.map(|sequence| (sequence, generation));
                true
            }
            Err(error) => {
                self.state
                    .set_warning(WarningSource::Command, error.clone());
                self.state
                    .record(format!("Reference command failed: {error}"));
                false
            }
        }
    }

    fn poll_reference_results(&mut self, now: Instant) {
        while let Some(result) = self
            .reference_worker
            .as_ref()
            .and_then(ReferenceWorker::poll)
        {
            if self
                .in_flight_candidate
                .is_some_and(|(_, generation)| generation == result.generation)
            {
                self.in_flight_candidate = None;
            }
            // A successful replacement is already durable. Reconcile it even when
            // cancellation arrived after the commit claim; otherwise disk and runtime diverge.
            let persisted_here = result.result.is_ok()
                && result.persisted_path.as_deref()
                    == Some(std::path::Path::new(
                        &self.state.config.reference_image_path,
                    ));
            if result.persisted_path.is_some() && !persisted_here && result.result.is_ok()
                || result.generation != self.reference_generation && !persisted_here
            {
                self.state.record(format!(
                    "Ignored obsolete reference result {}",
                    result.generation
                ));
                continue;
            }
            match result.result {
                Ok(data) => {
                    self.state.clear_warning(WarningSource::Reference);
                    if let ReferenceAction::ConfirmedCapture { frame_sequence } = result.action {
                        let candidate_matches = self
                            .state
                            .snapshot
                            .previews
                            .reference_candidate
                            .as_ref()
                            .is_some_and(|candidate| candidate.sequence == frame_sequence);
                        if candidate_matches {
                            self.state.take_reference_candidate();
                        }
                    } else if result.action == ReferenceAction::Import
                        && result.generation == self.reference_generation
                    {
                        self.state.discard_reference_candidate();
                    }
                    self.state
                        .install_reference(data.detector, &data.preview, now);
                    self.platform.reference_updated(&mut self.state);
                    let message = match result.action {
                        ReferenceAction::Load => "Reference loaded",
                        ReferenceAction::LegacyCapture { .. } => "Reference captured",
                        ReferenceAction::ConfirmedCapture { .. } => "Reference candidate confirmed",
                        ReferenceAction::Import => "Reference imported into local storage",
                    };
                    self.state.record(message);
                }
                Err(error) => {
                    if result.action == ReferenceAction::Load {
                        self.state.reference = None;
                        self.state.snapshot.previews.reference = None;
                        self.state.detector.reset();
                        self.state.snapshot.detection = stageswap_core::DetectionState::Unknown;
                        self.state.reset_still_image_detection();
                        self.platform.reference_updated(&mut self.state);
                    }
                    self.state
                        .set_warning(WarningSource::Reference, error.clone());
                    self.state
                        .record(format!("Reference operation failed: {error}"));
                }
            }
        }
    }

    fn shutdown_workers(&mut self) {
        if let Some(worker) = self.reference_worker.as_mut() {
            worker.signal_shutdown();
        }
        self.platform.signal_shutdown();
        let deadline = Instant::now() + WORKER_SHUTDOWN_TIMEOUT;
        if let Some(worker) = self.reference_worker.as_mut() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let _ = worker.finish_shutdown(remaining);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let _ = self.platform.finish_shutdown(remaining);
    }

    fn step(&mut self, now: Instant) -> bool {
        if !self.pacer.is_due(now, Duration::ZERO) {
            return false;
        }
        let skipped = self.pacer.advance(now);
        self.state.snapshot.output_deadline_misses = self
            .state
            .snapshot
            .output_deadline_misses
            .saturating_add(skipped);
        if skipped > 0
            && now.saturating_duration_since(self.last_deadline_log) >= Duration::from_secs(30)
        {
            self.state.record(format!(
                "Output pacer skipped {skipped} overdue deadline(s)"
            ));
            self.last_deadline_log = now;
        }
        self.poll_reference_results(now);
        self.platform.refresh_inputs(&mut self.state, now);
        #[cfg(windows)]
        if let Some(frame) = self.state.pending_reference_capture.take() {
            self.submit_reference_job(ReferenceJobKind::Persist {
                frame,
                path: std::path::PathBuf::from(self.state.config.reference_image_path.clone()),
                confirmed: false,
            });
        }
        self.state.refresh_input_fps(now);
        self.state.detect(now);
        self.state.tick(now);
        self.state.snapshot.output_fps = self.state.output_fps.observe(now);
        self.platform.publish(&mut self.state, now);
        true
    }
}

fn run<C: RuntimeClock>(
    config: AppConfig,
    commands: CommandInbox,
    shared: Arc<RwLock<AppSnapshot>>,
    clock: C,
) {
    let start_automatically = config.start_automatically;
    let now = clock.now();
    let mut state = RuntimeState::new_at(config, now);
    state.snapshot.availability = SourceAvailability::default();
    state.snapshot.webcam_state = DeviceState::Unavailable;
    state.snapshot.screen_state = DeviceState::Unavailable;
    state.snapshot.virtual_camera_state = DeviceState::Unavailable;
    state.snapshot.actual_output = Source::Placeholder;
    let platform = Platform::new(&mut state);
    if start_automatically {
        state.command(Command::Start);
    }
    let mut engine = RuntimeEngine::from_parts(state, platform, now);
    'runtime: loop {
        if commands.shutdown_requested() {
            break;
        }
        let mut processed = 0;
        if let Some(command) = commands.recv_timeout(engine.wait_duration(clock.now())) {
            if !engine.command(command) {
                break 'runtime;
            }
            processed += 1;
        }
        while processed < MAX_COMMANDS_PER_OUTPUT_CYCLE && !commands.shutdown_requested() {
            let Some(command) = commands.try_recv() else {
                break;
            };
            if !engine.command(command) {
                break 'runtime;
            }
            processed += 1;
        }
        if commands.shutdown_requested() {
            break;
        }
        let now = clock.now();
        if engine.step(now) {
            *shared
                .write()
                .expect("runtime snapshot lock is not poisoned") = engine.state.snapshot.clone();
        }
    }
    engine.shutdown_workers();
}

#[cfg(not(windows))]
struct Platform;

#[cfg(not(windows))]
impl Platform {
    fn new(_state: &mut RuntimeState) -> Self {
        Self
    }
    fn command(&mut self, _command: &Command, _state: &mut RuntimeState) {}
    fn refresh_inputs(&mut self, _state: &mut RuntimeState, _now: Instant) {}
    fn publish(&self, _state: &mut RuntimeState, _now: Instant) {}
    fn reference_updated(&mut self, _state: &mut RuntimeState) {}
}

impl RuntimePorts for Platform {
    fn command(&mut self, command: &Command, state: &mut RuntimeState) {
        Platform::command(self, command, state);
    }

    fn refresh_inputs(&mut self, state: &mut RuntimeState, now: Instant) {
        Platform::refresh_inputs(self, state, now);
    }

    fn publish(&self, state: &mut RuntimeState, now: Instant) {
        Platform::publish(self, state, now);
    }

    fn reference_updated(&mut self, state: &mut RuntimeState) {
        Platform::reference_updated(self, state);
    }

    fn signal_shutdown(&mut self) {
        #[cfg(windows)]
        Platform::signal_shutdown(self);
    }

    fn finish_shutdown(&mut self, timeout: Duration) -> bool {
        #[cfg(windows)]
        {
            Platform::finish_shutdown(self, timeout)
        }
        #[cfg(not(windows))]
        {
            let _ = timeout;
            true
        }
    }
}

#[cfg(test)]
mod tests;
