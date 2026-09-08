//! Selected-device lifecycle and publication, isolated from the output engine.

use super::*;
#[cfg(windows)]
use stageswap_core::{MonitorTracker, MonitorTrackerSettings};
#[cfg(windows)]
use stageswap_windows::{
    FramePublisher, FramePublisherSink, MediaFoundationVideoInput, ScreenInput, VideoInput,
    VirtualCameraController, WindowsGraphicsScreenInput, choose_video_device,
    current_session_active, frame_pipe_name,
};

#[cfg(windows)]
pub(super) struct DevicePlatform {
    pub(super) session_active: bool,
    pub(super) last_session_check: Instant,
    pub(super) session_query_failed: bool,
    pub(super) owner_thread: thread::ThreadId,
    pub(super) publisher: Option<FramePublisher>,
    pub(super) camera: Option<VirtualCameraController>,
    pub(super) pipe_name: Option<String>,
    pub(super) webcam: MediaFoundationVideoInput,
    pub(super) screen: WindowsGraphicsScreenInput,
    pub(super) selected_monitor: Option<MonitorDescriptor>,
    pub(super) selected_display_hdr_unsupported: bool,
    pub(super) monitor_tracker: MonitorTracker,
    pub(super) screen_capture_recovery: ScreenCaptureRecovery,
    pub(super) webcam_recovery: WebcamRecovery,
    pub(super) last_monitor_scan: Instant,
    pub(super) last_screen_capture_recovery_check: Instant,
    pub(super) monitor_scan_generation: u64,
    pub(super) monitor_scan_worker: Option<MonitorScanWorker>,
    pub(super) video_device_worker: Option<VideoDeviceWorker>,
    pub(super) capture_diagnostics: CaptureDiagnosticsState,
}

#[cfg(windows)]
impl DevicePlatform {
    pub(super) fn record_webcam_format(
        state: &mut RuntimeState,
        webcam: &MediaFoundationVideoInput,
    ) {
        let native = webcam.selected_native_format().unwrap_or("unknown");
        let output = webcam.selected_output_format().unwrap_or("unknown");
        state.record(format!(
            "Webcam format negotiated: native={native}; output={output}"
        ));
    }

    pub(super) fn new(
        state: &mut RuntimeState,
        output_ready: impl FnOnce(&RuntimeState, Option<FramePublisherSink>),
    ) -> Self {
        let now = Instant::now();
        state
            .snapshot
            .publisher_component
            .transition(ComponentLifecycle::Starting, now);
        state
            .snapshot
            .virtual_camera_component
            .transition(ComponentLifecycle::Starting, now);
        let pipe_name = match frame_pipe_name() {
            Ok(name) => Some(name),
            Err(error) => {
                state.set_warning(WarningSource::PublisherController, error.clone());
                state.record(format!("Virtual camera pipe failed: {error}"));
                None
            }
        };
        let publisher =
            pipe_name
                .as_ref()
                .and_then(|pipe_name| match FramePublisher::start(pipe_name) {
                    Ok(publisher) => {
                        state.record("Virtual camera frame pipe created");
                        Some(publisher)
                    }
                    Err(error) => {
                        state
                            .snapshot
                            .publisher_component
                            .mark_failed(now, error.clone());
                        state.set_warning(WarningSource::PublisherController, error.clone());
                        state.record(format!("Frame publisher failed: {error}"));
                        None
                    }
                });
        output_ready(state, publisher.as_ref().map(FramePublisher::sink));
        let camera = publisher.as_ref().and_then(|_| {
            pipe_name.as_ref().and_then(|pipe_name| {
                match VirtualCameraController::start(pipe_name.clone()) {
                    Ok(camera) => {
                        state.snapshot.virtual_camera_state = DeviceState::Ready;
                        state.snapshot.virtual_camera_component.mark_ready(now);
                        state.record("Virtual camera initialized");
                        Some(camera)
                    }
                    Err(error) => {
                        state.snapshot.virtual_camera_state = DeviceState::Failed;
                        state
                            .snapshot
                            .virtual_camera_component
                            .mark_failed(now, error.clone());
                        state.set_warning(WarningSource::VirtualCamera, error.clone());
                        state.record(format!("Virtual camera failed: {error}"));
                        None
                    }
                }
            })
        });
        if publisher.is_none() {
            state.snapshot.virtual_camera_state = DeviceState::Failed;
            if state.snapshot.publisher_component.lifecycle != ComponentLifecycle::Failed {
                state
                    .snapshot
                    .publisher_component
                    .mark_failed(now, "frame publisher is unavailable");
            }
            if state.snapshot.virtual_camera_component.lifecycle != ComponentLifecycle::Failed {
                state
                    .snapshot
                    .virtual_camera_component
                    .mark_failed(now, "frame publisher is unavailable");
            }
        }
        let session_active = match current_session_active() {
            Ok(active) => active,
            Err(error) => {
                state.record(error);
                false
            }
        };
        let mut webcam = MediaFoundationVideoInput::default();
        webcam.set_crop_to_16_9(state.config.crop_webcam_to_16_9);
        state
            .snapshot
            .webcam_component
            .transition(ComponentLifecycle::Starting, now);
        match webcam.enumerate() {
            Ok(devices) => {
                state.snapshot.video_devices = devices
                    .iter()
                    .map(|device| stageswap_core::VideoDeviceChoice {
                        id: device.id.clone(),
                        name: device.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .into();
                let saved = state.config.selected_video_device_id.clone();
                let saved_is_physical = devices.iter().any(|device| device.id == saved);
                let saved_opened =
                    saved_is_physical && session_active && webcam.start(&saved).is_ok();
                let selected = choose_video_device(
                    &saved,
                    &devices,
                    saved_opened || (!session_active && saved_is_physical),
                );
                let opened = if saved_opened || !session_active {
                    selected
                } else if let Some(selected) = selected {
                    match webcam.start(&selected) {
                        Ok(()) => Some(selected),
                        Err(error) => {
                            let error = error.message;
                            state.set_warning(WarningSource::WebcamCapture, error.clone());
                            state.record(format!("Webcam initialization failed: {error}"));
                            None
                        }
                    }
                } else {
                    None
                };
                if let Some(selected) = opened {
                    state.config.selected_video_device_id = selected;
                    state.snapshot.selected_video_device_id =
                        state.config.selected_video_device_id.clone();
                    if session_active {
                        state.snapshot.webcam_state = DeviceState::Initializing;
                        state
                            .snapshot
                            .webcam_component
                            .waiting_for_first_frame(now, now + WEBCAM_FIRST_FRAME_TIMEOUT);
                        state.snapshot.webcam_native_format =
                            webcam.selected_native_format().map(str::to_owned);
                        state.snapshot.webcam_output_format =
                            webcam.selected_output_format().map(str::to_owned);
                        Self::record_webcam_format(state, &webcam);
                        state.record("Webcam initialized");
                    }
                } else if devices.is_empty() {
                    state
                        .snapshot
                        .webcam_component
                        .transition(ComponentLifecycle::Stopped, now);
                    state.record("No physical webcam found");
                } else {
                    state
                        .snapshot
                        .webcam_component
                        .transition(ComponentLifecycle::Stopped, now);
                    state.record("Webcam selection required");
                }
            }
            Err(error) => {
                state.snapshot.webcam_state = DeviceState::Failed;
                state.snapshot.webcam_component.mark_failed_with_kind(
                    now,
                    ComponentFailureKind::Webcam(WebcamFailureKind::DriverFailure),
                    error.clone(),
                );
                state.set_warning(WarningSource::WebcamCapture, error.clone());
                state.record(format!("Webcam enumeration failed: {error}"));
            }
        }
        let mut screen = WindowsGraphicsScreenInput::default();
        state
            .snapshot
            .screen_component
            .transition(ComponentLifecycle::Starting, now);
        let selected_monitor = match screen.enumerate() {
            Ok(monitors) => {
                state.snapshot.monitors = monitors.clone().into();
                choose_initial_monitor(&state.config.selected_monitor_label, &monitors).and_then(
                    |monitor| {
                        if !session_active {
                            return Some(monitor);
                        }
                        match screen.start(&monitor, state.config.cursor_visible) {
                            Ok(()) => {
                                state.snapshot.screen_state = DeviceState::Initializing;
                                state
                                    .snapshot
                                    .screen_component
                                    .waiting_for_first_frame(now, now + SCREEN_FIRST_FRAME_TIMEOUT);
                                state.record(format!(
                                    "Screen capture initialized: {} generation={}",
                                    monitor.label,
                                    screen.generation()
                                ));
                                Some(monitor)
                            }
                            Err(error) => {
                                let failure_kind = error.kind;
                                let error = error.message;
                                state.snapshot.screen_state = DeviceState::Failed;
                                state.snapshot.screen_component.mark_failed_with_kind(
                                    now,
                                    ComponentFailureKind::Screen(failure_kind),
                                    error.clone(),
                                );
                                state.set_warning(WarningSource::ScreenCapture, error.clone());
                                state.record(format!("Screen capture failed: {error}"));
                                None
                            }
                        }
                    },
                )
            }
            Err(error) => {
                state.snapshot.screen_state = DeviceState::Failed;
                state.snapshot.screen_component.mark_failed_with_kind(
                    now,
                    ComponentFailureKind::Screen(ScreenFailureKind::CaptureFailure),
                    error.clone(),
                );
                state.set_warning(WarningSource::ScreenCapture, error.clone());
                state.record(format!("Monitor enumeration failed: {error}"));
                None
            }
        };
        let mut monitor_tracker = MonitorTracker::new(MonitorTrackerSettings {
            match_threshold: state.config.similarity_threshold,
        });
        if let Some(monitor) = selected_monitor.clone() {
            monitor_tracker.select(monitor);
        }
        if let Some(monitor) = selected_monitor.as_ref() {
            state
                .config
                .selected_monitor_label
                .clone_from(&monitor.label);
        }
        state.snapshot.selected_monitor = selected_monitor.clone();
        let selected_display_hdr_unsupported = selected_monitor.as_ref().is_some_and(|monitor| {
            match screen.display_uses_hdr_or_ten_bit(monitor) {
                Ok(unsupported) => unsupported,
                Err(error) => {
                    state.set_warning(
                        WarningSource::Hdr,
                        format!(
                            "Could not determine the selected display color capability: {error}"
                        ),
                    );
                    state.record(format!("Display color capability check failed: {error}"));
                    false
                }
            }
        });
        if selected_display_hdr_unsupported {
            state.set_warning(
                WarningSource::Hdr,
                "HDR or 10-bit color is enabled on the selected display; disable HDR in Windows Display settings before using automatic matching or reference capture"
                    .to_owned(),
            );
        } else {
            state.clear_warning(WarningSource::Hdr);
        }
        let monitor_scan_worker = match MonitorScanWorker::start(session_active) {
            Ok(worker) => Some(worker),
            Err(error) => {
                state.record(error);
                None
            }
        };
        let video_device_worker = match VideoDeviceWorker::start() {
            Ok(worker) => Some(worker),
            Err(error) => {
                state.record(error);
                None
            }
        };
        state.set_capture_suspended(!session_active, now);
        let mut platform = Self {
            session_active,
            last_session_check: now,
            session_query_failed: false,
            owner_thread: thread::current().id(),
            publisher,
            camera,
            pipe_name,
            webcam,
            screen,
            selected_monitor,
            selected_display_hdr_unsupported,
            monitor_tracker,
            screen_capture_recovery: ScreenCaptureRecovery::default(),
            webcam_recovery: WebcamRecovery::default(),
            last_monitor_scan: Instant::now(),
            last_screen_capture_recovery_check: Instant::now(),
            monitor_scan_generation: 1,
            monitor_scan_worker,
            video_device_worker,
            capture_diagnostics: CaptureDiagnosticsState::default(),
        };
        if state.config.automatic_monitor_rescans {
            platform.request_monitor_scan(state, state.config.cursor_visible);
        }
        platform
    }

    pub(super) fn command(&mut self, command: &Command, state: &mut RuntimeState) {
        self.assert_owner_thread();
        self.refresh_session(state, Instant::now());
        match command {
            Command::UpdateSettings(config) | Command::ReloadSettings(config) => {
                let reload_settings = matches!(command, Command::ReloadSettings(_));
                if config.crop_webcam_to_16_9 != state.config.crop_webcam_to_16_9 {
                    self.webcam.set_crop_to_16_9(config.crop_webcam_to_16_9);
                }
                if config.selected_video_device_id != state.config.selected_video_device_id {
                    let now = Instant::now();
                    self.webcam_recovery.reset();
                    self.webcam.stop();
                    state.snapshot.webcam_native_format = None;
                    state.snapshot.webcam_output_format = None;
                    if config.selected_video_device_id.is_empty() || !self.session_active {
                        state.snapshot.webcam_state = DeviceState::Unavailable;
                        state
                            .snapshot
                            .webcam_component
                            .transition(ComponentLifecycle::Stopped, now);
                    } else {
                        match self.webcam.start(&config.selected_video_device_id) {
                            Ok(()) => {
                                state.snapshot.webcam_state = DeviceState::Initializing;
                                state
                                    .snapshot
                                    .webcam_component
                                    .waiting_for_first_frame(now, now + WEBCAM_FIRST_FRAME_TIMEOUT);
                                state.snapshot.webcam_native_format =
                                    self.webcam.selected_native_format().map(str::to_owned);
                                state.snapshot.webcam_output_format =
                                    self.webcam.selected_output_format().map(str::to_owned);
                                Self::record_webcam_format(state, &self.webcam);
                            }
                            Err(error) => {
                                let failure_kind = error.kind;
                                let error = error.message;
                                state.snapshot.webcam_state = DeviceState::Failed;
                                state.snapshot.webcam_component.mark_failed_with_kind(
                                    now,
                                    ComponentFailureKind::Webcam(failure_kind),
                                    error.clone(),
                                );
                                state.record(format!("Webcam selection failed: {error}"));
                            }
                        }
                    }
                }
                let cursor_changed = config.cursor_visible != state.config.cursor_visible;
                let threshold_changed =
                    config.similarity_threshold != state.config.similarity_threshold;
                let automatic_rescans_changed =
                    config.automatic_monitor_rescans != state.config.automatic_monitor_rescans;
                let automatic_recovery_changed = config.automatic_screen_capture_recovery
                    != state.config.automatic_screen_capture_recovery;
                if cursor_changed {
                    let old = state.config.cursor_visible;
                    state.config.cursor_visible = config.cursor_visible;
                    self.restart_screen(state);
                    state.config.cursor_visible = old;
                }
                if reload_settings {
                    self.invalidate_monitor_scans(config.similarity_threshold);
                } else if cursor_changed || threshold_changed || automatic_rescans_changed {
                    self.invalidate_monitor_scans(config.similarity_threshold);
                    if config.automatic_monitor_rescans {
                        self.request_monitor_scan(state, config.cursor_visible);
                    }
                }
                if automatic_recovery_changed {
                    self.screen_capture_recovery.reset();
                    self.last_screen_capture_recovery_check = Instant::now();
                }
            }
            Command::CaptureReference => {
                state.pending_reference_capture = None;
                if self.selected_display_hdr_unsupported {
                    state.record(
                        "Reference capture is unavailable while HDR or 10-bit color is enabled",
                    );
                    return;
                }
                if let Some(frame) = self.screen.latest_frame() {
                    state.clear_warning(WarningSource::Reference);
                    state.pending_reference_capture = Some(frame);
                } else {
                    state.set_warning(
                        WarningSource::Reference,
                        "Reference capture failed because the selected screen has no frame",
                    );
                    state.record("Reference capture failed: no screen frame");
                }
            }
            Command::CaptureReferenceCandidate => {
                if self.selected_display_hdr_unsupported {
                    state.record(
                        "Reference capture is unavailable while HDR or 10-bit color is enabled",
                    );
                    return;
                }
                if let Some(frame) = self.screen.latest_frame() {
                    state.clear_warning(WarningSource::Reference);
                    state.stage_reference_candidate(frame);
                    state.record("Reference candidate captured for review");
                } else {
                    state.set_warning(
                        WarningSource::Reference,
                        "Reference capture failed because the selected screen has no frame",
                    );
                    state.record("Reference candidate capture failed: no screen frame");
                }
            }
            Command::ConfirmReferenceCandidate => {}
            Command::DiscardReferenceCandidate => {
                state.discard_reference_candidate();
                state.record("Reference candidate discarded");
            }
            Command::ImportReference(_) => {}
            Command::SelectMonitor(monitor) => {
                let available = self.screen.enumerate().unwrap_or_default();
                if let Some(monitor) = available
                    .into_iter()
                    .find(|candidate| candidate.display_name == monitor.display_name)
                {
                    self.selected_monitor = Some(monitor.clone());
                    self.selected_display_hdr_unsupported = self
                        .screen
                        .display_uses_hdr_or_ten_bit(&monitor)
                        .unwrap_or(false);
                    self.synchronize_hdr_warning(state);
                    self.monitor_tracker.select(monitor.clone());
                    self.monitor_scan_generation = self.monitor_scan_generation.wrapping_add(1);
                    if let Some(worker) = self.monitor_scan_worker.as_mut() {
                        worker.clear_pending();
                    }
                    state
                        .config
                        .selected_monitor_label
                        .clone_from(&monitor.label);
                    state.snapshot.selected_monitor = Some(monitor);
                    self.restart_screen(state);
                } else {
                    state.set_warning(
                        WarningSource::ScreenCapture,
                        "Tracked monitor selection failed: monitor unavailable",
                    );
                    state.record("Tracked monitor selection failed: monitor unavailable");
                }
            }
            Command::RefreshVideoDevices => {
                if self
                    .video_device_worker
                    .as_ref()
                    .is_none_or(|worker| !worker.request())
                {
                    state.set_warning(
                        WarningSource::Command,
                        "Video device refresh worker is unavailable",
                    );
                    state.record("Video device refresh worker is unavailable");
                }
            }
            Command::Rescan => {
                if !self.queue_monitor_scan(state, state.config.cursor_visible) {
                    state.set_warning(
                        WarningSource::Command,
                        "Monitor rescan worker is unavailable",
                    );
                    state.record("Monitor rescan worker is unavailable");
                } else {
                    state.clear_warning(WarningSource::Command);
                }
            }
            Command::Stop => {
                if let Some(publisher) = &self.publisher {
                    let _ = publisher.invalidate();
                }
            }
            Command::Restart(RestartTarget::VirtualCamera)
            | Command::Restart(RestartTarget::All) => {
                self.restart_virtual_camera(state);
            }
            Command::Restart(RestartTarget::Webcam) => self.restart_webcam(state),
            Command::Restart(RestartTarget::ScreenCapture) => self.restart_screen(state),
            _ => {}
        }
        if matches!(command, Command::Restart(RestartTarget::All)) {
            self.restart_webcam(state);
            self.restart_screen(state);
        }
    }

    pub(super) fn reference_updated(&mut self, state: &mut RuntimeState) {
        self.assert_owner_thread();
        self.invalidate_monitor_scans(state.config.similarity_threshold);
        if state.config.automatic_monitor_rescans {
            self.request_monitor_scan(state, state.config.cursor_visible);
        }
    }

    /// Checks the owning session on the device thread, independently of dashboard visibility.
    pub(super) fn refresh_session(&mut self, state: &mut RuntimeState, now: Instant) {
        if now.saturating_duration_since(self.last_session_check) < Duration::from_millis(250) {
            return;
        }
        self.last_session_check = now;
        let active = match current_session_active() {
            Ok(active) => {
                self.session_query_failed = false;
                active
            }
            Err(error) => {
                if !self.session_query_failed {
                    state.record(error);
                }
                self.session_query_failed = true;
                false
            }
        };
        if active == self.session_active {
            return;
        }
        self.session_active = active;
        self.invalidate_monitor_scans(state.config.similarity_threshold);
        if let Some(worker) = &mut self.monitor_scan_worker {
            worker.capture_active.store(false, Ordering::Release);
            if active {
                worker.capture_active = Arc::new(AtomicBool::new(true));
            }
        }
        self.webcam_recovery.reset();
        self.screen_capture_recovery.reset();
        self.last_monitor_scan = now;
        self.last_screen_capture_recovery_check = now;
        if active {
            state.set_capture_suspended(false, now);
            self.webcam
                .set_crop_to_16_9(state.config.crop_webcam_to_16_9);
            self.restart_webcam(state);
            self.restart_screen(state);
            if state.config.automatic_monitor_rescans {
                self.request_monitor_scan(state, state.config.cursor_visible);
            }
            state.record("Windows session active: capture resumed");
        } else {
            state.release_session_capture(&mut self.webcam, &mut self.screen, now);
            state.record("Windows session inactive: capture released");
        }
    }

    pub(super) fn synchronize_hdr_warning(&self, state: &mut RuntimeState) {
        if self.selected_display_hdr_unsupported {
            state.set_warning(
                WarningSource::Hdr,
                "HDR or 10-bit color is enabled on the selected display; disable HDR in Windows Display settings before using automatic matching or reference capture",
            );
        } else {
            state.clear_warning(WarningSource::Hdr);
        }
    }

    pub(super) fn assert_owner_thread(&self) {
        assert_eq!(
            thread::current().id(),
            self.owner_thread,
            "Windows device objects must only be used by their owner thread"
        );
    }

    pub(super) fn queue_webcam_recovery(
        &mut self,
        state: &mut RuntimeState,
        now: Instant,
        reason: &str,
    ) {
        self.webcam.stop();
        state.snapshot.previews.webcam = None;
        state.snapshot.availability.camera_ready = false;
        let scheduled =
            if self.webcam_recovery.active && self.webcam_recovery.waiting_for_first_frame {
                self.webcam_recovery.attempt_failed(now)
            } else {
                self.webcam_recovery.schedule_initial(now)
            };
        state.snapshot.webcam_component.consecutive_restart_failures =
            u32::from(self.webcam_recovery.attempts);
        state.snapshot.webcam_component.next_permitted_retry = self.webcam_recovery.next_attempt;
        if scheduled {
            let delay = self.webcam_recovery.next_attempt.map_or(0, |attempt_at| {
                attempt_at.saturating_duration_since(now).as_millis()
            });
            state.record(format!(
                "Webcam automatic recovery scheduled in {delay} ms after: {reason}"
            ));
        } else if !self.webcam_recovery.active {
            state.record(format!(
                "Webcam automatic recovery exhausted after {} attempts: {reason}",
                self.webcam_recovery.attempts
            ));
        }
    }

    pub(super) fn attempt_webcam_recovery(&mut self, state: &mut RuntimeState, now: Instant) {
        let Some(attempt) = self.webcam_recovery.begin_due_attempt(now) else {
            return;
        };
        let id = state.config.selected_video_device_id.clone();
        state.snapshot.webcam_component.consecutive_restart_failures = u32::from(attempt);
        state.snapshot.webcam_component.next_permitted_retry = None;
        state
            .snapshot
            .webcam_component
            .transition(ComponentLifecycle::Restarting, now);
        state.record(format!(
            "Webcam automatic recovery attempt {attempt}/{}",
            WEBCAM_RECOVERY_DELAYS.len()
        ));
        self.webcam.stop();
        state.snapshot.webcam_native_format = None;
        state.snapshot.webcam_output_format = None;
        if id.is_empty() {
            self.webcam_recovery.exhaust();
            state.snapshot.webcam_state = DeviceState::Unavailable;
            state.snapshot.webcam_component.mark_failed(
                now,
                "webcam automatic recovery cannot continue without a selected device",
            );
            state.record("Webcam automatic recovery stopped: no video input selected");
            return;
        }
        match self.webcam.start(&id) {
            Ok(()) => {
                self.webcam_recovery.attempt_started();
                state.snapshot.webcam_state = DeviceState::Initializing;
                state
                    .snapshot
                    .webcam_component
                    .waiting_for_first_frame(now, now + WEBCAM_FIRST_FRAME_TIMEOUT);
                state.snapshot.webcam_component.consecutive_restart_failures = u32::from(attempt);
                state.snapshot.webcam_native_format =
                    self.webcam.selected_native_format().map(str::to_owned);
                state.snapshot.webcam_output_format =
                    self.webcam.selected_output_format().map(str::to_owned);
                Self::record_webcam_format(state, &self.webcam);
                state.record(format!(
                    "Webcam automatic recovery attempt {attempt} started capture_generation={}",
                    self.webcam.capture_generation()
                ));
            }
            Err(error) => {
                let failure_kind = error.kind;
                let error = error.message;
                state.snapshot.webcam_state = DeviceState::Failed;
                let kind = failure_kind;
                state.snapshot.webcam_component.mark_failed_with_kind(
                    now,
                    ComponentFailureKind::Webcam(kind),
                    error.clone(),
                );
                state.snapshot.webcam_component.consecutive_restart_failures = u32::from(attempt);
                let retry = webcam_failure_is_automatically_recoverable(kind)
                    && self.webcam_recovery.attempt_failed(now);
                state.snapshot.webcam_component.next_permitted_retry =
                    self.webcam_recovery.next_attempt;
                if retry {
                    state.record(format!(
                        "Webcam automatic recovery attempt {attempt} failed; another attempt is scheduled: {error}"
                    ));
                } else {
                    self.webcam_recovery.exhaust();
                    state.record(format!(
                        "Webcam automatic recovery stopped after attempt {attempt}: {error}"
                    ));
                }
            }
        }
    }

    pub(super) fn restart_webcam(&mut self, state: &mut RuntimeState) {
        if !self.session_active {
            return;
        }
        let now = Instant::now();
        let id = state.config.selected_video_device_id.clone();
        if state.snapshot.webcam_component.lifecycle == ComponentLifecycle::Restarting {
            return;
        }
        self.webcam_recovery.reset();
        state
            .snapshot
            .webcam_component
            .transition(ComponentLifecycle::Restarting, now);
        self.webcam.stop();
        state.snapshot.webcam_native_format = None;
        state.snapshot.webcam_output_format = None;
        if id.is_empty() {
            state.snapshot.webcam_state = DeviceState::Unavailable;
            state
                .snapshot
                .webcam_component
                .transition(ComponentLifecycle::Stopped, now);
            state.set_warning(
                WarningSource::WebcamCapture,
                "Webcam restart skipped because no video input is selected",
            );
            state.record("Webcam restart skipped: no video input selected");
            return;
        }
        match self.webcam.start(&id) {
            Ok(()) => {
                state.snapshot.webcam_state = DeviceState::Initializing;
                state
                    .snapshot
                    .webcam_component
                    .waiting_for_first_frame(now, now + WEBCAM_FIRST_FRAME_TIMEOUT);
                state.snapshot.webcam_native_format =
                    self.webcam.selected_native_format().map(str::to_owned);
                state.snapshot.webcam_output_format =
                    self.webcam.selected_output_format().map(str::to_owned);
                Self::record_webcam_format(state, &self.webcam);
                state.record("Webcam restarted");
            }
            Err(error) => {
                let failure_kind = error.kind;
                let error = error.message;
                state.snapshot.webcam_state = DeviceState::Failed;
                state.snapshot.webcam_component.mark_failed_with_kind(
                    now,
                    ComponentFailureKind::Webcam(failure_kind),
                    error.clone(),
                );
                state.set_warning(WarningSource::WebcamCapture, error.clone());
                state.record(format!("Webcam restart failed: {error}"));
            }
        }
    }

    pub(super) fn restart_virtual_camera(&mut self, state: &mut RuntimeState) {
        let now = Instant::now();
        if state.snapshot.virtual_camera_component.lifecycle == ComponentLifecycle::Restarting {
            return;
        }
        state
            .snapshot
            .virtual_camera_component
            .transition(ComponentLifecycle::Restarting, now);
        let result = (|| {
            if self.pipe_name.is_none() {
                self.pipe_name = Some(frame_pipe_name()?);
            }
            let pipe_name = self.pipe_name.as_deref().expect("pipe name was resolved");
            if let Err(error) = FramePublisher::ensure_running(&mut self.publisher, pipe_name) {
                state
                    .snapshot
                    .publisher_component
                    .mark_failed(now, error.clone());
                state.set_warning(WarningSource::PublisherController, error.clone());
                return Err(error);
            }
            state.snapshot.publisher_component.mark_ready(now);
            state.clear_warning(WarningSource::PublisherController);
            if let Some(camera) = &mut self.camera {
                camera.restart()
            } else {
                VirtualCameraController::start(pipe_name.to_owned())
                    .map(|camera| self.camera = Some(camera))
            }
        })();
        match result {
            Ok(()) => {
                state.snapshot.virtual_camera_state = DeviceState::Ready;
                state.snapshot.virtual_camera_component.mark_ready(now);
                state.clear_warning(WarningSource::VirtualCamera);
                state.record("Virtual camera restarted");
            }
            Err(error) => {
                state.snapshot.virtual_camera_state = DeviceState::Failed;
                state
                    .snapshot
                    .virtual_camera_component
                    .mark_failed(now, error.clone());
                state.set_warning(WarningSource::VirtualCamera, error.clone());
                state.record(format!("Virtual camera restart failed: {error}"));
            }
        }
    }

    pub(super) fn refresh_inputs(&mut self, state: &mut RuntimeState, now: Instant) {
        self.assert_owner_thread();
        self.refresh_session(state, now);
        if !self.session_active {
            return;
        }
        self.refresh_video_device_result(state);
        self.refresh_monitor_scan(state);
        let (monitor_scan_due, screen_capture_recovery_due) = automatic_screen_tasks_due(
            state.config.automatic_monitor_rescans,
            state.config.automatic_screen_capture_recovery,
            self.last_monitor_scan,
            self.last_screen_capture_recovery_check,
            now,
        );
        if monitor_scan_due {
            self.request_monitor_scan(state, state.config.cursor_visible);
        }
        if screen_capture_recovery_due {
            self.check_screen_capture_recovery(state, now);
        }
        let webcam_dropped = self.webcam.dropped_frame_count();
        let screen_dropped = self.screen.dropped_frame_count();
        let webcam_fallbacks = self.webcam.pool_fallback_count();
        let screen_fallbacks = self.screen.pool_fallback_count();
        state.snapshot.webcam_capture_dropped_frames = webcam_dropped;
        state.snapshot.screen_capture_dropped_frames = screen_dropped;
        state.snapshot.webcam_capture_pool_fallbacks = webcam_fallbacks;
        state.snapshot.screen_capture_pool_fallbacks = screen_fallbacks;
        if self.capture_diagnostics.observe(
            webcam_dropped,
            screen_dropped,
            webcam_fallbacks,
            screen_fallbacks,
            now,
        ) {
            state.record_verbose(format!(
                "Capture buffer status: webcam_dropped={webcam_dropped}, screen_dropped={screen_dropped}, webcam_pool_fallbacks={webcam_fallbacks}, screen_pool_fallbacks={screen_fallbacks}, output_deadline_misses={}",
                state.snapshot.output_deadline_misses
            ));
        }
        let webcam = self.webcam.latest_frame();
        let webcam_is_stale = webcam.as_ref().is_some_and(|frame| {
            now.saturating_duration_since(frame.received_at) > FRAME_STALE_AFTER
        });
        let webcam = (!webcam_is_stale).then_some(webcam).flatten();
        state.snapshot.previews.webcam = None;
        state.snapshot.availability.camera_ready = webcam.is_some();
        if let Some(failure) = self.webcam.last_failure() {
            let error = failure.message.clone();
            let kind = failure.kind;
            state.snapshot.availability.camera_ready = false;
            state.set_warning(WarningSource::WebcamCapture, error.clone());
            if state.snapshot.webcam_state != DeviceState::Failed {
                state.record(error.clone());
            }
            state.snapshot.webcam_state = DeviceState::Failed;
            state.snapshot.webcam_component.mark_failed_with_kind(
                now,
                ComponentFailureKind::Webcam(kind),
                error.clone(),
            );
            state.snapshot.previews.webcam = None;
            if webcam_failure_is_automatically_recoverable(kind) {
                self.queue_webcam_recovery(state, now, &error);
            }
        } else if webcam_is_stale {
            let error = "Webcam frames are stale; safe fallback is active";
            state.set_warning(WarningSource::WebcamCapture, error);
            state.snapshot.webcam_state = DeviceState::Failed;
            state
                .snapshot
                .webcam_component
                .transition(ComponentLifecycle::Stale, now);
            state.snapshot.previews.webcam = None;
            self.queue_webcam_recovery(state, now, error);
        } else if webcam.is_some() {
            let recovered = self.webcam_recovery.active;
            let recovery_attempts = self.webcam_recovery.attempts;
            self.webcam_recovery.reset();
            state.clear_warning(WarningSource::WebcamCapture);
            state.snapshot.webcam_state = DeviceState::Ready;
            let was_ready = state.snapshot.webcam_component.lifecycle == ComponentLifecycle::Ready;
            state.snapshot.webcam_component.mark_ready(now);
            state.snapshot.webcam_component.last_success_at =
                webcam.as_ref().map(|frame| frame.received_at);
            state.snapshot.previews.webcam = webcam;
            if !was_ready {
                state.record("Webcam first frame received");
            }
            if recovered {
                state.record(format!(
                    "Webcam automatic recovery succeeded after {recovery_attempts} attempt(s) capture_generation={}",
                    self.webcam.capture_generation()
                ));
            }
        } else if state
            .snapshot
            .webcam_component
            .first_frame_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            let error = "webcam did not deliver a frame before the startup deadline";
            state.set_warning(WarningSource::WebcamCapture, error);
            state.snapshot.webcam_state = DeviceState::Failed;
            state.snapshot.webcam_component.mark_failed(now, error);
            state.snapshot.webcam_component.last_failure_kind = Some(ComponentFailureKind::Webcam(
                WebcamFailureKind::DriverFailure,
            ));
            state.snapshot.previews.webcam = None;
            state.record("Webcam first-frame deadline expired");
            self.queue_webcam_recovery(state, now, error);
        }
        self.attempt_webcam_recovery(state, now);
        let screen = self.screen.latest_frame();
        state.snapshot.previews.screen = None;
        state.snapshot.availability.screen_ready = screen.is_some()
            && !(self.selected_display_hdr_unsupported
                && state.snapshot.mode == stageswap_core::OutputMode::Automatic);
        if self.selected_display_hdr_unsupported
            && state.snapshot.mode == stageswap_core::OutputMode::Automatic
        {
            state.set_warning(
                WarningSource::ScreenCapture,
                "Screen capture is unavailable while HDR or 10-bit color is enabled",
            );
            state.snapshot.screen_state = DeviceState::Failed;
            state.snapshot.screen_component.mark_failed_with_kind(
                now,
                ComponentFailureKind::Screen(ScreenFailureKind::UnsupportedHdr),
                "HDR or 10-bit color must be disabled for automatic matching",
            );
            state.snapshot.previews.screen = None;
        } else if let Some(failure) = self.screen.last_failure() {
            let error = failure.message;
            state.snapshot.availability.screen_ready = false;
            state.set_warning(WarningSource::ScreenCapture, error.clone());
            if state.snapshot.screen_state != DeviceState::Failed {
                state.record(error.clone());
            }
            state.snapshot.screen_state = DeviceState::Failed;
            state.snapshot.screen_component.mark_failed_with_kind(
                now,
                ComponentFailureKind::Screen(failure.kind),
                error,
            );
            state.snapshot.previews.screen = None;
        } else if let Some(screen) = screen {
            state.clear_warning(WarningSource::ScreenCapture);
            let received_at = screen.received_at;
            state.snapshot.screen_state = DeviceState::Ready;
            let was_ready = state.snapshot.screen_component.lifecycle == ComponentLifecycle::Ready;
            state.snapshot.screen_component.mark_ready(now);
            state.snapshot.screen_component.last_success_at = Some(received_at);
            state.snapshot.previews.screen = Some(screen);
            if !was_ready {
                state.record(format!(
                    "Screen capture first frame received generation={}",
                    self.screen.generation()
                ));
            }
        } else if state
            .snapshot
            .screen_component
            .first_frame_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            state.set_warning(
                WarningSource::ScreenCapture,
                "Screen capture did not deliver a frame before the startup deadline",
            );
            state.snapshot.screen_state = DeviceState::Failed;
            state.snapshot.screen_component.mark_failed(
                now,
                "screen capture did not deliver a frame before the startup deadline",
            );
            state.snapshot.screen_component.last_failure_kind = Some(ComponentFailureKind::Screen(
                ScreenFailureKind::CaptureFailure,
            ));
            state.snapshot.previews.screen = None;
            state.record("Screen capture first-frame deadline expired");
        }
        if self
            .camera
            .as_ref()
            .is_some_and(|camera| !camera.is_running())
        {
            state.snapshot.virtual_camera_state = DeviceState::Failed;
            state.set_warning(
                WarningSource::VirtualCamera,
                "virtual camera stopped unexpectedly",
            );
            state
                .snapshot
                .virtual_camera_component
                .mark_failed(now, "virtual camera stopped unexpectedly");
        }
    }

    pub(super) fn refresh_video_device_result(&mut self, state: &mut RuntimeState) {
        let Some(result) = self
            .video_device_worker
            .as_ref()
            .and_then(VideoDeviceWorker::poll)
        else {
            return;
        };
        match result {
            Ok(devices) => {
                state.snapshot.video_devices = devices
                    .into_iter()
                    .map(|device| stageswap_core::VideoDeviceChoice {
                        id: device.id,
                        name: device.name,
                    })
                    .collect::<Vec<_>>()
                    .into();
                state.clear_warning(WarningSource::Command);
                state.record("Video device list refreshed from settings");
            }
            Err(error) => {
                state.set_warning(WarningSource::Command, error.clone());
                state.record(format!("Video device refresh failed: {error}"));
            }
        }
    }

    pub(super) fn restart_screen(&mut self, state: &mut RuntimeState) {
        self.restart_screen_with_policy(state, false);
    }

    pub(super) fn restart_screen_with_policy(&mut self, state: &mut RuntimeState, automatic: bool) {
        if !self.session_active {
            return;
        }
        let now = Instant::now();
        if automatic
            && state
                .snapshot
                .screen_component
                .next_permitted_retry
                .is_some_and(|retry_at| now < retry_at)
        {
            return;
        }
        state
            .snapshot
            .screen_component
            .transition(ComponentLifecycle::Restarting, now);
        self.screen_capture_recovery.reset();
        self.last_screen_capture_recovery_check = now;
        self.screen.stop();
        state.snapshot.previews.screen = None;
        state.snapshot.availability.screen_ready = false;
        let Some(monitor) = self.selected_monitor.as_ref() else {
            state.snapshot.screen_state = DeviceState::Unavailable;
            state.set_warning(
                WarningSource::ScreenCapture,
                "Screen capture cannot restart because the selected display is unavailable",
            );
            state.snapshot.screen_component.mark_failed(
                now,
                "selected display is unavailable; automatic monitor reselection is disabled",
            );
            state.snapshot.screen_component.last_failure_kind = Some(ComponentFailureKind::Screen(
                ScreenFailureKind::MissingSelectedMonitor,
            ));
            return;
        };
        match self.screen.start(monitor, state.config.cursor_visible) {
            Ok(()) => {
                state.snapshot.screen_state = DeviceState::Initializing;
                state
                    .snapshot
                    .screen_component
                    .waiting_for_first_frame(now, now + SCREEN_FIRST_FRAME_TIMEOUT);
                state.record(format!(
                    "Screen capture restarted generation={}",
                    self.screen.generation()
                ));
            }
            Err(error) => {
                let failure_kind = error.kind;
                let error = error.message;
                state.snapshot.screen_state = DeviceState::Failed;
                let failures = state
                    .snapshot
                    .screen_component
                    .consecutive_restart_failures
                    .saturating_add(1);
                state.snapshot.screen_component.mark_failed_with_kind(
                    now,
                    ComponentFailureKind::Screen(failure_kind),
                    error.clone(),
                );
                state.set_warning(WarningSource::ScreenCapture, error.clone());
                state.snapshot.screen_component.consecutive_restart_failures = failures;
                if automatic {
                    state.snapshot.screen_component.next_permitted_retry =
                        Some(now + screen_restart_backoff(failures));
                }
                state.record(format!("Screen capture restart failed: {error}"));
            }
        }
    }

    pub(super) fn invalidate_monitor_scans(&mut self, match_threshold: f64) {
        self.monitor_scan_generation = self.monitor_scan_generation.wrapping_add(1);
        if let Some(worker) = self.monitor_scan_worker.as_mut() {
            worker.clear_pending();
        }
        let mut tracker = MonitorTracker::new(MonitorTrackerSettings { match_threshold });
        if let Some(monitor) = self.selected_monitor.clone() {
            tracker.select(monitor);
        }
        self.monitor_tracker = tracker;
    }

    pub(super) fn check_screen_capture_recovery(&mut self, state: &mut RuntimeState, now: Instant) {
        self.last_screen_capture_recovery_check = now;
        match self
            .screen_capture_recovery
            .observe(self.screen.latest_frame().as_deref())
        {
            ScreenCaptureRecoveryObservation::Clear => {}
            ScreenCaptureRecoveryObservation::AwaitingConfirmation => {
                state.record_verbose(format!(
                    "Screen capture is black or unavailable; awaiting the next automatic recovery check generation={}",
                    self.screen.generation()
                ));
            }
            ScreenCaptureRecoveryObservation::Restart => {
                state.record(format!(
                    "Screen capture remains black or unavailable; restarting screen capture generation={}",
                    self.screen.generation()
                ));
                self.restart_screen_with_policy(state, true);
            }
        }
    }

    pub(super) fn request_monitor_scan(&mut self, state: &RuntimeState, cursor_visible: bool) {
        if !self.queue_monitor_scan(state, cursor_visible) {
            self.last_monitor_scan = Instant::now();
        }
    }

    pub(super) fn queue_monitor_scan(
        &mut self,
        state: &RuntimeState,
        cursor_visible: bool,
    ) -> bool {
        if !self.session_active {
            return true;
        }
        let Some(worker) = &mut self.monitor_scan_worker else {
            return false;
        };
        if worker.request(MonitorScanRequest {
            capture_active: Arc::clone(&worker.capture_active),
            generation: self.monitor_scan_generation,
            reference: state.reference.clone(),
            cursor_visible,
        }) {
            self.last_monitor_scan = Instant::now();
            true
        } else {
            false
        }
    }

    pub(super) fn refresh_monitor_scan(&mut self, state: &mut RuntimeState) {
        let Some(result) = self
            .monitor_scan_worker
            .as_ref()
            .and_then(MonitorScanWorker::poll)
        else {
            return;
        };
        if result.generation != self.monitor_scan_generation {
            return;
        }
        state.snapshot.monitors = result.monitors.into();
        if result.scores.is_empty() {
            return;
        }
        let tracking = self.monitor_tracker.apply_scan(&result.scores);
        if tracking.confirmation_pending {
            self.queue_monitor_scan(state, state.config.cursor_visible);
            return;
        }
        if let Some(monitor) = tracking.tracked
            && self.selected_monitor.as_ref() != Some(&monitor)
        {
            self.selected_monitor = Some(monitor);
            self.selected_display_hdr_unsupported = self
                .selected_monitor
                .as_ref()
                .and_then(|monitor| self.screen.display_uses_hdr_or_ten_bit(monitor).ok())
                .unwrap_or(false);
            self.synchronize_hdr_warning(state);
            state.snapshot.selected_monitor = self.selected_monitor.clone();
            if let Some(monitor) = self.selected_monitor.as_ref() {
                state
                    .config
                    .selected_monitor_label
                    .clone_from(&monitor.label);
            }
            self.restart_screen(state);
            state.record("Reference monitor changed after two scans");
        }
    }
}

#[cfg(windows)]
impl Drop for DevicePlatform {
    fn drop(&mut self) {
        self.assert_owner_thread();
        if let Some(publisher) = &self.publisher {
            let _ = publisher.invalidate();
        }
    }
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub(super) struct PendingDeviceCommands {
    pub(super) settings: Option<Command>,
    pub(super) mode: Option<Command>,
    pub(super) monitor: Option<MonitorDescriptor>,
    pub(super) refresh_video_devices: bool,
    pub(super) rescan: bool,
    pub(super) stop_output: bool,
    pub(super) capture_reference: bool,
    pub(super) capture_candidate: bool,
    pub(super) discard_candidate: bool,
    pub(super) restarts: HashSet<RestartTarget>,
    // Outer None means no update; Some(None) explicitly clears the reference.
    pub(super) reference: Option<Option<GrayImage>>,
}

#[cfg(any(windows, test))]
impl PendingDeviceCommands {
    /// Coalesces installs and explicit clears while dismissing obsolete candidates.
    pub(super) fn update_reference(&mut self, reference: Option<GrayImage>) {
        self.reference = Some(reference);
        self.capture_candidate = false;
        self.discard_candidate = true;
    }

    pub(super) fn push(&mut self, command: &Command) {
        match command {
            command @ (Command::UpdateSettings(_) | Command::ReloadSettings(_)) => {
                self.settings = Some(command.clone());
                self.mode = None;
            }
            command @ Command::SetMode(_) => self.mode = Some(command.clone()),
            Command::SelectMonitor(monitor) => self.monitor = Some(monitor.clone()),
            Command::RefreshVideoDevices => self.refresh_video_devices = true,
            Command::Rescan => self.rescan = true,
            Command::Stop => self.stop_output = true,
            Command::CaptureReference => self.capture_reference = true,
            Command::CaptureReferenceCandidate => self.capture_candidate = true,
            Command::DiscardReferenceCandidate => {
                self.capture_candidate = false;
                self.discard_candidate = true;
            }
            Command::Restart(RestartTarget::All) => {
                self.restarts.clear();
                self.restarts.insert(RestartTarget::All);
            }
            Command::Restart(target) if !self.restarts.contains(&RestartTarget::All) => {
                self.restarts.insert(*target);
            }
            _ => {}
        }
    }

    pub(super) fn take(&mut self) -> (Vec<Command>, Option<Option<GrayImage>>) {
        let mut commands = Vec::with_capacity(9);
        commands.extend(self.settings.take());
        commands.extend(self.mode.take());
        commands.extend(self.monitor.take().map(Command::SelectMonitor));
        if std::mem::take(&mut self.refresh_video_devices) {
            commands.push(Command::RefreshVideoDevices);
        }
        if std::mem::take(&mut self.rescan) {
            commands.push(Command::Rescan);
        }
        if std::mem::take(&mut self.capture_reference) {
            commands.push(Command::CaptureReference);
        }
        if std::mem::take(&mut self.discard_candidate) {
            commands.push(Command::DiscardReferenceCandidate);
        }
        if std::mem::take(&mut self.capture_candidate) {
            commands.push(Command::CaptureReferenceCandidate);
        }
        if std::mem::take(&mut self.stop_output) {
            commands.push(Command::Stop);
        }
        if self.restarts.remove(&RestartTarget::All) {
            self.restarts.clear();
            commands.push(Command::Restart(RestartTarget::All));
        } else {
            commands.extend(self.restarts.drain().map(Command::Restart));
        }
        (commands, self.reference.take())
    }
}

#[cfg(windows)]
#[derive(Default)]
pub(super) struct DeviceSnapshot {
    pub(super) capture_suspended: bool,
    pub(super) app: AppSnapshot,
    pub(super) warnings: WarningRegistry,
    pub(super) observed_at: Option<Instant>,
    pub(super) publisher: Option<FramePublisherSink>,
    pub(super) legacy_capture: Option<(u64, Arc<Frame>)>,
    pub(super) candidate_event: Option<(u64, Option<Arc<Frame>>)>,
}

#[cfg(windows)]
pub(super) struct DeviceWorker {
    pub(super) latest: Arc<RwLock<Arc<DeviceSnapshot>>>,
    pub(super) pending: Arc<(Mutex<PendingDeviceCommands>, Condvar)>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) done: Receiver<()>,
    pub(super) worker: Option<JoinHandle<()>>,
    pub(super) last_activity_id: u64,
    pub(super) last_legacy_capture_id: u64,
    pub(super) last_candidate_event_id: u64,
}

#[cfg(windows)]
impl DeviceWorker {
    pub(super) fn start(config: AppConfig) -> Result<Self, String> {
        let latest = Arc::new(RwLock::new(Arc::new(DeviceSnapshot::default())));
        let worker_latest = Arc::clone(&latest);
        let pending = Arc::new((Mutex::new(PendingDeviceCommands::default()), Condvar::new()));
        let worker_pending = Arc::clone(&pending);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (done_sender, done_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("stageswap-device-owner".into())
            .spawn(move || {
                let now = Instant::now();
                let mut state = RuntimeState::new_at(config, now);
                let mut platform = DevicePlatform::new(&mut state, |state, publisher| {
                    let snapshot = DeviceSnapshot {
                        app: state.snapshot.clone(),
                        warnings: state.warnings.clone(),
                        observed_at: Some(Instant::now()),
                        publisher,
                        ..DeviceSnapshot::default()
                    };
                    *worker_latest
                        .write()
                        .expect("device snapshot state is not poisoned") = Arc::new(snapshot);
                });
                let mut pacer = FramePacer::new(
                    now,
                    Duration::from_nanos(1_000_000_000 / u64::from(PIPELINE_FPS)),
                );
                let mut last_slow_command_log = now - Duration::from_secs(60);
                let mut legacy_capture = None;
                let mut legacy_capture_id = 0_u64;
                let mut candidate_event = None;
                let mut candidate_event_id = 0_u64;
                publish_device_snapshot(
                    &worker_latest,
                    &platform,
                    &state,
                    &legacy_capture,
                    &candidate_event,
                );
                while !worker_stop.load(Ordering::Acquire) {
                    let (commands, reference) = {
                        let (lock, _) = &*worker_pending;
                        lock.lock()
                            .expect("device command state is not poisoned")
                            .take()
                    };
                    if let Some(reference) = reference {
                        state.reference = reference;
                        platform.reference_updated(&mut state);
                    }
                    for command in commands {
                        let started = Instant::now();
                        platform.command(&command, &mut state);
                        state.apply_command(command.clone(), false);
                        if matches!(
                            command,
                            Command::CaptureReferenceCandidate | Command::DiscardReferenceCandidate
                        ) {
                            candidate_event_id = candidate_event_id.wrapping_add(1).max(1);
                            candidate_event = Some((
                                candidate_event_id,
                                state.snapshot.previews.reference_candidate.clone(),
                            ));
                        }
                        if matches!(command, Command::CaptureReference)
                            && let Some(frame) = state.pending_reference_capture.take()
                        {
                            legacy_capture_id = legacy_capture_id.wrapping_add(1).max(1);
                            legacy_capture = Some((legacy_capture_id, frame));
                        }
                        let elapsed = started.elapsed();
                        if elapsed >= Duration::from_millis(100)
                            && started.saturating_duration_since(last_slow_command_log)
                                >= Duration::from_secs(30)
                        {
                            state.record_verbose(format!(
                                "Device command took {} ms",
                                elapsed.as_millis()
                            ));
                            last_slow_command_log = started;
                        }
                    }
                    let now = Instant::now();
                    if pacer.is_due(now, Duration::ZERO) {
                        pacer.advance(now);
                        platform.refresh_inputs(&mut state, now);
                        publish_device_snapshot(
                            &worker_latest,
                            &platform,
                            &state,
                            &legacy_capture,
                            &candidate_event,
                        );
                    }
                    let timeout = pacer
                        .wait_duration(Instant::now())
                        .min(Duration::from_millis(34));
                    let (lock, changed) = &*worker_pending;
                    let guard = lock.lock().expect("device command state is not poisoned");
                    if !worker_stop.load(Ordering::Acquire) {
                        let _ = changed.wait_timeout(guard, timeout);
                    }
                }
                let _ = platform.publisher.as_ref().map(FramePublisher::invalidate);
                drop(platform);
                let _ = done_sender.try_send(());
            })
            .map_err(|error| format!("could not start device worker: {error}"))?;
        Ok(Self {
            latest,
            pending,
            stop,
            done: done_receiver,
            worker: Some(worker),
            last_activity_id: 0,
            last_legacy_capture_id: 0,
            last_candidate_event_id: 0,
        })
    }

    pub(super) fn push(&self, command: &Command) {
        let (lock, changed) = &*self.pending;
        lock.lock()
            .expect("device command state is not poisoned")
            .push(command);
        changed.notify_one();
    }

    pub(super) fn update_reference(&self, reference: Option<GrayImage>) {
        let (lock, changed) = &*self.pending;
        let mut pending = lock.lock().expect("device command state is not poisoned");
        pending.update_reference(reference);
        drop(pending);
        changed.notify_one();
    }

    pub(super) fn snapshot(&self) -> Arc<DeviceSnapshot> {
        Arc::clone(
            &self
                .latest
                .read()
                .expect("device snapshot state is not poisoned"),
        )
    }

    pub(super) fn signal_shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.pending.1.notify_all();
    }

    pub(super) fn finish_shutdown(&mut self, timeout: Duration) -> bool {
        finish_worker_shutdown(&self.done, &mut self.worker, timeout)
    }
}

#[cfg(windows)]
impl Drop for DeviceWorker {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.signal_shutdown();
            let _ = self.finish_shutdown(WORKER_SHUTDOWN_TIMEOUT);
        }
    }
}

#[cfg(windows)]
pub(super) fn publish_device_snapshot(
    destination: &RwLock<Arc<DeviceSnapshot>>,
    platform: &DevicePlatform,
    state: &RuntimeState,
    legacy_capture: &Option<(u64, Arc<Frame>)>,
    candidate_event: &Option<(u64, Option<Arc<Frame>>)>,
) {
    let snapshot = DeviceSnapshot {
        capture_suspended: state.capture_suspended,
        app: state.snapshot.clone(),
        warnings: state.warnings.clone(),
        observed_at: Some(Instant::now()),
        publisher: platform.publisher.as_ref().map(FramePublisher::sink),
        legacy_capture: legacy_capture.clone(),
        candidate_event: candidate_event.clone(),
    };
    *destination
        .write()
        .expect("device snapshot state is not poisoned") = Arc::new(snapshot);
}

/// Accepts only frames that the capture worker considers usable and are still fresh.
#[cfg(any(windows, test))]
pub(super) fn ready_webcam_frame(snapshot: &AppSnapshot, now: Instant) -> Option<Arc<Frame>> {
    snapshot
        .previews
        .webcam
        .as_ref()
        .filter(|frame| {
            snapshot.availability.camera_ready
                && frame.is_fresh_at(now, stageswap_core::FRAME_STALE_AFTER)
        })
        .cloned()
}

/// Static screen frames remain usable only while their owning worker is responsive.
#[cfg(any(windows, test))]
pub(super) fn ready_screen_frame(
    snapshot: &AppSnapshot,
    observed_at: Option<Instant>,
    now: Instant,
) -> Option<Arc<Frame>> {
    snapshot
        .previews
        .screen
        .as_ref()
        .filter(|_| {
            snapshot.availability.screen_ready
                && observed_at.is_some_and(|observed| {
                    now.saturating_duration_since(observed) <= FRAME_STALE_AFTER
                })
        })
        .cloned()
}

#[cfg(windows)]
pub(super) struct Platform {
    pub(super) worker: Option<DeviceWorker>,
}

#[cfg(windows)]
impl Platform {
    pub(super) fn new(state: &mut RuntimeState) -> Self {
        match DeviceWorker::start(state.config.clone()) {
            Ok(worker) => Self {
                worker: Some(worker),
            },
            Err(error) => {
                state.set_warning(WarningSource::DeviceWorker, error.clone());
                state.snapshot.webcam_state = DeviceState::Failed;
                state.snapshot.screen_state = DeviceState::Failed;
                state.snapshot.virtual_camera_state = DeviceState::Failed;
                state.record(error);
                Self { worker: None }
            }
        }
    }

    pub(super) fn command(&mut self, command: &Command, _state: &mut RuntimeState) {
        if let Some(worker) = &self.worker {
            worker.push(command);
        }
    }

    pub(super) fn refresh_inputs(&mut self, state: &mut RuntimeState, now: Instant) {
        let Some(worker) = &mut self.worker else {
            state.snapshot.previews.webcam = None;
            state.snapshot.previews.screen = None;
            state.snapshot.availability = SourceAvailability::default();
            return;
        };
        let device = worker.snapshot();
        let app = &device.app;
        state.set_capture_suspended(device.capture_suspended, now);
        if device.observed_at.is_some_and(|observed_at| {
            now.saturating_duration_since(observed_at) > FRAME_STALE_AFTER
        }) {
            state.set_warning(
                WarningSource::DeviceWorker,
                "Device worker is delayed; safe fallback is active",
            );
        } else {
            state.clear_warning(WarningSource::DeviceWorker);
        }
        state.snapshot.webcam_state = app.webcam_state;
        state.snapshot.screen_state = app.screen_state;
        state.snapshot.virtual_camera_state = app.virtual_camera_state;
        state.snapshot.webcam_component = app.webcam_component.clone();
        state.snapshot.screen_component = app.screen_component.clone();
        state.snapshot.publisher_component = app.publisher_component.clone();
        state.snapshot.virtual_camera_component = app.virtual_camera_component.clone();
        state.snapshot.video_devices = Arc::clone(&app.video_devices);
        state.snapshot.selected_video_device_id = app.selected_video_device_id.clone();
        state.snapshot.webcam_native_format = app.webcam_native_format.clone();
        state.snapshot.webcam_output_format = app.webcam_output_format.clone();
        state.snapshot.webcam_capture_dropped_frames = app.webcam_capture_dropped_frames;
        state.snapshot.screen_capture_dropped_frames = app.screen_capture_dropped_frames;
        state.snapshot.webcam_capture_pool_fallbacks = app.webcam_capture_pool_fallbacks;
        state.snapshot.screen_capture_pool_fallbacks = app.screen_capture_pool_fallbacks;
        state.snapshot.monitors = Arc::clone(&app.monitors);
        state.snapshot.selected_monitor = app.selected_monitor.clone();
        state.merge_device_warnings(&device.warnings);

        let webcam_is_stale = app
            .previews
            .webcam
            .as_ref()
            .is_some_and(|frame| !frame.is_fresh_at(now, FRAME_STALE_AFTER));
        let webcam = ready_webcam_frame(app, now);
        state.snapshot.previews.webcam = webcam;
        if webcam_is_stale {
            state.snapshot.webcam_state = DeviceState::Failed;
            state
                .snapshot
                .webcam_component
                .transition(ComponentLifecycle::Stale, now);
        }
        state.snapshot.previews.screen = ready_screen_frame(app, device.observed_at, now);
        state.snapshot.availability.camera_ready = state.snapshot.previews.webcam.is_some();
        state.snapshot.availability.screen_ready = state.snapshot.previews.screen.is_some();

        if let Some((capture_id, frame)) = &device.legacy_capture
            && *capture_id > worker.last_legacy_capture_id
        {
            state.pending_reference_capture = Some(Arc::clone(frame));
            worker.last_legacy_capture_id = *capture_id;
        }
        if let Some((event_id, candidate)) = &device.candidate_event
            && *event_id > worker.last_candidate_event_id
        {
            state.snapshot.previews.reference_candidate = candidate.clone();
            worker.last_candidate_event_id = *event_id;
        }

        let first = app.recent_activity_first_id;
        if worker.last_activity_id.saturating_add(1) < first {
            state.record("Device activity entries were skipped");
            worker.last_activity_id = first.saturating_sub(1);
        }
        for (index, activity) in app.recent_activity.iter().enumerate() {
            let id = first.saturating_add(index as u64);
            if id > worker.last_activity_id {
                state.record(activity.clone());
                worker.last_activity_id = id;
            }
        }
    }

    pub(super) fn publish(&self, state: &mut RuntimeState, now: Instant) {
        let Some(worker) = &self.worker else {
            return;
        };
        let device = worker.snapshot();
        let Some(frame) = state.snapshot.previews.final_output.as_deref() else {
            return;
        };
        if let Some(publisher) = &device.publisher {
            match publisher.publish(frame) {
                Ok(()) => {
                    state.snapshot.publisher_component.mark_ready(now);
                    state.clear_warning(WarningSource::PublisherSink);
                }
                Err(error) => {
                    state
                        .snapshot
                        .publisher_component
                        .mark_failed(now, error.clone());
                    let already_reported =
                        state.warnings.entries[WarningSource::PublisherSink as usize].as_deref()
                            == Some(error.as_str());
                    state.set_warning(WarningSource::PublisherSink, error.clone());
                    if !already_reported {
                        state.record(format!("Frame publish failed: {error}"));
                    }
                }
            }
            state.record_publisher_diagnostics(publisher.diagnostics().into(), now);
        }
    }

    pub(super) fn reference_updated(&mut self, state: &mut RuntimeState) {
        if let Some(worker) = &self.worker {
            worker.update_reference(state.reference.clone());
        }
    }

    pub(super) fn signal_shutdown(&mut self) {
        if let Some(worker) = &mut self.worker {
            worker.signal_shutdown();
        }
    }

    pub(super) fn finish_shutdown(&mut self, timeout: Duration) -> bool {
        self.worker
            .as_mut()
            .is_none_or(|worker| worker.finish_shutdown(timeout))
    }
}
