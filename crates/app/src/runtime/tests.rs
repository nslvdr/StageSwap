use super::*;
use crate::runtime_clock::VirtualRuntimeClock;
use stageswap_core::OutputMode;

#[derive(Default)]
struct FakeComponentPort {
    ready_at: Option<Instant>,
    terminal_failure_at: Option<Instant>,
    starts: u32,
    restarts: u32,
}

#[derive(Default)]
struct FakeRuntimePorts {
    webcam: FakeComponentPort,
    screen: FakeComponentPort,
    publisher: FakeComponentPort,
    virtual_camera: FakeComponentPort,
}

impl RuntimePorts for FakeRuntimePorts {
    fn command(&mut self, command: &Command, _state: &mut RuntimeState) {
        match command {
            Command::Start => {
                self.webcam.starts += 1;
                self.screen.starts += 1;
                self.publisher.starts += 1;
                self.virtual_camera.starts += 1;
            }
            Command::Restart(stageswap_core::RestartTarget::Webcam) => {
                self.webcam.restarts += 1;
            }
            Command::Restart(stageswap_core::RestartTarget::ScreenCapture) => {
                self.screen.restarts += 1;
            }
            _ => {}
        }
    }

    fn refresh_inputs(&mut self, state: &mut RuntimeState, now: Instant) {
        for (component, status) in [
            (&self.webcam, &mut state.snapshot.webcam_component),
            (&self.screen, &mut state.snapshot.screen_component),
        ] {
            if component
                .terminal_failure_at
                .is_some_and(|failure_at| now >= failure_at)
            {
                status.mark_failed(now, "scripted terminal failure");
            } else if component.ready_at.is_some_and(|ready_at| now >= ready_at) {
                status.mark_ready(now);
            } else if status
                .first_frame_deadline
                .is_some_and(|deadline| now >= deadline)
            {
                status.mark_failed(now, "scripted first-frame deadline");
            }
        }
    }

    fn publish(&self, _state: &mut RuntimeState, _now: Instant) {}
}

#[derive(Default)]
struct ScriptedRuntimePorts {
    webcam: Option<Arc<Frame>>,
    screen: Option<Arc<Frame>>,
    published: Mutex<Vec<Arc<Frame>>>,
    refresh_count: usize,
    reference_updates: Vec<Option<GrayImage>>,
}

impl ScriptedRuntimePorts {
    fn published_frames(&self) -> Vec<Arc<Frame>> {
        self.published.lock().unwrap().clone()
    }
}

impl RuntimePorts for ScriptedRuntimePorts {
    fn reference_updated(&mut self, state: &mut RuntimeState) {
        self.reference_updates.push(state.reference.clone());
    }

    fn command(&mut self, _command: &Command, _state: &mut RuntimeState) {}

    fn refresh_inputs(&mut self, state: &mut RuntimeState, _now: Instant) {
        self.refresh_count += 1;
        state.snapshot.previews.webcam = self.webcam.clone();
        state.snapshot.previews.screen = self.screen.clone();
        state.snapshot.availability = SourceAvailability {
            camera_ready: self.webcam.is_some(),
            screen_ready: self.screen.is_some(),
        };
        state.snapshot.webcam_state = if self.webcam.is_some() {
            DeviceState::Ready
        } else {
            DeviceState::Unavailable
        };
        state.snapshot.screen_state = if self.screen.is_some() {
            DeviceState::Ready
        } else {
            DeviceState::Unavailable
        };
    }

    fn publish(&self, state: &mut RuntimeState, _now: Instant) {
        if let Some(frame) = state.snapshot.previews.final_output.as_ref() {
            self.published.lock().unwrap().push(Arc::clone(frame));
        }
    }
}

fn scripted_engine(
    state: RuntimeState,
    platform: ScriptedRuntimePorts,
    now: Instant,
) -> RuntimeEngine<ScriptedRuntimePorts> {
    let interval = Duration::from_nanos(1_000_000_000 / u64::from(PIPELINE_FPS));
    RuntimeEngine {
        state,
        platform,
        pacer: FramePacer::new(now, interval),
        reference_worker: None,
        reference_generation: 0,
        in_flight_candidate: None,
        reference_commit: None,
        last_deadline_log: now - Duration::from_secs(30),
    }
}

fn scripted_solid_frame(size: Size, color_bgra: u32, sequence: u64, now: Instant) -> Arc<Frame> {
    Arc::new(Frame::placeholder(size, color_bgra, sequence, 0, now))
}

fn running_scripted_state(now: Instant) -> RuntimeState {
    let mut state = RuntimeState::new_at(
        AppConfig {
            start_automatically: false,
            ..AppConfig::default()
        },
        now,
    );
    state.snapshot.run_state = RunState::Running;
    state
}

fn latest_published(platform: &ScriptedRuntimePorts) -> Arc<Frame> {
    platform
        .published_frames()
        .pop()
        .expect("scripted runtime should publish an output frame")
}

fn reference_for(frame: &Frame) -> GrayImage {
    gray_thumbnail(frame).expect("scripted reference frame should be valid")
}

fn monitor(display_name: &str, label: &str) -> MonitorDescriptor {
    MonitorDescriptor {
        display_name: display_name.into(),
        label: label.into(),
        ..MonitorDescriptor::default()
    }
}

#[test]
fn flow_discovery_shutdown_serializes_with_wait_and_wakes_idle_workers() {
    let slot = Arc::new(CoalescingSlot::<()>::new());
    let stop = Arc::new(AtomicBool::new(false));
    let guard = slot.pending.lock().unwrap();
    let (started_tx, started_rx) = mpsc::sync_channel(0);
    let (done_tx, done_rx) = mpsc::sync_channel(0);
    let producer_slot = Arc::clone(&slot);
    let producer_stop = Arc::clone(&stop);
    let worker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        producer_slot.stop(&producer_stop);
        done_tx.send(()).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
    assert!(!stop.load(Ordering::Acquire));
    let (guard, timeout) = slot
        .changed
        .wait_timeout_while(guard, Duration::from_secs(2), |_| {
            !stop.load(Ordering::Acquire)
        })
        .unwrap();
    assert!(!timeout.timed_out());
    drop(guard);
    done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.join().unwrap();
    assert_eq!(slot.wait_take(&stop), None);
}

#[test]
fn contract_monitor_matching_requires_verified_sdr() {
    assert!(monitor_color_allows_matching(Ok(false)));
    assert!(!monitor_color_allows_matching(Ok(true)));
    assert!(!monitor_color_allows_matching(Err(
        "display query failed".into()
    )));
}

#[test]
fn flow_reference_cancellation_preserves_disk_and_late_commit_updates_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reference.png");
    let now = Instant::now();
    let original = scripted_solid_frame(PIPELINE_SIZE, 0xff000000, 1, now);
    let candidate = scripted_solid_frame(PIPELINE_SIZE, 0xffffffff, 2, now);
    persist_frame_atomic(&original, &path).unwrap();
    let mut state = running_scripted_state(now);
    state.config.reference_image_path = path.display().to_string();
    let mut engine = scripted_engine(state, ScriptedRuntimePorts::default(), now);
    let commit = Arc::new(ReferenceCommit::default());
    engine.reference_commit = Some(Arc::clone(&commit));
    engine.in_flight_candidate = Some((candidate.sequence, 1));
    engine
        .state
        .stage_reference_candidate(Arc::clone(&candidate));
    assert!(engine.command(Command::DiscardReferenceCandidate));
    assert!(commit.is_cancelled());
    assert!(engine.state.snapshot.previews.reference_candidate.is_none());
    let cancelled = execute_reference_job(ReferenceJob {
        generation: 1,
        commit: Some(commit),
        kind: ReferenceJobKind::Persist {
            frame: Arc::clone(&candidate),
            path: path.clone(),
            confirmed: true,
        },
    });
    assert!(cancelled.result.is_err());
    assert_eq!(
        image::open(&path).unwrap().to_rgba8().get_pixel(0, 0).0,
        [0, 0, 0, 255]
    );
    assert!(!pending_reference_path(&path).exists());

    // Once replacement is claimed, cancellation cannot undo a committed save.
    let commit = Arc::new(ReferenceCommit::default());
    commit.claim().unwrap();
    commit.cancel();
    let committed = execute_reference_job(ReferenceJob {
        generation: 1,
        commit: Some(commit),
        kind: ReferenceJobKind::Persist {
            frame: candidate,
            path: path.clone(),
            confirmed: true,
        },
    });
    assert!(committed.result.is_ok());
    let (result_sender, results) = mpsc::sync_channel(1);
    let (_done_sender, done) = mpsc::sync_channel(1);
    engine.reference_worker = Some(ReferenceWorker {
        jobs: None,
        results,
        stop: Arc::new(AtomicBool::new(false)),
        done,
        worker: None,
    });
    engine.reference_generation = 2;
    result_sender.send(committed).ok().unwrap();
    engine.poll_reference_results(now);
    assert!(engine.state.reference.is_some());
    assert_eq!(
        &engine
            .state
            .snapshot
            .previews
            .reference
            .as_ref()
            .unwrap()
            .pixels()[..4],
        &[255; 4]
    );
    assert_eq!(
        image::open(&path).unwrap().to_rgba8().get_pixel(0, 0).0,
        [255; 4]
    );
}

#[test]
fn flow_runtime_engine_uses_virtual_time_for_delayed_frames_and_deadlines() {
    let start = Instant::now();
    let clock = VirtualRuntimeClock::new(start);
    let mut state = RuntimeState::new_at(AppConfig::default(), start);
    state
        .snapshot
        .webcam_component
        .waiting_for_first_frame(start, start + WEBCAM_FIRST_FRAME_TIMEOUT);
    state
        .snapshot
        .screen_component
        .waiting_for_first_frame(start, start + SCREEN_FIRST_FRAME_TIMEOUT);
    let platform = FakeRuntimePorts {
        webcam: FakeComponentPort {
            ready_at: Some(start + Duration::from_millis(2_900)),
            ..FakeComponentPort::default()
        },
        ..FakeRuntimePorts::default()
    };
    let mut engine = RuntimeEngine::from_parts(state, platform, start);
    assert!(engine.command(Command::Start));
    assert_eq!(engine.platform.webcam.starts, 1);
    assert_eq!(engine.platform.screen.starts, 1);
    assert_eq!(engine.platform.publisher.starts, 1);
    assert_eq!(engine.platform.virtual_camera.starts, 1);
    assert!(engine.command(Command::Restart(stageswap_core::RestartTarget::Webcam)));
    assert_eq!(engine.platform.webcam.restarts, 1);

    clock.advance(Duration::from_secs(2));
    assert!(engine.step(clock.now()));
    assert!(engine.state.snapshot.output_deadline_misses > 0);
    assert_eq!(
        engine.state.snapshot.screen_component.lifecycle,
        ComponentLifecycle::Failed
    );
    assert_eq!(
        engine.state.snapshot.webcam_component.lifecycle,
        ComponentLifecycle::WaitingForFirstFrame
    );

    clock.advance(Duration::from_millis(900));
    assert!(engine.step(clock.now()));
    assert_eq!(
        engine.state.snapshot.webcam_component.lifecycle,
        ComponentLifecycle::Ready
    );
}

#[test]
fn flow_scripted_pipeline_debounces_detection_and_composes_reversible_transition() {
    let start = Instant::now();
    let clock = VirtualRuntimeClock::new(start);
    let camera = scripted_solid_frame(PIPELINE_SIZE, 0xff20_4080, 1, start);
    let matching_screen = scripted_solid_frame(Size::new(4, 2), 0xffff_ffff, 2, start);
    let mismatching_screen = scripted_solid_frame(Size::new(4, 2), 0xff00_ff00, 3, start);
    let camera_pixels = camera.pixels_arc();
    let mut engine = scripted_engine(
        running_scripted_state(start),
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&matching_screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    engine.state.reference = Some(reference_for(&matching_screen));

    assert!(engine.step(clock.now()));
    assert_eq!(engine.state.detector.counters(), (1, 0));
    let first = latest_published(&engine.platform);
    assert_eq!(first.pixels(), camera.pixels());
    assert!(Arc::ptr_eq(&first.pixels_arc(), &camera_pixels));

    clock.advance(Duration::from_millis(249));
    assert!(engine.step(clock.now()));
    assert_eq!(
        engine.state.detector.counters(),
        (1, 0),
        "detection must not run before the 250 ms interval"
    );

    for _ in 0..4 {
        clock.advance(Duration::from_millis(250));
        assert!(engine.step(clock.now()));
    }
    assert_eq!(
        engine.state.snapshot.detection,
        stageswap_core::DetectionState::Matching
    );
    assert_eq!(engine.state.detector.counters(), (5, 0));
    assert_eq!(engine.state.snapshot.actual_output, Source::Camera);

    engine.platform.screen = Some(Arc::clone(&mismatching_screen));
    for _ in 0..3 {
        clock.advance(Duration::from_millis(250));
        assert!(engine.step(clock.now()));
    }
    assert_eq!(
        engine.state.snapshot.detection,
        stageswap_core::DetectionState::NotMatching
    );
    assert_eq!(engine.state.snapshot.actual_output, Source::Screen);
    assert!(engine.state.snapshot.transition.active);
    assert!(engine.state.snapshot.transition.screen_mix.abs() < f64::EPSILON);

    clock.advance(Duration::from_millis(250));
    assert!(engine.step(clock.now()));
    let halfway = engine.state.snapshot.transition;
    assert!((halfway.screen_mix - 0.5).abs() < 0.001);
    let halfway_output = latest_published(&engine.platform);
    assert_ne!(halfway_output.pixels(), camera.pixels());
    assert_eq!(halfway_output.sequence, engine.state.sequence);
    let expected_timestamp = i64::try_from(
        clock
            .now()
            .duration_since(engine.state.started_at)
            .as_nanos()
            / 100,
    )
    .unwrap();
    assert_eq!(halfway_output.timestamp_100ns, expected_timestamp);
    assert_eq!(halfway_output.received_at, clock.now());

    clock.advance(Duration::from_millis(250));
    assert!(engine.step(clock.now()));
    let completed = engine.state.snapshot.transition;
    assert_eq!(completed.logical_source, Source::Screen);
    assert!((completed.screen_mix - 1.0).abs() < f64::EPSILON);
    assert!(!completed.active);
    let screen_output = latest_published(&engine.platform);
    assert_eq!(&screen_output.pixels()[..4], &[0, 0, 0, 255]);
    let center = ((PIPELINE_SIZE.height as usize / 2) * PIPELINE_SIZE.width as usize
        + PIPELINE_SIZE.width as usize / 2)
        * 4;
    assert_eq!(
        &screen_output.pixels()[center..center + 4],
        &[0, 255, 0, 255]
    );
    assert_eq!(camera.pixels_arc().as_ref(), camera_pixels.as_ref());

    let reversal_start = clock.now() + Duration::from_secs(1);
    let mut reversal = scripted_engine(
        running_scripted_state(reversal_start),
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&mismatching_screen)),
            ..ScriptedRuntimePorts::default()
        },
        reversal_start,
    );
    reversal.state.last_detection = reversal_start;
    reversal.state.snapshot.detection = stageswap_core::DetectionState::NotMatching;
    assert!(reversal.step(reversal_start));
    let halfway_at = reversal_start + Duration::from_millis(250);
    reversal.state.last_detection = halfway_at;
    assert!(reversal.step(halfway_at));
    assert!((reversal.state.snapshot.transition.screen_mix - 0.5).abs() < 0.001);

    reversal.state.snapshot.detection = stageswap_core::DetectionState::Matching;
    reversal.state.last_detection = halfway_at;
    let reverse_at = halfway_at + Duration::from_millis(34);
    assert!(reversal.step(reverse_at));
    let reversed = reversal.state.snapshot.transition;
    assert!(reversed.reversed);
    assert!(reversed.active);
    assert!(reversed.screen_mix > 0.5 && reversed.screen_mix < 0.6);
    assert_eq!(reversal.state.snapshot.actual_output, Source::Camera);
}

#[test]
fn flow_still_non_reference_screen_activates_and_reverses_pip_in_auto_only() {
    let start = Instant::now();
    let camera = scripted_solid_frame(PIPELINE_SIZE, 0xff20_4080, 1, start);
    let still_screen = scripted_solid_frame(PIPELINE_SIZE, 0xff00_ff00, 2, start);
    let moved_screen = scripted_solid_frame(PIPELINE_SIZE, 0xffff_0000, 3, start);
    let reference = scripted_solid_frame(PIPELINE_SIZE, 0xffff_ffff, 4, start);
    let config = AppConfig {
        still_image_pip_enabled: true,
        still_image_pip_delay_seconds: 30,
        ..AppConfig::default()
    };
    let mut state = RuntimeState::new_at(config, start);
    state.snapshot.run_state = RunState::Running;
    state.reference = Some(reference_for(&reference));
    let mut engine = scripted_engine(
        state,
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&still_screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );

    assert!(engine.step(start));
    assert!(!engine.state.still_image_detector.active());
    assert!(engine.step(start + Duration::from_secs(29)));
    assert!(!engine.state.still_image_detector.active());
    assert!(engine.step(start + Duration::from_secs(30)));
    assert!(engine.state.still_image_detector.active());
    assert_eq!(engine.state.snapshot.actual_output, Source::Camera);
    assert_eq!(engine.state.snapshot.still_image_pip_mix, 0.0);

    assert!(engine.step(start + Duration::from_millis(30_500)));
    assert_eq!(engine.state.snapshot.still_image_pip_mix, 1.0);
    assert!(engine.state.snapshot.still_image_pip_active);
    assert_eq!(
        engine.state.snapshot.output_layout,
        OutputLayout::WebcamMainScreenPip
    );
    let output = latest_published(&engine.platform);
    let inset_center = (596 * PIPELINE_SIZE.width as usize + 208) * 4;
    assert_eq!(
        &output.pixels()[inset_center..inset_center + 4],
        &[0, 255, 0, 255]
    );

    engine.platform.screen = Some(Arc::clone(&moved_screen));
    assert!(engine.step(start + Duration::from_millis(30_750)));
    assert!(!engine.state.still_image_detector.active());
    assert_eq!(engine.state.snapshot.actual_output, Source::Screen);
    assert!(engine.step(start + Duration::from_millis(31_250)));
    assert_eq!(engine.state.snapshot.still_image_pip_mix, 0.0);
    assert_eq!(engine.state.snapshot.output_layout, OutputLayout::Screen);

    engine
        .state
        .apply_command(Command::SetMode(OutputMode::ForceScreen), false);
    engine.platform.screen = Some(still_screen);
    assert!(engine.step(start + Duration::from_secs(62)));
    assert!(engine.step(start + Duration::from_secs(93)));
    assert!(!engine.state.still_image_detector.active());
    assert_eq!(engine.state.snapshot.output_layout, OutputLayout::Screen);
}

#[test]
fn flow_still_image_pip_supports_screen_main_and_requires_both_live_sources() {
    let start = Instant::now();
    let camera = scripted_solid_frame(PIPELINE_SIZE, 0xff20_4080, 1, start);
    let screen = scripted_solid_frame(PIPELINE_SIZE, 0xff00_ff00, 2, start);
    let reference = scripted_solid_frame(PIPELINE_SIZE, 0xffff_ffff, 3, start);
    let config = AppConfig {
        still_image_pip_enabled: true,
        still_image_pip_delay_seconds: 30,
        still_image_pip_layout: StillImagePipLayout::ScreenMain,
        ..AppConfig::default()
    };
    let mut state = RuntimeState::new_at(config, start);
    state.snapshot.run_state = RunState::Running;
    state.reference = Some(reference_for(&reference));
    let mut engine = scripted_engine(
        state,
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    assert!(engine.step(start));
    assert!(engine.step(start + Duration::from_secs(30)));
    assert!(engine.step(start + Duration::from_millis(30_500)));
    assert_eq!(
        engine.state.snapshot.output_layout,
        OutputLayout::ScreenMainWebcamPip
    );
    assert_eq!(engine.state.snapshot.actual_output, Source::Screen);

    engine.platform.webcam = None;
    assert!(engine.step(start + Duration::from_secs(31)));
    assert!(!engine.state.still_image_detector.active());
    assert!(engine.step(start + Duration::from_millis(31_500)));
    assert_eq!(engine.state.snapshot.still_image_pip_mix, 0.0);
    assert_eq!(engine.state.snapshot.output_layout, OutputLayout::Screen);
}

#[test]
fn flow_force_pip_bypasses_the_timer_and_automatic_enablement() {
    let start = Instant::now();
    let camera = scripted_solid_frame(PIPELINE_SIZE, 0xff20_4080, 1, start);
    let screen = scripted_solid_frame(PIPELINE_SIZE, 0xff00_ff00, 2, start);
    let config = AppConfig {
        still_image_pip_enabled: false,
        still_image_pip_layout: StillImagePipLayout::WebcamMain,
        still_image_pip_size: StillImagePipSize::Large,
        output_mode: OutputMode::ForcePip,
        ..AppConfig::default()
    };
    let mut state = RuntimeState::new_at(config, start);
    state.snapshot.run_state = RunState::Running;
    let mut engine = scripted_engine(
        state,
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );

    assert!(engine.step(start));
    assert!(!engine.state.still_image_detector.active());
    assert_eq!(engine.state.snapshot.actual_output, Source::Camera);
    assert!(engine.step(start + Duration::from_millis(500)));
    assert_eq!(engine.state.snapshot.still_image_pip_mix, 1.0);
    assert_eq!(
        engine.state.snapshot.output_layout,
        OutputLayout::WebcamMainScreenPip
    );
    let output = latest_published(&engine.platform);
    let large_only_point = (578 * PIPELINE_SIZE.width as usize + 430) * 4;
    assert_eq!(
        &output.pixels()[large_only_point..large_only_point + 4],
        &[0, 255, 0, 255]
    );

    engine.platform.screen = None;
    assert!(engine.step(start + Duration::from_millis(750)));
    assert_eq!(engine.state.snapshot.actual_output, Source::Camera);
    assert!(engine.step(start + Duration::from_millis(1_250)));
    assert_eq!(engine.state.snapshot.still_image_pip_mix, 0.0);
    assert_eq!(engine.state.snapshot.output_layout, OutputLayout::Camera);
}

#[test]
fn flow_scripted_runtime_modes_and_missing_sources_use_safe_fallbacks() {
    let start = Instant::now();
    let camera = scripted_solid_frame(PIPELINE_SIZE, 0xff20_4080, 1, start);
    let screen = scripted_solid_frame(PIPELINE_SIZE, 0xffff_ffff, 2, start);

    let mut automatic = scripted_engine(
        running_scripted_state(start),
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    automatic.state.last_detection = start;
    automatic.state.snapshot.detection = stageswap_core::DetectionState::NotMatching;
    assert!(automatic.step(start));
    assert_eq!(automatic.state.snapshot.actual_output, Source::Screen);
    assert!(automatic.state.snapshot.transition.active);
    automatic.state.last_detection = start + Duration::from_millis(500);
    assert!(automatic.step(start + Duration::from_millis(500)));
    assert_eq!(automatic.state.snapshot.actual_output, Source::Screen);

    let mut forced_camera = scripted_engine(
        running_scripted_state(start),
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    forced_camera.state.last_detection = start;
    forced_camera.state.snapshot.mode = OutputMode::ForceCamera;
    forced_camera.state.snapshot.detection = stageswap_core::DetectionState::NotMatching;
    assert!(forced_camera.step(start));
    assert_eq!(forced_camera.state.snapshot.actual_output, Source::Camera);
    assert_eq!(
        latest_published(&forced_camera.platform).pixels(),
        camera.pixels()
    );

    let mut forced_screen_without_camera = scripted_engine(
        running_scripted_state(start),
        ScriptedRuntimePorts {
            webcam: None,
            screen: Some(Arc::clone(&screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    forced_screen_without_camera.state.last_detection = start;
    forced_screen_without_camera.state.snapshot.mode = OutputMode::ForceScreen;
    assert!(forced_screen_without_camera.step(start));
    forced_screen_without_camera.state.last_detection = start + Duration::from_millis(500);
    assert!(forced_screen_without_camera.step(start + Duration::from_millis(500)));
    assert_eq!(
        forced_screen_without_camera.state.snapshot.actual_output,
        Source::Screen
    );
    assert_eq!(
        latest_published(&forced_screen_without_camera.platform).pixels(),
        screen.pixels()
    );

    let mut automatic_without_screen = scripted_engine(
        running_scripted_state(start),
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: None,
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    automatic_without_screen.state.last_detection = start;
    automatic_without_screen.state.snapshot.detection = stageswap_core::DetectionState::NotMatching;
    assert!(automatic_without_screen.step(start));
    assert_eq!(
        automatic_without_screen.state.snapshot.actual_output,
        Source::Camera
    );
    assert_eq!(
        latest_published(&automatic_without_screen.platform).pixels(),
        camera.pixels()
    );

    let mut forced_camera_without_camera = scripted_engine(
        running_scripted_state(start),
        ScriptedRuntimePorts {
            webcam: None,
            screen: Some(Arc::clone(&screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    forced_camera_without_camera.state.last_detection = start;
    forced_camera_without_camera.state.snapshot.mode = OutputMode::ForceCamera;
    assert!(forced_camera_without_camera.step(start));
    assert_eq!(
        forced_camera_without_camera.state.snapshot.actual_output,
        Source::Placeholder
    );
    let placeholder = latest_published(&forced_camera_without_camera.platform);
    assert!(
        placeholder
            .pixels()
            .chunks_exact(4)
            .all(|pixel| pixel == [0x19, 0x17, 0x17, 0xff])
    );

    let mut stopped = running_scripted_state(start);
    stopped.snapshot.run_state = RunState::Stopped;
    stopped.snapshot.mode = OutputMode::ForceScreen;
    let mut stopped_engine = scripted_engine(
        stopped,
        ScriptedRuntimePorts {
            webcam: Some(Arc::clone(&camera)),
            screen: Some(Arc::clone(&screen)),
            ..ScriptedRuntimePorts::default()
        },
        start,
    );
    assert!(stopped_engine.step(start));
    assert_eq!(
        stopped_engine.state.snapshot.actual_output,
        Source::Placeholder
    );
    assert_eq!(
        latest_published(&stopped_engine.platform).pixels(),
        stageswap_core::off_frame_pixels().as_ref()
    );
}

#[test]
fn flow_shutdown_timeout_detaches_worker_and_allows_controlled_release() {
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let (done_sender, done_receiver) = mpsc::sync_channel(1);
    let mut worker = Some(thread::spawn(move || {
        worker_barrier.wait();
        let _ = done_sender.send(());
    }));

    assert!(!finish_worker_shutdown(
        &done_receiver,
        &mut worker,
        Duration::from_millis(1)
    ));
    assert!(worker.is_none());
    barrier.wait();
    assert!(done_receiver.recv_timeout(Duration::from_secs(1)).is_ok());
}

#[test]
fn flow_reference_worker_queue_reports_busy_without_a_consumer() {
    let (job_sender, _job_receiver) = mpsc::sync_channel(REFERENCE_JOB_CAPACITY);
    let (_result_sender, result_receiver) = mpsc::sync_channel(REFERENCE_JOB_CAPACITY);
    let (_done_sender, done_receiver) = mpsc::sync_channel(1);
    let mut worker = ReferenceWorker {
        jobs: Some(job_sender),
        results: result_receiver,
        stop: Arc::new(AtomicBool::new(false)),
        done: done_receiver,
        worker: None,
    };
    let job = || ReferenceJob {
        commit: None,
        generation: 1,
        kind: ReferenceJobKind::Load {
            path: std::path::PathBuf::from("unused-reference.png"),
        },
    };

    for _ in 0..REFERENCE_JOB_CAPACITY {
        assert!(worker.submit(job()).is_ok());
    }
    assert_eq!(
        worker.submit(job()).unwrap_err(),
        "reference worker is busy"
    );
    worker.signal_shutdown();
    assert!(worker.stop.load(Ordering::Acquire));
    assert!(worker.jobs.is_none());
}

#[test]
fn flow_reference_worker_retains_results_under_backpressure_and_can_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    let mut worker = ReferenceWorker::start().unwrap();
    let submit_batch = |worker: &ReferenceWorker| {
        for generation in 1..=6 {
            worker
                .jobs
                .as_ref()
                .unwrap()
                .send(ReferenceJob {
                    generation,
                    commit: None,
                    kind: ReferenceJobKind::Load {
                        path: directory.path().join("missing.png"),
                    },
                })
                .unwrap();
        }
    };
    submit_batch(&worker);
    // No consumer drains the four result slots while the worker completes
    // this batch; reaching a full queue must not disconnect it.
    assert!(matches!(
        worker.done.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    for generation in 1..=6 {
        let result = worker.results.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(result.generation, generation);
        assert!(result.result.is_err());
    }
    submit_batch(&worker);
    assert!(matches!(
        worker.done.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    worker.signal_shutdown();
    assert!(worker.finish_shutdown(Duration::from_secs(1)));
}

#[test]
fn flow_reference_worker_shutdown_closes_jobs_and_joins_without_sleeping() {
    let (job_sender, job_receiver) = mpsc::sync_channel(REFERENCE_JOB_CAPACITY);
    let (result_sender, result_receiver) = mpsc::sync_channel(REFERENCE_JOB_CAPACITY);
    let (done_sender, done_receiver) = mpsc::sync_channel(1);
    let worker_thread = thread::spawn(move || {
        drop(result_sender);
        drop(job_receiver);
        done_sender.send(()).unwrap();
    });
    let mut worker = ReferenceWorker {
        jobs: Some(job_sender),
        results: result_receiver,
        stop: Arc::new(AtomicBool::new(false)),
        done: done_receiver,
        worker: Some(worker_thread),
    };

    worker.signal_shutdown();
    assert!(worker.finish_shutdown(Duration::from_secs(1)));
    assert!(worker.worker.is_none());
}

#[test]
fn flow_obsolete_reference_results_are_ignored_but_current_results_install() {
    let start = Instant::now();
    let frame = scripted_solid_frame(Size::new(160, 90), 0xff40_4040, 1, start);
    let data = reference_data_from_frame(&frame).unwrap();
    let (result_sender, result_receiver) = mpsc::sync_channel(REFERENCE_JOB_CAPACITY);
    let (_done_sender, done_receiver) = mpsc::sync_channel(1);
    let worker = ReferenceWorker {
        jobs: None,
        results: result_receiver,
        stop: Arc::new(AtomicBool::new(false)),
        done: done_receiver,
        worker: None,
    };
    let mut engine = scripted_engine(
        running_scripted_state(start),
        ScriptedRuntimePorts::default(),
        start,
    );
    engine.reference_worker = Some(worker);
    engine.reference_generation = 2;

    result_sender
        .send(ReferenceResult {
            persisted_path: None,
            generation: 1,
            action: ReferenceAction::Load,
            result: Ok(data),
        })
        .unwrap();
    engine.poll_reference_results(start);
    assert!(engine.state.reference.is_none());
    assert!(
        engine
            .state
            .snapshot
            .recent_activity
            .iter()
            .any(|message| message.contains("Ignored obsolete reference result 1"))
    );

    let current_data = reference_data_from_frame(&frame).unwrap();
    result_sender
        .send(ReferenceResult {
            persisted_path: None,
            generation: 2,
            action: ReferenceAction::Load,
            result: Ok(current_data),
        })
        .unwrap();
    engine.poll_reference_results(start + Duration::from_millis(1));
    assert!(engine.state.reference.is_some());
    assert!(engine.state.snapshot.previews.reference.is_some());
    assert_eq!(
        engine
            .state
            .snapshot
            .recent_activity
            .last()
            .map(String::as_str),
        Some("Reference loaded")
    );
    assert!(engine.platform.reference_updates.last().unwrap().is_some());
    result_sender
        .send(ReferenceResult {
            persisted_path: None,
            generation: 2,
            action: ReferenceAction::Load,
            result: Err("reference was removed".into()),
        })
        .unwrap();
    engine.poll_reference_results(start + Duration::from_millis(2));
    assert!(engine.state.reference.is_none());
    assert!(engine.state.snapshot.previews.reference.is_none());
    assert_eq!(engine.platform.reference_updates.last(), Some(&None));
}

#[test]
fn flow_session_disconnect_clears_capture_and_preserves_user_intent() {
    #[derive(Default)]
    struct Capture {
        stops: u32,
    }
    impl stageswap_windows::VideoInput for Capture {
        fn enumerate(&self) -> Result<Vec<stageswap_windows::InputDevice>, String> {
            Ok(Vec::new())
        }
        fn start(&mut self, _id: &str) -> Result<(), stageswap_windows::WebcamError> {
            panic!("suspension must not open capture")
        }
        fn stop(&mut self) {
            self.stops += 1;
        }
        fn latest_frame(&self) -> Option<Arc<Frame>> {
            None
        }
    }
    impl stageswap_windows::ScreenInput for Capture {
        fn enumerate(&self) -> Result<Vec<MonitorDescriptor>, String> {
            Ok(Vec::new())
        }
        fn start(
            &mut self,
            _monitor: &MonitorDescriptor,
            _cursor: bool,
        ) -> Result<(), stageswap_windows::ScreenError> {
            panic!("suspension must not open capture")
        }
        fn stop(&mut self) {
            self.stops += 1;
        }
        fn latest_frame(&self) -> Option<Arc<Frame>> {
            None
        }
    }
    let mut webcam = Capture::default();
    let mut screen = Capture::default();
    let now = Instant::now();
    let mut state = running_scripted_state(now);
    state.apply_command(Command::SetMode(OutputMode::ForceScreen), false);
    state.config.selected_video_device_id = "saved-webcam".into();
    state.config.selected_monitor_label = "saved-screen".into();
    state.snapshot.previews.webcam = Some(scripted_solid_frame(PIPELINE_SIZE, 0xff123456, 1, now));
    state.snapshot.previews.screen = state.snapshot.previews.webcam.clone();
    state.snapshot.availability = SourceAvailability {
        camera_ready: true,
        screen_ready: true,
    };
    state
        .snapshot
        .webcam_component
        .waiting_for_first_frame(now, now + WEBCAM_FIRST_FRAME_TIMEOUT);
    state
        .snapshot
        .screen_component
        .waiting_for_first_frame(now, now + SCREEN_FIRST_FRAME_TIMEOUT);
    state.release_session_capture(&mut webcam, &mut screen, now);
    assert_eq!((webcam.stops, screen.stops), (1, 1));
    assert!(state.snapshot.previews.webcam.is_none());
    assert!(state.snapshot.previews.screen.is_none());
    assert_eq!(state.snapshot.availability, SourceAvailability::default());
    assert_eq!(
        state.snapshot.webcam_component.lifecycle,
        ComponentLifecycle::Stopped
    );
    assert_eq!(
        state.snapshot.screen_component.lifecycle,
        ComponentLifecycle::Stopped
    );
    assert!(
        state
            .snapshot
            .webcam_component
            .first_frame_deadline
            .is_none()
    );
    assert!(
        state
            .snapshot
            .screen_component
            .first_frame_deadline
            .is_none()
    );
    assert_eq!(state.snapshot.run_state, RunState::Running);
    assert_eq!(state.snapshot.mode, OutputMode::ForceScreen);

    // Start and mode commands cannot bypass suspension; idle frames keep advancing.
    state.apply_command(Command::Start, false);
    state.apply_command(Command::SetMode(OutputMode::ForceCamera), false);
    let previous = state
        .snapshot
        .previews
        .final_output
        .as_ref()
        .unwrap()
        .sequence;
    state.tick(now + Duration::from_millis(34));
    let output = state.snapshot.previews.final_output.as_ref().unwrap();
    assert!(output.sequence > previous);
    assert_eq!(
        output.pixels(),
        off_frame(FrameMetadata {
            sequence: 1,
            timestamp_100ns: 0,
            received_at: now,
        })
        .pixels()
    );

    state.set_capture_suspended(false, now + Duration::from_secs(1));
    assert_eq!(state.snapshot.run_state, RunState::Running);
    assert_eq!(state.snapshot.mode, OutputMode::ForceCamera);
    assert_eq!(state.config.selected_video_device_id, "saved-webcam");
    assert_eq!(state.config.selected_monitor_label, "saved-screen");
    assert_eq!(state.snapshot.availability, SourceAvailability::default());
    // A fresh capture survives repeated observations of an already active session.
    state.snapshot.previews.webcam = Some(scripted_solid_frame(PIPELINE_SIZE, 0xff123456, 2, now));
    state.set_capture_suspended(false, now + Duration::from_secs(2));
    assert!(state.snapshot.previews.webcam.is_some());
    state.set_capture_suspended(true, now + Duration::from_secs(3));
    state.apply_command(Command::Stop, false);
    state.set_capture_suspended(false, now + Duration::from_secs(4));
    assert_eq!(state.snapshot.run_state, RunState::Stopped);
    state.tick(now + Duration::from_secs(4));
    assert_eq!(state.snapshot.actual_output, Source::Placeholder);
}

#[test]
fn flow_initial_monitor_selection_prefers_saved_then_secondary_then_primary() {
    let monitors = [
        monitor("primary", "Desk"),
        monitor("secondary", "Stage"),
        monitor("third", "Stage"),
    ];
    assert_eq!(
        choose_initial_monitor("Desk", &monitors)
            .unwrap()
            .display_name,
        "primary"
    );
    assert_eq!(
        choose_initial_monitor("Stage", &monitors)
            .unwrap()
            .display_name,
        "secondary"
    );
    assert_eq!(
        choose_initial_monitor("Missing", &monitors)
            .unwrap()
            .display_name,
        "secondary"
    );
    assert_eq!(
        choose_initial_monitor("", &monitors).unwrap().display_name,
        "secondary"
    );

    let primary = [monitor("primary", "Desk")];
    assert_eq!(
        choose_initial_monitor("Missing", &primary)
            .unwrap()
            .display_name,
        "primary"
    );
    assert!(choose_initial_monitor("Missing", &[]).is_none());
}

#[test]
fn flow_fps_trackers_measure_output_and_capture_stalls() {
    let start = Instant::now();
    let mut output = OutputFpsTracker::default();
    let mut output_reading = None;
    for frame in 0..=30 {
        output_reading = output.observe(start + Duration::from_secs_f64(frame as f64 / 30.0));
    }
    assert_eq!(output_reading, Some(30));
    assert_eq!(output.observe(start + Duration::from_secs(3)), None);

    let mut source = SourceFpsTracker::default();
    let mut source_reading = None;
    for sequence in 1..=31 {
        let received_at = start + Duration::from_secs_f64((sequence - 1) as f64 / 30.0);
        let frame = Frame::placeholder(Size::new(1, 1), 0xff00_0000, sequence, 0, received_at);
        source_reading = source.observe(Some(&frame), received_at);
    }
    assert_eq!(source_reading, Some(30));
    let last = Frame::placeholder(
        Size::new(1, 1),
        0xff00_0000,
        31,
        0,
        start + Duration::from_secs(1),
    );
    assert_eq!(
        source.observe(Some(&last), start + Duration::from_secs(2)),
        Some(0)
    );
}

#[test]
fn flow_disco_effect_changes_only_rgb_and_preserves_frame_metadata() {
    let now = Instant::now();
    let source = Frame::placeholder(Size::new(32, 18), 0xff30_6090, 17, 42, now);
    let mut effect = DiscoEffect::new(source.size);

    let first = effect.apply(&source, Duration::ZERO);
    let later = effect.apply(&source, Duration::from_millis(990));

    assert_eq!(first.size, source.size);
    assert_eq!(first.stride, source.stride);
    assert_eq!(first.sequence, source.sequence);
    assert_eq!(first.timestamp_100ns, source.timestamp_100ns);
    assert_eq!(first.received_at, source.received_at);
    assert!(first.pixels().chunks_exact(4).all(|pixel| pixel[3] == 0xff));
    assert!(later.pixels().chunks_exact(4).all(|pixel| pixel[3] == 0xff));
    assert_ne!(first.pixels(), source.pixels());
    assert_ne!(later.pixels(), first.pixels());
}

#[test]
fn flow_disco_flash_pattern_has_primary_secondary_and_major_hits() {
    assert_eq!(disco_flash_lift(0), 38);
    assert_eq!(disco_flash_lift(1), 26);
    assert_eq!(disco_flash_lift(8), 22);
    assert_eq!(disco_flash_lift(9), 8);
    assert_eq!(disco_flash_lift(12), 0);
    assert_eq!(disco_flash_lift(36), 30);
}

#[cfg(not(debug_assertions))]
#[test]
fn flow_disco_effect_keeps_release_720p_processing_inside_the_frame_budget() {
    let now = Instant::now();
    let source = Frame::placeholder(PIPELINE_SIZE, 0xff30_6090, 1, 0, now);
    let mut effect = DiscoEffect::new(PIPELINE_SIZE);
    let started_at = Instant::now();
    for sequence in 0..300 {
        std::hint::black_box(effect.apply(
            &source,
            Duration::from_nanos(sequence * 1_000_000_000 / u64::from(PIPELINE_FPS)),
        ));
    }
    assert!(
        started_at.elapsed() <= Duration::from_secs(10),
        "300 disco frames exceeded the 30 fps processing budget"
    );
}

fn frame_with_bright_pixels(bright_pixels: usize) -> Frame {
    let size = Size::new(160, 90);
    let mut pixels = vec![0; size.width as usize * size.height as usize * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let value = u8::from(index < bright_pixels) * 255;
        pixel.copy_from_slice(&[value, value, value, 255]);
    }
    Frame::new(pixels.into(), size, size.width * 4, 1, 0, Instant::now()).unwrap()
}

fn solid_frame(value: u8) -> Frame {
    let size = Size::new(160, 90);
    let mut pixels = vec![value; size.width as usize * size.height as usize * 4];
    for alpha in pixels.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
    Frame::new(pixels.into(), size, size.width * 4, 1, 0, Instant::now()).unwrap()
}

#[test]
fn flow_black_screen_detection_allows_small_cursor_but_rejects_visible_content() {
    assert!(is_nearly_black(&frame_with_bright_pixels(0)));
    assert!(is_nearly_black(&solid_frame(BLACK_LUMA_THRESHOLD)));
    assert!(!is_nearly_black(&solid_frame(BLACK_LUMA_THRESHOLD + 1)));
    assert!(is_nearly_black(&frame_with_bright_pixels(100)));
    assert!(!is_nearly_black(&frame_with_bright_pixels(200)));
}

#[test]
fn flow_illustrated_jw_library_idle_display_is_not_treated_as_failed_capture() {
    let mut image =
        image::load_from_memory(include_bytes!("../../assets/setup-reference-example.png"))
            .unwrap()
            .to_rgba8();
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let size = Size::new(image.width(), image.height());
    let frame = Frame::new(
        image.into_raw().into(),
        size,
        size.width * 4,
        1,
        0,
        Instant::now(),
    )
    .unwrap();
    let mut recovery = ScreenCaptureRecovery::default();

    assert!(!is_nearly_black(&frame));
    assert_eq!(
        recovery.observe(Some(&frame)),
        ScreenCaptureRecoveryObservation::Clear
    );
    assert_eq!(
        recovery.observe(Some(&frame)),
        ScreenCaptureRecoveryObservation::Clear
    );
}

#[test]
fn flow_screen_recovery_confirms_failures_and_clears_on_visible_frames() {
    let black = frame_with_bright_pixels(0);
    let restart_after = |first: Option<&Frame>, second: Option<&Frame>| {
        let mut recovery = ScreenCaptureRecovery::default();
        assert_eq!(
            recovery.observe(first),
            ScreenCaptureRecoveryObservation::AwaitingConfirmation
        );
        assert_eq!(
            recovery.observe(second),
            ScreenCaptureRecoveryObservation::Restart
        );
        recovery
    };

    restart_after(None, None);
    restart_after(Some(&black), None);
    let mut recovery = restart_after(Some(&black), Some(&black));
    assert_eq!(
        recovery.observe(Some(&black)),
        ScreenCaptureRecoveryObservation::AwaitingConfirmation
    );

    let visible = frame_with_bright_pixels(200);
    let mut visible_recovery = ScreenCaptureRecovery::default();
    assert_eq!(
        visible_recovery.observe(None),
        ScreenCaptureRecoveryObservation::AwaitingConfirmation
    );
    assert_eq!(
        visible_recovery.observe(Some(&visible)),
        ScreenCaptureRecoveryObservation::Clear
    );
}

#[test]
fn flow_screen_recovery_accepts_stale_visible_session_frames() {
    let mut visible = frame_with_bright_pixels(200);
    visible.received_at = Instant::now() - FRAME_STALE_AFTER - Duration::from_secs(1);
    let mut recovery = ScreenCaptureRecovery::default();

    assert_eq!(
        recovery.observe(Some(&visible)),
        ScreenCaptureRecoveryObservation::Clear
    );
    assert_eq!(recovery.consecutive_failures, 0);
}

#[test]
fn flow_display_discovery_and_screen_capture_recovery_are_scheduled_independently() {
    let started_at = Instant::now();
    let due_at = started_at + AUTOMATIC_SCREEN_CHECK_INTERVAL;
    assert_eq!(
        automatic_screen_tasks_due(false, false, started_at, started_at, due_at),
        (false, false)
    );
    assert_eq!(
        automatic_screen_tasks_due(true, false, started_at, started_at, due_at),
        (true, false)
    );
    assert_eq!(
        automatic_screen_tasks_due(false, true, started_at, started_at, due_at),
        (false, true)
    );
    assert_eq!(
        automatic_screen_tasks_due(true, true, started_at, started_at, due_at),
        (true, true)
    );
    assert_eq!(
        automatic_screen_tasks_due(
            true,
            true,
            due_at,
            started_at,
            due_at + Duration::from_millis(1),
        ),
        (false, true)
    );
}

#[test]
fn flow_screen_restart_backoff_is_bounded_and_resets_externally_on_success() {
    assert_eq!(screen_restart_backoff(1), Duration::from_secs(5));
    assert_eq!(screen_restart_backoff(2), Duration::from_secs(10));
    assert_eq!(screen_restart_backoff(3), Duration::from_secs(20));
    assert_eq!(screen_restart_backoff(10), Duration::from_secs(60));
}

#[test]
fn flow_publisher_logging_respects_verbose_setting_and_throttles_heartbeats() {
    let now = Instant::now();
    let mut diagnostics = PublisherDiagnosticsSnapshot {
        connected: true,
        published_sequence: 1,
        transmitted_sequence: 1,
        connection_count: 1,
        ..PublisherDiagnosticsSnapshot::default()
    };

    let mut compact = RuntimeState::new_at(AppConfig::default(), now);
    compact.record_publisher_diagnostics(diagnostics, now);
    assert_eq!(compact.activity.len(), 1);
    for frame in 2..=120 {
        diagnostics.published_sequence = frame;
        diagnostics.transmitted_sequence = frame;
        compact.record_publisher_diagnostics(diagnostics, now + Duration::from_millis(frame * 33));
    }
    assert_eq!(compact.activity.len(), 1);
    compact.record_verbose("debug details");
    assert_eq!(compact.activity.len(), 1);

    let mut verbose = RuntimeState::new_at(
        AppConfig {
            verbose_logging: true,
            ..AppConfig::default()
        },
        now,
    );
    diagnostics.published_sequence = 1;
    diagnostics.transmitted_sequence = 1;
    verbose.record_publisher_diagnostics(diagnostics, now);
    diagnostics.published_sequence = 152;
    diagnostics.transmitted_sequence = 152;
    verbose.record_publisher_diagnostics(diagnostics, now + Duration::from_secs(5));
    assert_eq!(verbose.activity.len(), 2);
    verbose.record_verbose("debug details");
    assert_eq!(
        verbose.activity.back().map(String::as_str),
        Some("debug details")
    );
}

#[test]
fn flow_publisher_connection_and_error_changes_bypass_heartbeat_throttle() {
    let now = Instant::now();
    let mut state = RuntimeState::new_at(AppConfig::default(), now);
    let mut diagnostics = PublisherDiagnosticsSnapshot::default();
    state.record_publisher_diagnostics(diagnostics, now);
    diagnostics.connection_count = 1;
    diagnostics.disconnect_count = 1;
    diagnostics.last_disconnect_error = Some(109);
    state.record_publisher_diagnostics(diagnostics, now + Duration::from_millis(10));
    assert_eq!(state.activity.len(), 2);
}

#[test]
fn contract_capture_diagnostics_report_changes_and_throttle_heartbeats() {
    let now = Instant::now();
    let mut diagnostics = CaptureDiagnosticsState::default();
    assert!(diagnostics.observe(0, 0, 0, 0, now));
    assert!(!diagnostics.observe(0, 0, 0, 0, now + Duration::from_secs(1)));
    assert!(diagnostics.observe(0, 0, 1, 0, now + Duration::from_secs(2)));
    assert!(!diagnostics.observe(0, 0, 1, 0, now + Duration::from_secs(3)));
    assert!(diagnostics.observe(0, 0, 1, 0, now + Duration::from_secs(7)));
}

#[test]
fn flow_webcam_recovery_uses_three_nonblocking_attempts_then_exhausts() {
    let started_at = Instant::now();
    let mut recovery = WebcamRecovery::default();
    assert!(recovery.schedule_initial(started_at));
    assert_eq!(
        recovery.next_attempt,
        Some(started_at + Duration::from_millis(500))
    );
    assert_eq!(
        recovery.begin_due_attempt(started_at + Duration::from_millis(499)),
        None
    );

    let first_at = started_at + Duration::from_millis(500);
    assert_eq!(recovery.begin_due_attempt(first_at), Some(1));
    recovery.attempt_started();
    assert!(recovery.attempt_failed(first_at));
    assert_eq!(
        recovery.next_attempt,
        Some(first_at + Duration::from_secs(1))
    );

    let second_at = first_at + Duration::from_secs(1);
    assert_eq!(recovery.begin_due_attempt(second_at), Some(2));
    assert!(recovery.attempt_failed(second_at));
    assert_eq!(
        recovery.next_attempt,
        Some(second_at + Duration::from_secs(2))
    );

    let third_at = second_at + Duration::from_secs(2);
    assert_eq!(recovery.begin_due_attempt(third_at), Some(3));
    assert!(!recovery.attempt_failed(third_at));
    assert!(!recovery.active);
    assert_eq!(recovery.next_attempt, None);
}

#[test]
fn flow_webcam_recovery_success_and_manual_reset_clear_all_retry_state() {
    let now = Instant::now();
    let mut recovery = WebcamRecovery::default();
    recovery.schedule_initial(now);
    assert_eq!(
        recovery.begin_due_attempt(now + Duration::from_millis(500)),
        Some(1)
    );
    recovery.attempt_started();
    recovery.exhaust();
    assert!(!recovery.active);
    assert!(!recovery.waiting_for_first_frame);
    recovery.reset();
    assert_eq!(recovery, WebcamRecovery::default());
}

#[test]
fn flow_reference_detection_is_gray_but_preview_stays_in_color() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reference.png");
    let image = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    image.save(&path).unwrap();

    let (reference, preview) = load_reference(path.to_str().unwrap()).unwrap();

    assert_eq!(reference.size, Size::new(160, 90));
    assert_eq!(preview.size, Size::new(2, 1));
    assert_eq!(preview.pixels(), &[0, 0, 255, 255, 255, 0, 0, 255]);
}

#[test]
fn flow_imported_reference_matches_capture_geometry_and_survives_reload() {
    let directory = tempfile::tempdir().unwrap();
    for (width, height) in [(640, 480), (1680, 720), (1280, 720)] {
        let rgba = image::RgbaImage::from_fn(width, height, |x, y| {
            let value = if (x / 80 + y / 60) % 2 == 0 { 210 } else { 50 };
            image::Rgba([value, value, value, 255])
        });
        let source = directory.path().join("import.png");
        let destination = directory.path().join("reference.png");
        rgba.save(&source).unwrap();
        let result = execute_reference_job(ReferenceJob {
            commit: None,
            generation: 1,
            kind: ReferenceJobKind::Import {
                source,
                destination: destination.clone(),
            },
        })
        .result
        .unwrap();
        let frame = Frame::new(
            rgba.into_raw().into(),
            Size::new(width, height),
            width * 4,
            1,
            0,
            Instant::now(),
        )
        .unwrap()
        .aspect_fit(PIPELINE_SIZE, Instant::now());
        let captured = gray_thumbnail(&frame).unwrap();
        assert_eq!(image_similarity(&result.detector, &captured), 1.0);
        assert_eq!(
            decode_reference(&destination).unwrap().detector,
            result.detector
        );
    }
}

#[test]
fn flow_reference_candidate_is_isolated_discardable_and_committed_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reference.png");
    let config = AppConfig {
        reference_image_path: path.display().to_string(),
        ..AppConfig::default()
    };
    let mut state = RuntimeState::new(config);
    let original = Arc::new(solid_frame(32));
    state.snapshot.previews.reference = Some(Arc::clone(&original));
    let candidate = Arc::new(solid_frame(176));

    state.stage_reference_candidate(Arc::clone(&candidate));
    assert!(Arc::ptr_eq(
        state.snapshot.previews.reference.as_ref().unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        state
            .snapshot
            .previews
            .reference_candidate
            .as_ref()
            .unwrap(),
        &candidate
    ));

    state.discard_reference_candidate();
    assert!(state.snapshot.previews.reference_candidate.is_none());
    assert!(Arc::ptr_eq(
        state.snapshot.previews.reference.as_ref().unwrap(),
        &original
    ));

    state.stage_reference_candidate(Arc::clone(&candidate));
    state.confirm_reference_candidate().unwrap();

    assert!(state.snapshot.previews.reference_candidate.is_none());
    assert_eq!(
        state.snapshot.previews.reference.as_ref().unwrap().pixels(),
        candidate.pixels()
    );
    let (_, loaded) = load_reference(path.to_str().unwrap()).unwrap();
    assert_eq!(loaded.pixels(), candidate.pixels());
}

#[test]
fn flow_failed_candidate_save_keeps_candidate_and_active_reference_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let invalid_path = directory.path().join("reference-target-is-a-directory");
    std::fs::create_dir(&invalid_path).unwrap();
    let config = AppConfig {
        reference_image_path: invalid_path.display().to_string(),
        ..AppConfig::default()
    };
    let mut state = RuntimeState::new(config);
    let original = Arc::new(solid_frame(32));
    let candidate = Arc::new(solid_frame(176));
    state.snapshot.previews.reference = Some(Arc::clone(&original));
    state.stage_reference_candidate(Arc::clone(&candidate));

    assert!(state.confirm_reference_candidate().is_err());
    assert!(Arc::ptr_eq(
        state.snapshot.previews.reference.as_ref().unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        state
            .snapshot
            .previews
            .reference_candidate
            .as_ref()
            .unwrap(),
        &candidate
    ));
    assert!(!directory.path().join("reference.pending.png").exists());
}

#[test]
fn flow_reference_decode_enforces_dimension_limit_and_caps_retained_preview() {
    let directory = tempfile::tempdir().unwrap();
    let boundary = directory.path().join("boundary.png");
    image::RgbaImage::new(REFERENCE_MAX_DIMENSION, 1)
        .save(&boundary)
        .unwrap();
    assert!(decode_reference(&boundary).is_ok());

    let oversized = directory.path().join("oversized.png");
    image::RgbaImage::new(REFERENCE_MAX_DIMENSION + 1, 1)
        .save(&oversized)
        .unwrap();
    assert!(decode_reference(&oversized).is_err());

    let large = image::RgbaImage::new(2000, 1000);
    let data = reference_data_from_rgba(&large).unwrap();
    assert_eq!(data.preview.size, Size::new(1280, 640));
    assert_eq!(data.detector.size, Size::new(160, 90));
}

#[test]
fn flow_atomic_reference_replace_preserves_exact_pixels_and_cleans_pending_file() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("reference.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .save(&destination)
        .unwrap();
    let replacement = image::RgbaImage::from_pixel(3, 1, image::Rgba([9, 8, 7, 255]));

    persist_rgba_atomic(&replacement, &destination).unwrap();

    assert_eq!(image::open(&destination).unwrap().to_rgba8(), replacement);
    assert!(!directory.path().join("reference.pending.png").exists());
}

#[test]
fn flow_warning_registry_preserves_source_ownership_and_priority() {
    let mut state = RuntimeState::new(AppConfig::default());
    state.set_warning(WarningSource::Hdr, "hdr");
    state.set_warning(WarningSource::VirtualCamera, "camera");
    state.set_warning(WarningSource::PublisherSink, "publisher");
    assert_eq!(state.snapshot.warning.as_deref(), Some("publisher"));
    assert_eq!(state.snapshot.active_warnings.len(), 3);
    assert_eq!(state.snapshot.recent_alerts.len(), 3);

    state.clear_warning(WarningSource::VirtualCamera);
    assert_eq!(state.snapshot.warning.as_deref(), Some("publisher"));
    assert_eq!(state.snapshot.active_warnings.len(), 2);
    state.clear_warning(WarningSource::PublisherSink);
    assert_eq!(state.snapshot.warning.as_deref(), Some("hdr"));
    state.clear_warning(WarningSource::Hdr);
    assert!(state.snapshot.warning.is_none());
    assert!(state.snapshot.active_warnings.is_empty());
}

#[test]
fn contract_warning_changes_emit_one_alert_per_source_message() {
    let mut state = RuntimeState::new(AppConfig::default());
    state.set_warning(WarningSource::WebcamCapture, "camera failed");
    state.set_warning(WarningSource::WebcamCapture, "camera failed");
    assert_eq!(state.snapshot.recent_alerts.len(), 1);
    assert_eq!(state.snapshot.recent_alerts[0].id, 1);
    assert_eq!(state.snapshot.recent_alerts_first_id, 1);

    state.set_warning(WarningSource::WebcamCapture, "camera recovered then failed");
    assert_eq!(state.snapshot.recent_alerts.len(), 2);
    assert_eq!(state.snapshot.recent_alerts[1].id, 2);
    assert_eq!(
        state.snapshot.recent_alerts[1].source,
        RuntimeAlertSource::Webcam
    );
}

#[test]
fn flow_activity_ids_keep_duplicate_messages_and_track_ring_rollover() {
    let mut state = RuntimeState::new(AppConfig::default());
    state.record("duplicate");
    state.record("duplicate");
    assert_eq!(state.snapshot.recent_activity_first_id, 1);
    assert_eq!(state.snapshot.recent_activity.len(), 2);
    for index in 0..ACTIVITY_LIMIT {
        state.record(format!("event-{index}"));
    }
    assert_eq!(state.snapshot.recent_activity.len(), ACTIVITY_LIMIT);
    assert_eq!(state.snapshot.recent_activity_first_id, 3);
    assert_eq!(state.snapshot.recent_activity[0], "event-0");
}

#[test]
fn flow_device_command_application_does_not_replay_user_activity() {
    let mut state = RuntimeState::new(AppConfig::default());
    assert!(state.apply_command(Command::Start, false));
    assert!(state.snapshot.recent_activity.is_empty());

    assert!(state.command(Command::Start));
    assert_eq!(state.snapshot.recent_activity.len(), 1);
    assert_eq!(state.snapshot.recent_activity[0], "Automation started");
}

#[test]
fn flow_duplicate_device_refresh_and_rescan_activity_is_coalesced() {
    let mut state = RuntimeState::new(AppConfig::default());

    assert!(state.command(Command::RefreshVideoDevices));
    assert!(state.command(Command::RefreshVideoDevices));
    assert!(state.command(Command::Rescan));
    assert!(state.command(Command::Rescan));

    assert_eq!(state.snapshot.recent_activity.len(), 2);
    assert_eq!(
        state.snapshot.recent_activity[0],
        "Video device list refreshed"
    );
    assert_eq!(
        state.snapshot.recent_activity[1],
        "Monitor rescan requested"
    );
}

#[test]
fn flow_settings_only_reload_reference_when_required() {
    let now = Instant::now();
    let config = AppConfig::default();
    let state = RuntimeState::new_at(config.clone(), now);
    let mut engine = RuntimeEngine::from_parts(state, FakeRuntimePorts::default(), now);
    assert_eq!(engine.reference_generation, 1);

    assert!(engine.command(Command::UpdateSettings(Box::new(config.clone()))));
    assert_eq!(engine.reference_generation, 1);
    assert!(engine.command(Command::ReloadSettings(Box::new(config))));
    assert_eq!(engine.reference_generation, 2);
}

#[test]
fn flow_webcam_failure_and_staleness_exclude_retained_frames() {
    let now = Instant::now();
    let mut snapshot = RuntimeState::new_at(AppConfig::default(), now).snapshot;
    let frame = scripted_solid_frame(Size::new(2, 2), 0xff40_4040, 1, now);
    snapshot.previews.webcam = Some(Arc::clone(&frame));
    snapshot.availability.camera_ready = false;
    assert!(ready_webcam_frame(&snapshot, now).is_none());
    snapshot.availability.camera_ready = true;
    assert!(Arc::ptr_eq(
        &ready_webcam_frame(&snapshot, now).unwrap(),
        &frame
    ));
    assert!(ready_webcam_frame(&snapshot, now + Duration::from_secs(2)).is_none());
    snapshot.previews.webcam = None;
    assert!(ready_webcam_frame(&snapshot, now).is_none());
}

#[test]
fn flow_device_commands_are_bounded_coalesced_and_restart_all_subsumes_individuals() {
    let mut pending = PendingDeviceCommands::default();
    let first = AppConfig {
        cursor_visible: false,
        ..AppConfig::default()
    };
    let latest = AppConfig {
        cursor_visible: true,
        ..first.clone()
    };
    pending.push(&Command::UpdateSettings(Box::new(first)));
    pending.push(&Command::UpdateSettings(Box::new(latest.clone())));
    pending.push(&Command::RefreshVideoDevices);
    pending.push(&Command::RefreshVideoDevices);
    pending.push(&Command::Restart(RestartTarget::Webcam));
    pending.push(&Command::Restart(RestartTarget::All));
    pending.push(&Command::Restart(RestartTarget::ScreenCapture));

    let (commands, reference) = pending.take();

    assert!(reference.is_none());
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, Command::UpdateSettings(_)))
            .count(),
        1
    );
    assert!(commands.contains(&Command::UpdateSettings(Box::new(latest))));
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, Command::RefreshVideoDevices))
            .count(),
        1
    );
    assert!(commands.contains(&Command::Restart(RestartTarget::All)));
    assert!(!commands.contains(&Command::Restart(RestartTarget::Webcam)));
    assert!(!commands.contains(&Command::Restart(RestartTarget::ScreenCapture)));
    assert!(pending.take().0.is_empty());
    pending.update_reference(Some(GrayImage::new(Size::new(1, 1), vec![1]).unwrap()));
    pending.update_reference(None);
    let (commands, reference) = pending.take();
    assert_eq!(reference, Some(None));
    assert!(commands.contains(&Command::DiscardReferenceCandidate));
    assert_eq!(pending.take().1, None);
}

#[test]
fn flow_device_batches_preserve_candidate_and_output_intent() {
    let mut pending = PendingDeviceCommands::default();
    pending.push(&Command::DiscardReferenceCandidate);
    pending.push(&Command::CaptureReferenceCandidate);
    assert_eq!(
        pending.take().0,
        vec![
            Command::DiscardReferenceCandidate,
            Command::CaptureReferenceCandidate
        ]
    );
    pending.push(&Command::CaptureReferenceCandidate);
    pending.push(&Command::DiscardReferenceCandidate);
    assert_eq!(pending.take().0, vec![Command::DiscardReferenceCandidate]);
    pending.push(&Command::CaptureReferenceCandidate);
    pending.update_reference(None);
    assert_eq!(pending.take().0, vec![Command::DiscardReferenceCandidate]);
    pending.push(&Command::SetMode(OutputMode::ForceScreen));
    pending.push(&Command::ReloadSettings(Box::default()));
    assert!(matches!(
        pending.take().0.as_slice(),
        [Command::ReloadSettings(_)]
    ));
    pending.push(&Command::UpdateSettings(Box::default()));
    pending.push(&Command::SetMode(OutputMode::ForceCamera));
    assert!(matches!(
        pending.take().0.as_slice(),
        [
            Command::UpdateSettings(_),
            Command::SetMode(OutputMode::ForceCamera)
        ]
    ));
}

#[test]
fn flow_screen_fallback_uses_worker_liveness_without_expiring_static_content() {
    let now = Instant::now();
    let mut snapshot = AppSnapshot::default();
    let frame = Arc::new(solid_frame(128));
    snapshot.previews.screen = Some(Arc::clone(&frame));
    snapshot.availability.screen_ready = true;
    let later = now + Duration::from_secs(60);
    assert!(Arc::ptr_eq(
        &ready_screen_frame(&snapshot, Some(later), later).unwrap(),
        &frame
    ));
    assert!(ready_screen_frame(&snapshot, Some(now), later).is_none());
    assert!(ready_screen_frame(&snapshot, None, now).is_none());
    snapshot.availability.screen_ready = false;
    assert!(ready_screen_frame(&snapshot, Some(later), later).is_none());
}

#[test]
fn flow_discovery_request_slots_keep_only_the_latest_pending_work() {
    let slot = CoalescingSlot::new();
    assert!(slot.replace(1_u64));
    assert!(slot.replace(2_u64));
    assert_eq!(slot.take(), Some(2));
    assert_eq!(slot.take(), None);
    assert!(slot.replace(3_u64));
    slot.clear();
    assert_eq!(slot.take(), None);
}

#[test]
fn flow_commands_publish_immutable_snapshots() {
    let runtime = RuntimeHandle::spawn(AppConfig {
        start_automatically: false,
        ..AppConfig::default()
    });
    assert!(runtime.send(Command::Start).is_accepted());
    assert!(
        runtime
            .send(Command::SetMode(OutputMode::ForceScreen))
            .is_accepted()
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let snapshot = loop {
        let snapshot = runtime.snapshot();
        if snapshot.run_state == RunState::Running
            && snapshot.mode == OutputMode::ForceScreen
            && snapshot.previews.final_output.is_some()
        {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "runtime did not publish command results"
        );
        thread::yield_now();
    };
    assert_eq!(snapshot.run_state, RunState::Running);
    assert_eq!(snapshot.mode, OutputMode::ForceScreen);
    assert_eq!(snapshot.actual_output, Source::Placeholder);
    assert!(snapshot.previews.final_output.is_some());
}

#[test]
fn flow_disco_mode_is_session_only_and_never_decorates_the_stopped_output() {
    let runtime = RuntimeHandle::spawn(AppConfig {
        start_automatically: false,
        ..AppConfig::default()
    });
    assert!(!runtime.snapshot().disco_enabled);

    assert!(runtime.send(Command::ToggleDisco).is_accepted());
    let deadline = Instant::now() + Duration::from_secs(1);
    let enabled = loop {
        let snapshot = runtime.snapshot();
        if snapshot.disco_enabled {
            break snapshot;
        }
        assert!(Instant::now() < deadline, "disco mode was not enabled");
        thread::yield_now();
    };
    assert_eq!(enabled.run_state, RunState::Stopped);
    assert_eq!(
        enabled
            .previews
            .final_output
            .expect("stopped output is present")
            .pixels(),
        stageswap_core::off_frame_pixels().as_ref()
    );

    assert!(runtime.send(Command::ToggleDisco).is_accepted());
    loop {
        if !runtime.snapshot().disco_enabled {
            break;
        }
        assert!(Instant::now() < deadline, "disco mode was not disabled");
        thread::yield_now();
    }
}

#[test]
fn flow_stopped_runtime_publishes_off_frame_and_resumes_after_start() {
    let runtime = RuntimeHandle::spawn(AppConfig {
        start_automatically: false,
        ..AppConfig::default()
    });
    let initial_snapshot = runtime.snapshot();
    let initial = initial_snapshot
        .previews
        .final_output
        .expect("initial off frame is present");
    assert_eq!(initial_snapshot.run_state, RunState::Stopped);
    assert_eq!(initial_snapshot.actual_output, Source::Placeholder);
    assert_eq!(
        initial.pixels(),
        stageswap_core::off_frame_pixels().as_ref()
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let later = loop {
        let output = runtime
            .snapshot()
            .previews
            .final_output
            .expect("off frame remains present");
        if output.sequence > initial.sequence {
            break output;
        }
        assert!(
            Instant::now() < deadline,
            "stopped output clock did not advance"
        );
        thread::yield_now();
    };
    assert_eq!(later.pixels(), stageswap_core::off_frame_pixels().as_ref());

    assert!(runtime.send(Command::Start).is_accepted());
    let running_sequence = loop {
        let snapshot = runtime.snapshot();
        if snapshot.run_state == RunState::Running
            && let Some(output) = snapshot.previews.final_output
            && output.sequence > later.sequence
        {
            break output.sequence;
        }
        assert!(Instant::now() < deadline, "runtime did not resume output");
        thread::yield_now();
    };
    assert!(running_sequence > later.sequence);

    assert!(runtime.send(Command::Stop).is_accepted());
    let stopped = loop {
        let snapshot = runtime.snapshot();
        if snapshot.run_state == RunState::Stopped
            && snapshot
                .previews
                .final_output
                .as_ref()
                .is_some_and(|output| output.sequence > running_sequence)
        {
            break snapshot;
        }
        assert!(Instant::now() < deadline, "runtime did not stop");
        thread::yield_now();
    };
    let stopped_output = stopped.previews.final_output.expect("off frame is present");
    assert_eq!(stopped.actual_output, Source::Placeholder);
    assert_eq!(
        stopped_output.pixels(),
        stageswap_core::off_frame_pixels().as_ref()
    );
}

#[test]
fn flow_command_traffic_does_not_accelerate_output_clock() {
    let runtime = RuntimeHandle::spawn(AppConfig {
        start_automatically: true,
        ..AppConfig::default()
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let first_sequence = loop {
        if let Some(frame) = runtime.snapshot().previews.final_output {
            break frame.sequence;
        }
        assert!(Instant::now() < deadline, "runtime did not start");
        thread::yield_now();
    };
    for index in 0..40 {
        let mode = if index % 2 == 0 {
            OutputMode::ForceCamera
        } else {
            OutputMode::ForceScreen
        };
        assert!(runtime.send(Command::SetMode(mode)).is_accepted());
        thread::sleep(Duration::from_millis(2));
    }
    thread::sleep(Duration::from_millis(20));
    let last_sequence = runtime
        .snapshot()
        .previews
        .final_output
        .expect("runtime stopped publishing")
        .sequence;
    assert!(
        last_sequence.saturating_sub(first_sequence) <= 5,
        "commands accelerated output from sequence {first_sequence} to {last_sequence}"
    );
}

#[cfg(windows)]
#[test]
#[ignore = "requires installed COM source, interactive desktop, and a physical webcam"]
fn native_all_four_manual_restart_actions_recover_components() {
    let runtime = RuntimeHandle::spawn(AppConfig::default());
    let ready_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = runtime.snapshot();
        if snapshot.webcam_state == DeviceState::Ready
            && snapshot.screen_state == DeviceState::Ready
            && snapshot.virtual_camera_state == DeviceState::Ready
        {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "components did not initialize"
        );
        thread::sleep(Duration::from_millis(25));
    }
    for (target, expected) in [
        (RestartTarget::Webcam, "Webcam restarted"),
        (RestartTarget::ScreenCapture, "Screen capture restarted"),
        (RestartTarget::VirtualCamera, "Virtual camera restarted"),
        (RestartTarget::All, "Restart requested: All"),
    ] {
        assert!(runtime.send(Command::Restart(target)).is_accepted());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = runtime.snapshot();
            if snapshot
                .recent_activity
                .iter()
                .any(|activity| activity.contains(expected))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "restart did not complete: {target:?}"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

#[test]
fn contract_webcam_recovery_uses_failure_categories() {
    for kind in [
        WebcamFailureKind::DriverFailure,
        WebcamFailureKind::DeviceInvalidated,
    ] {
        assert!(webcam_failure_is_automatically_recoverable(kind));
    }
    for kind in [
        WebcamFailureKind::PrivacyDisabled,
        WebcamFailureKind::AccessDenied,
        WebcamFailureKind::DeviceBusy,
        WebcamFailureKind::UnsupportedFormat,
        WebcamFailureKind::IncompatibleTypeChange,
    ] {
        assert!(!webcam_failure_is_automatically_recoverable(kind));
    }
}
