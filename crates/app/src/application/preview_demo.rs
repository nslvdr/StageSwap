use super::*;

impl SwitcherApp {
    #[cfg(not(windows))]
    pub(super) fn with_ui_preview(mut self, request: UiPreviewRequest) -> Self {
        let preview_now = Instant::now();
        let periodic_notifications = !matches!(
            request.target,
            UiPreviewTarget::UpdateVersions
                | UiPreviewTarget::Notifications(
                    NotificationPreviewState::Empty
                        | NotificationPreviewState::Critical
                        | NotificationPreviewState::Updates
                )
        );
        if periodic_notifications {
            self.seed_ui_preview_notifications(preview_now);
        }
        match request.target {
            UiPreviewTarget::UpdateVersions => {
                self.view = AppView::Settings;
                self.settings_tab = SettingsTab::Updates;
                self.config.update_channel = UpdateChannel::Beta;
                self.updates.picker_open = true;
                self.updates.versions = update::VersionCatalog::Ready(update::preview_versions());
            }
            UiPreviewTarget::Settings(tab) => {
                self.view = AppView::Settings;
                self.settings_tab = tab;
            }
            UiPreviewTarget::Setup(step) => {
                self.view = AppView::SetupGuide;
                self.setup_session = Some(SetupSession::preview(step));
            }
            UiPreviewTarget::Dialog(kind) => {
                self.view = AppView::Settings;
                self.settings_tab = match kind {
                    AppDialogKind::ClearLogs => SettingsTab::Diagnostics,
                    AppDialogKind::ReferenceCapture => SettingsTab::Matching,
                    _ => SettingsTab::General,
                };
                if matches!(
                    kind,
                    AppDialogKind::Admin
                        | AppDialogKind::ReplaceAdminBaseline
                        | AppDialogKind::LoadAdminConfig
                        | AppDialogKind::RemoveAdminBaseline
                ) {
                    self.admin_profile_status = Some(AdminProfileStatus {
                        auto_restore_on_launch: true,
                        reference_included: true,
                    });
                }
                self.open_dialog(kind);
            }
            UiPreviewTarget::Notifications(state) => {
                self.view = AppView::Dashboard;
                let now = Instant::now();
                match state {
                    NotificationPreviewState::Empty => {
                        self.notification_center_open = true;
                    }
                    NotificationPreviewState::Critical => {
                        self.notifications.push_critical(
                            NotificationSource::Webcam,
                            "The selected webcam stopped delivering frames.",
                            now,
                            true,
                        );
                        self.notification_center_open = true;
                    }
                    NotificationPreviewState::Updates => {
                        self.notifications.push_update(
                            "2.4.0",
                            "StageSwap 2.4.0 is ready. Open Settings → Updates to install it.",
                            now,
                            true,
                        );
                        self.notification_center_open = true;
                    }
                    NotificationPreviewState::Stacked => {
                        self.notification_center_open = false;
                    }
                }
            }
        }
        self.settings_opened_at = None;
        self.settings_section_changed_at = None;
        self.ui_preview = Some(UiPreviewSession {
            snapshot: ui_preview_snapshot(),
            next_notification_at: periodic_notifications
                .then_some(preview_now + UI_PREVIEW_NOTIFICATION_INTERVAL),
            notification_count: 0,
        });
        self
    }

    #[cfg(not(windows))]
    pub(super) fn seed_ui_preview_notifications(&mut self, now: Instant) {
        self.notifications.push_critical(
            NotificationSource::Webcam,
            "The selected webcam stopped delivering frames.",
            now,
            true,
        );
        self.notifications.push_critical(
            NotificationSource::Screen,
            "The selected secondary screen is not providing frames.",
            now,
            true,
        );
        self.notifications.push_update(
            "2.4.0",
            "StageSwap 2.4.0 is ready. Open Settings → Updates to install it.",
            now,
            true,
        );
    }

    #[cfg(not(windows))]
    pub(super) fn tick_ui_preview_notifications(&mut self, context: &egui::Context, now: Instant) {
        let count = {
            let Some(preview) = self.ui_preview.as_mut() else {
                return;
            };
            let Some(next_notification_at) = preview.next_notification_at else {
                return;
            };
            if now < next_notification_at {
                context.request_repaint_after(next_notification_at - now);
                return;
            }

            preview.notification_count = preview.notification_count.saturating_add(1);
            preview.next_notification_at = Some(now + UI_PREVIEW_NOTIFICATION_INTERVAL);
            preview.notification_count.to_string()
        };
        let message = format_text(
            self.locale(),
            "Preview activity notification {0}: 30 seconds have passed.",
            &[&count],
        );
        self.notifications
            .push_information(NotificationSource::Preview, message, now);
        context.request_repaint_after(UI_PREVIEW_NOTIFICATION_INTERVAL);
    }

    #[cfg(not(windows))]
    pub(super) fn with_ui_screenshot(mut self, path: PathBuf) -> Self {
        self.ui_screenshot = Some(UiScreenshotCapture {
            path,
            frames_until_request: 2,
            requested: false,
        });
        self
    }

    #[cfg(not(windows))]
    pub(super) fn with_setup_demo_preview_state(
        mut self,
        state: Option<SetupDemoPreviewState>,
    ) -> Self {
        self.setup_demo_preview_state = state;
        self
    }

    #[cfg(not(windows))]
    pub(super) fn with_setup_reference_preview_state(
        mut self,
        state: Option<SetupReferencePreviewState>,
    ) -> Self {
        let Some(state) = state else {
            return self;
        };
        let Some(preview) = self.ui_preview.as_mut() else {
            return self;
        };
        match state {
            SetupReferencePreviewState::Captured => {
                preview.snapshot.previews.reference_candidate = None;
                self.setup_reference_capture = SetupReferenceCaptureState::Idle;
            }
            SetupReferencePreviewState::Empty => {
                preview.snapshot.previews.reference = None;
                preview.snapshot.previews.reference_candidate = None;
                self.setup_reference_capture = SetupReferenceCaptureState::Idle;
            }
            SetupReferencePreviewState::Review => {
                preview.snapshot.previews.reference = None;
                preview.snapshot.previews.reference_candidate =
                    preview.snapshot.previews.screen.clone();
                self.setup_reference_capture = SetupReferenceCaptureState::Review {
                    captured_at: Instant::now() - SETUP_REFERENCE_FLASH_DURATION,
                };
            }
            SetupReferencePreviewState::MissingScreen => {
                preview.snapshot.previews.reference = None;
                preview.snapshot.previews.reference_candidate = None;
                preview.snapshot.previews.screen = None;
                preview.snapshot.selected_monitor = None;
                preview.snapshot.monitors = Vec::new().into();
                preview.snapshot.screen_state = DeviceState::Unavailable;
                preview.snapshot.availability.screen_ready = false;
                self.config.selected_monitor_label.clear();
                self.setup_reference_capture = SetupReferenceCaptureState::Idle;
            }
        }
        self
    }

    pub(super) fn setup_demo_preview_alpha(&self) -> Option<f32> {
        #[cfg(not(windows))]
        {
            self.setup_demo_preview_state.map(|state| match state {
                SetupDemoPreviewState::Matching => 0.0,
                SetupDemoPreviewState::Changed => 1.0,
            })
        }
        #[cfg(windows)]
        {
            None
        }
    }
}

#[cfg(not(windows))]
pub(super) fn parse_ui_preview_request(
    args: &[String],
) -> Result<Option<UiPreviewRequest>, String> {
    let Some(preview_index) = args.iter().position(|argument| argument == "--ui-preview") else {
        return Ok(None);
    };
    ui_preview_locale(args)?;
    let target = match args.get(preview_index + 1).map(String::as_str) {
        Some("updates-versions") => UiPreviewTarget::UpdateVersions,
        Some("dialog-exit") => UiPreviewTarget::Dialog(AppDialogKind::Exit),
        Some("dialog-clear-logs") => UiPreviewTarget::Dialog(AppDialogKind::ClearLogs),
        Some("dialog-admin") => UiPreviewTarget::Dialog(AppDialogKind::Admin),
        Some("dialog-replace-baseline") => {
            UiPreviewTarget::Dialog(AppDialogKind::ReplaceAdminBaseline)
        }
        Some("dialog-load-admin-config") => UiPreviewTarget::Dialog(AppDialogKind::LoadAdminConfig),
        Some("dialog-remove-baseline") => {
            UiPreviewTarget::Dialog(AppDialogKind::RemoveAdminBaseline)
        }
        Some("dialog-reference-capture") => {
            UiPreviewTarget::Dialog(AppDialogKind::ReferenceCapture)
        }
        Some("notifications") => UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
        Some("notifications-empty") => {
            UiPreviewTarget::Notifications(NotificationPreviewState::Empty)
        }
        Some("notifications-critical") => {
            UiPreviewTarget::Notifications(NotificationPreviewState::Critical)
        }
        Some("notifications-updates") => {
            UiPreviewTarget::Notifications(NotificationPreviewState::Updates)
        }
        Some(value) if value.starts_with("setup-") => {
            let number = value
                .strip_prefix("setup-")
                .and_then(|value| value.parse::<usize>().ok());
            let step = number.and_then(SetupStep::from_number).ok_or_else(|| {
                format!("unknown setup preview '{value}'; expected setup-1 through setup-5")
            })?;
            UiPreviewTarget::Setup(step)
        }
        Some(value) if !value.starts_with("--") => SettingsTab::from_preview_name(value)
            .map(UiPreviewTarget::Settings)
            .ok_or_else(|| {
                format!(
                    "unknown UI preview '{value}'; expected a Settings page, notifications preview, or dialog-* preview"
                )
            })?,
        _ => UiPreviewTarget::Settings(SettingsTab::General),
    };
    Ok(Some(UiPreviewRequest { target }))
}

#[cfg(not(windows))]
pub(super) fn parse_ui_screenshot_path(args: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(index) = args
        .iter()
        .position(|argument| argument == "--ui-screenshot")
    else {
        return Ok(None);
    };
    let value = args
        .get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| "--ui-screenshot requires an absolute PNG path".to_string())?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("--ui-screenshot requires an absolute PNG path".to_string());
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("png") {
        return Err("--ui-screenshot path must end in .png".to_string());
    }
    Ok(Some(path))
}

#[cfg(not(windows))]
pub(super) fn parse_setup_demo_preview_state(
    args: &[String],
) -> Result<Option<SetupDemoPreviewState>, String> {
    let Some(index) = args
        .iter()
        .position(|argument| argument == "--ui-setup-demo-state")
    else {
        return Ok(None);
    };
    match args.get(index + 1).map(String::as_str) {
        Some("matching") => Ok(Some(SetupDemoPreviewState::Matching)),
        Some("changed" | "non-matching") => Ok(Some(SetupDemoPreviewState::Changed)),
        _ => Err("--ui-setup-demo-state requires either matching or non-matching".to_string()),
    }
}

#[cfg(not(windows))]
pub(super) fn parse_setup_reference_preview_state(
    args: &[String],
) -> Result<Option<SetupReferencePreviewState>, String> {
    let Some(index) = args
        .iter()
        .position(|argument| argument == "--ui-setup-reference-state")
    else {
        return Ok(None);
    };
    match args.get(index + 1).map(String::as_str) {
        Some("captured") => Ok(Some(SetupReferencePreviewState::Captured)),
        Some("empty") => Ok(Some(SetupReferencePreviewState::Empty)),
        Some("review") => Ok(Some(SetupReferencePreviewState::Review)),
        Some("missing-screen") => Ok(Some(SetupReferencePreviewState::MissingScreen)),
        _ => Err(
            "--ui-setup-reference-state requires captured, empty, review, or missing-screen"
                .to_string(),
        ),
    }
}

#[cfg(not(windows))]
pub(super) fn ui_preview_locale(args: &[String]) -> Result<Locale, String> {
    let Some(index) = args.iter().position(|argument| argument == "--ui-language") else {
        return Ok(Locale::English);
    };
    let value = args
        .get(index + 1)
        .ok_or_else(|| "--ui-language requires en-US, fr-FR, or es".to_string())?;
    Locale::from_tag(value)
        .ok_or_else(|| format!("unknown UI language '{value}'; expected en-US, fr-FR, or es"))
}

#[cfg(not(windows))]
pub(super) fn ui_preview_config() -> AppConfig {
    let locale = ui_preview_locale(&std::env::args().collect::<Vec<_>>()).unwrap_or_default();
    AppConfig {
        selected_video_device_id: "preview-camera".into(),
        selected_monitor_label: "Stage display".into(),
        start_with_windows: true,
        start_minimized: true,
        interface_language: locale.tag().into(),
        start_automatically: true,
        output_mode: OutputMode::Automatic,
        still_image_pip_enabled: true,
        ..AppConfig::default()
    }
}

#[cfg(not(windows))]
pub(super) fn ui_preview_snapshot() -> AppSnapshot {
    let webcam = ui_preview_frame(
        1,
        "setup-webcam-example",
        include_bytes!("../../assets/setup-webcam-example.png"),
    );
    let screen = ui_preview_frame(
        2,
        "setup-reference-example",
        include_bytes!("../../assets/setup-reference-example.png"),
    );
    AppSnapshot {
        run_state: RunState::Running,
        mode: OutputMode::Automatic,
        detection: DetectionState::Matching,
        automatic_target: Source::Camera,
        actual_output: Source::Camera,
        availability: stageswap_core::SourceAvailability {
            camera_ready: true,
            screen_ready: true,
        },
        webcam_state: DeviceState::Ready,
        screen_state: DeviceState::Ready,
        virtual_camera_state: DeviceState::Ready,
        webcam_fps: Some(30),
        screen_fps: Some(30),
        output_fps: Some(30),
        recent_activity_first_id: 1,
        recent_activity: vec![
            "Reference display confirmed".into(),
            "Automatic output selected Camera".into(),
        ]
        .into(),
        previews: stageswap_core::PreviewFrames {
            final_output: Some(Arc::clone(&webcam)),
            webcam: Some(webcam),
            screen: Some(Arc::clone(&screen)),
            reference: Some(screen),
            reference_candidate: None,
        },
        video_devices: vec![
            stageswap_core::VideoDeviceChoice {
                id: "preview-camera".into(),
                name: "Studio Camera".into(),
            },
            stageswap_core::VideoDeviceChoice {
                id: "preview-camera-secondary".into(),
                name: "Wide Camera".into(),
            },
        ]
        .into(),
        selected_video_device_id: "preview-camera".into(),
        monitors: vec![
            stageswap_core::MonitorDescriptor {
                display_name: "preview-display-1".into(),
                label: "Control display".into(),
                width: 1920,
                height: 1080,
                ..stageswap_core::MonitorDescriptor::default()
            },
            stageswap_core::MonitorDescriptor {
                display_name: "preview-display-2".into(),
                label: "Stage display".into(),
                x: 1920,
                width: 1920,
                height: 1080,
                ..stageswap_core::MonitorDescriptor::default()
            },
        ]
        .into(),
        selected_monitor: Some(stageswap_core::MonitorDescriptor {
            display_name: "preview-display-2".into(),
            label: "Stage display".into(),
            x: 1920,
            width: 1920,
            height: 1080,
            ..stageswap_core::MonitorDescriptor::default()
        }),
        ..AppSnapshot::default()
    }
}

#[cfg(not(windows))]
pub(super) fn ui_preview_frame(sequence: u64, name: &'static str, bytes: &[u8]) -> Arc<Frame> {
    let image = image::load_from_memory(bytes)
        .unwrap_or_else(|error| panic!("{name} should be a valid embedded image: {error}"))
        .to_rgba8();
    let size = stageswap_core::Size::new(image.width(), image.height());
    let mut pixels = image.into_raw();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(
        Frame::new(
            pixels.into(),
            size,
            size.width * 4,
            sequence,
            0,
            Instant::now(),
        )
        .expect("UI preview frame is valid"),
    )
}

#[cfg(not(windows))]
pub(super) fn clone_ui_preview_frame(frame: &Frame, sequence: u64) -> Option<Arc<Frame>> {
    Frame::new(
        frame.pixels_arc(),
        frame.size,
        frame.stride,
        sequence,
        frame.timestamp_100ns,
        Instant::now(),
    )
    .ok()
    .map(Arc::new)
}

#[cfg(not(windows))]
pub(super) fn save_ui_screenshot(
    path: &std::path::Path,
    image: &egui::ColorImage,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_array());
    }
    let source = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba)
        .ok_or_else(|| "screenshot pixel buffer had an invalid size".to_string())?;
    let output = if source.dimensions() == (1280, 720) {
        source
    } else {
        image::imageops::resize(&source, 1280, 720, image::imageops::FilterType::Lanczos3)
    };
    output
        .save(path)
        .map_err(|error| format!("could not save {}: {error}", path.display()))
}
