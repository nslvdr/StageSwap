use super::*;

#[test]
fn contract_settings_switch_renders_both_states_without_state_labels() {
    fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, texts);
                }
            }
            _ => {}
        }
    }

    for (initial_value, expected_fill) in [
        (false, settings_switch_colors().0),
        (true, settings_switch_colors().1),
    ] {
        let context = egui::Context::default();
        let mut value = initial_value;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 100.0))),
                ..egui::RawInput::default()
            },
            |ui| {
                settings_toggle_row_without_separator(
                    ui,
                    &mut value,
                    "Test switch",
                    "Static switch description.",
                );
            },
        );
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut texts);
        }

        assert!(texts.iter().any(|text| text == "Test switch"));
        assert!(
            texts
                .iter()
                .any(|text| text == "Static switch description.")
        );
        assert!(!texts.iter().any(|text| text == "On" || text == "Off"));
        assert!(output.shapes.iter().any(|clipped| {
            matches!(
                &clipped.shape,
                egui::Shape::Rect(rect) if rect.fill == expected_fill
            )
        }));
    }
}

#[test]
fn smoke_default_ui_fonts_are_bundled() {
    assert!(!egui::FontDefinitions::default().font_data.is_empty());
}

#[test]
fn contract_user_data_directory_is_stageswap() {
    assert_eq!(
        local_data_directory()
            .file_name()
            .and_then(|name| name.to_str()),
        Some("StageSwap")
    );
}

#[test]
fn contract_window_title_includes_the_package_version() {
    assert_eq!(
        WINDOW_TITLE,
        format!("StageSwap - v{}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(APP_VERSION_LABEL, format!("v{}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn contract_minimized_start_is_reasserted_after_eframe_initialization() {
    assert_eq!(initial_visibility_override(true, true), None);
    assert_eq!(initial_visibility_override(false, true), Some(false));
    assert_eq!(initial_visibility_override(false, false), Some(true));
}

#[test]
fn contract_visible_ui_and_hidden_logic_use_distinct_repaint_cadences() {
    assert_eq!(repaint_interval(true), VISIBLE_REFRESH);
    assert_eq!(repaint_interval(false), HIDDEN_REFRESH);
    assert_eq!(VISIBLE_REFRESH, Duration::from_nanos(1_000_000_000 / 30));
    assert_eq!(HIDDEN_REFRESH, Duration::from_millis(250));
}

#[cfg(not(windows))]
#[test]
fn contract_hiding_window_suspends_preview_work_and_releases_source_frames() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    let now = Instant::now();
    let frame = Arc::new(Frame::placeholder(
        stageswap_core::Size::new(1280, 720),
        0xff00_0001,
        1,
        0,
        now,
    ));
    let pixels = frame.pixels_arc();
    let converter = PreviewConverter::new(PreviewKind::Webcam.key());
    converter.submit(
        Arc::clone(&frame),
        [240, 135],
        EMBEDDED_PREVIEW_REFRESH,
        now,
    );
    app.previews
        .converters
        .insert(PreviewKind::Webcam, converter);
    app.previews.enlarged = Some(PreviewKind::Webcam);
    let output_sequence = app
        .runtime
        .snapshot()
        .previews
        .final_output
        .expect("the stopped runtime publishes an output frame")
        .sequence;

    app.hide_window(&egui::Context::default());
    drop(frame);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Arc::strong_count(&pixels) > 1 && Instant::now() < deadline {
        thread::yield_now();
    }
    assert!(!app.window_visible);
    assert_eq!(app.previews.enlarged, None);
    assert_eq!(Arc::strong_count(&pixels), 1);
    let runtime_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let snapshot = app.runtime.snapshot();
        if snapshot
            .previews
            .final_output
            .as_ref()
            .is_some_and(|output| output.sequence > output_sequence)
        {
            break;
        }
        assert!(
            Instant::now() < runtime_deadline,
            "runtime output stopped while the window was hidden"
        );
        thread::yield_now();
    }
    assert!(
        app.previews
            .converters
            .get(&PreviewKind::Webcam)
            .unwrap()
            .take_ready()
            .is_none()
    );
}

#[test]
fn contract_preview_conversion_preserves_size_and_bgra_channels() {
    let frame = Frame::new(
        vec![3, 2, 1, 255, 30, 20, 10, 255].into(),
        stageswap_core::Size::new(2, 1),
        8,
        1,
        0,
        Instant::now(),
    )
    .unwrap();
    let image = frame_image(&frame, [2, 1]);
    assert_eq!(image.size, [2, 1]);
    assert_eq!(image.as_raw(), &[1, 2, 3, 255, 10, 20, 30, 255]);
}

#[test]
fn contract_preview_texture_is_capped_to_its_display_size() {
    let frame = Frame::placeholder(
        stageswap_core::Size::new(1280, 720),
        0xff03_0201,
        1,
        0,
        Instant::now(),
    );
    let size = preview_texture_size(
        &frame,
        egui::vec2(320.0, 180.0),
        1.0,
        DASHBOARD_PREVIEW_TEXTURE_LIMIT,
    );
    assert_eq!(size, [240, 135]);
    let image = frame_image(&frame, size);
    assert_eq!(image.size, size);
    assert_eq!(image.pixels[0], Color32::from_rgb(3, 2, 1));

    let high_dpi = preview_texture_size(
        &frame,
        egui::vec2(640.0, 360.0),
        2.0,
        DASHBOARD_PREVIEW_TEXTURE_LIMIT,
    );
    assert_eq!(high_dpi, [240, 135]);

    let enlarged = preview_texture_size(
        &frame,
        egui::vec2(1280.0, 720.0),
        1.0,
        ENLARGED_PREVIEW_TEXTURE_LIMIT,
    );
    assert_eq!(enlarged, [1280, 720]);
}

#[cfg(not(debug_assertions))]
#[test]
fn flow_four_thirty_fps_embedded_previews_fit_inside_one_cpu_second() {
    let frame = Frame::placeholder(
        stageswap_core::Size::new(1280, 720),
        0xff03_0201,
        1,
        0,
        Instant::now(),
    );
    let started = Instant::now();
    for _ in 0..120 {
        std::hint::black_box(frame_image(&frame, [240, 135]));
    }
    assert!(
        started.elapsed() <= Duration::from_secs(1),
        "120 embedded preview conversions exceeded one CPU second"
    );
}

#[cfg(not(windows))]
#[test]
fn flow_each_dashboard_preview_can_expand_even_without_a_frame() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    let context = egui::Context::default();
    let screen_rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 280.0));

    for kind in [
        PreviewKind::Webcam,
        PreviewKind::Screen,
        PreviewKind::Reference,
        PreviewKind::Output,
    ] {
        app.previews.enlarged = None;
        let mut preview_rect = Rect::NOTHING;
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..egui::RawInput::default()
            },
            |ui| {
                preview_rect =
                    app.preview_cell(ui, kind, None, [320.0, 180.0], Source::Camera, Some(30));
            },
        );
        let position = preview_rect.center();
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..egui::RawInput::default()
            },
            |ui| {
                app.preview_cell(ui, kind, None, [320.0, 180.0], Source::Camera, Some(30));
            },
        );

        assert_eq!(app.previews.enlarged, Some(kind));
    }
}

#[cfg(not(windows))]
#[test]
fn flow_enlarged_dashboard_preview_closes_on_click_escape_and_navigation() {
    fn dashboard_input(event: egui::Event) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 720.0))),
            events: vec![event],
            ..egui::RawInput::default()
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Settings(SettingsTab::General),
    });
    app.view = AppView::Dashboard;
    let context = egui::Context::default();

    app.previews.enlarged = Some(PreviewKind::Webcam);
    let position = Pos2::new(20.0, 20.0);
    let _ = context.run_ui(
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 720.0))),
            ..egui::RawInput::default()
        },
        |ui| app.dashboard(ui),
    );
    assert_eq!(app.previews.enlarged, None);

    app.previews.enlarged = Some(PreviewKind::Screen);
    let _ = context.run_ui(
        dashboard_input(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }),
        |ui| app.dashboard(ui),
    );
    assert_eq!(app.previews.enlarged, None);

    app.previews.enlarged = Some(PreviewKind::Output);
    app.open_settings();
    assert_eq!(app.view, AppView::Settings);
    assert_eq!(app.previews.enlarged, None);
}

#[test]
fn contract_preview_converter_collapses_pending_jobs_to_latest_frame() {
    let converter = PreviewConverter::new(PreviewKind::Output.key());
    let now = Instant::now();
    for sequence in 1..=12 {
        converter.submit(
            Arc::new(Frame::placeholder(
                stageswap_core::Size::new(1280, 720),
                0xff00_0000 | sequence,
                u64::from(sequence),
                0,
                now,
            )),
            [240, 135],
            Duration::ZERO,
            now,
        );
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(prepared) = converter.take_ready()
            && prepared.id.sequence == 12
        {
            assert_eq!(prepared.size, [240, 135]);
            assert_eq!(prepared.image.pixels[0], Color32::from_rgb(0, 0, 12));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "latest preview frame was not prepared"
        );
        thread::yield_now();
    }
}

#[test]
fn contract_preview_contours_mark_live_output_and_active_source() {
    assert_eq!(
        preview_contour(PreviewKind::Output, Source::Camera, OutputLayout::Camera),
        PreviewContour::Live
    );
    assert_eq!(
        preview_contour(PreviewKind::Webcam, Source::Camera, OutputLayout::Camera),
        PreviewContour::Active
    );
    assert_eq!(
        preview_contour(PreviewKind::Screen, Source::Camera, OutputLayout::Camera),
        PreviewContour::Neutral
    );
    assert_eq!(
        preview_contour(PreviewKind::Screen, Source::Screen, OutputLayout::Screen),
        PreviewContour::Active
    );
    assert_eq!(
        preview_contour(
            PreviewKind::Webcam,
            Source::Placeholder,
            OutputLayout::Placeholder,
        ),
        PreviewContour::Neutral
    );
    assert_eq!(
        preview_contour(PreviewKind::Reference, Source::Screen, OutputLayout::Screen,),
        PreviewContour::Neutral
    );
    for kind in [PreviewKind::Webcam, PreviewKind::Screen] {
        assert_eq!(
            preview_contour(kind, Source::Camera, OutputLayout::WebcamMainScreenPip,),
            PreviewContour::Active
        );
    }
}

#[test]
fn contract_health_states_have_lifecycle_order_icons_and_semantic_colors() {
    assert_eq!(
        HEALTH_STATES,
        [
            DeviceState::Initializing,
            DeviceState::Ready,
            DeviceState::Unavailable,
            DeviceState::Failed,
        ]
    );
    assert_eq!(device_state_icon(DeviceState::Initializing), UiIcon::Loader);
    assert_eq!(device_state_icon(DeviceState::Ready), UiIcon::Check);
    assert_eq!(
        device_state_icon(DeviceState::Unavailable),
        UiIcon::Unavailable
    );
    assert_eq!(device_state_icon(DeviceState::Failed), UiIcon::Error);
    assert_eq!(
        indicator_palette(device_state_tone(DeviceState::Ready), 1.0).1,
        ACTIVE_GREEN
    );
    assert_eq!(
        indicator_palette(device_state_tone(DeviceState::Initializing), 1.0).1,
        TRANSITION_AMBER
    );
    assert_eq!(
        indicator_palette(device_state_tone(DeviceState::Failed), 1.0).1,
        LIVE_RED
    );
    assert_eq!(
        indicator_palette(device_state_tone(DeviceState::Unavailable), 1.0).1,
        LIVE_RED
    );
    let inactive = indicator_palette(device_state_tone(DeviceState::Ready), 0.0);
    assert_eq!(inactive.1, Color32::from_rgb(91, 97, 108));
    assert_eq!(
        inactive,
        indicator_palette(device_state_tone(DeviceState::Failed), 0.0)
    );
}

#[test]
fn flow_not_matching_is_warning_amber_but_missing_reference_is_error_red() {
    assert_eq!(
        detection_indicator_tone(DetectionState::NotMatching),
        IndicatorTone::Amber
    );
    assert_eq!(
        settings_detection_style(DetectionState::NotMatching).2,
        TRANSITION_AMBER
    );
    assert_eq!(
        detection_indicator_tone(DetectionState::ReferenceMissing),
        IndicatorTone::Red
    );
    assert_eq!(
        settings_detection_style(DetectionState::ReferenceMissing).2,
        LIVE_RED
    );
}

#[test]
fn contract_fps_overlays_use_runtime_metrics_for_all_live_pipelines() {
    assert!(PreviewKind::Webcam.shows_fps());
    assert!(PreviewKind::Screen.shows_fps());
    assert!(PreviewKind::Output.shows_fps());
    assert!(!PreviewKind::Reference.shows_fps());
    assert!(!PreviewOptions::settings("missing").show_fps);
    assert!(PreviewOptions::dashboard(PreviewKind::Webcam, Some(30)).show_fps);
    let output = PreviewOptions::dashboard(PreviewKind::Output, Some(30));
    assert!(output.show_fps);
    assert_eq!(output.fps, Some(30));
    assert_eq!(output.style, PreviewStyle::Card);
    assert_eq!(output.texture_limit, [240, 135]);
    assert_eq!(output.refresh_interval, EMBEDDED_PREVIEW_REFRESH);
    let enlarged = PreviewOptions::enlarged(PreviewKind::Output, Some(30));
    assert_eq!(enlarged.texture_limit, [1280, 720]);
    assert_eq!(enlarged.refresh_interval, ENLARGED_PREVIEW_REFRESH);
    assert_eq!(enlarged.style, PreviewStyle::CinemaPeek);
    assert_eq!(CINEMA_PEEK_SCALE, 0.92);
}

#[cfg(not(windows))]
#[test]
fn smoke_enlarged_dashboard_preview_keeps_metadata_without_contour() {
    fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, texts);
                }
            }
            _ => {}
        }
    }

    fn has_stroke(shape: &egui::Shape) -> bool {
        match shape {
            egui::Shape::Rect(rect) => rect.stroke.width > 0.0,
            egui::Shape::Vec(shapes) => shapes.iter().any(has_stroke),
            _ => false,
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Settings(SettingsTab::General),
    });
    app.view = AppView::Dashboard;
    app.previews.enlarged = Some(PreviewKind::Output);
    let context = egui::Context::default();
    ui_icon::install_fonts(&context);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 720.0))),
            ..egui::RawInput::default()
        },
        |ui| app.dashboard(ui),
    );
    let mut texts = Vec::new();
    for clipped in &output.shapes {
        collect_text(&clipped.shape, &mut texts);
    }
    assert!(texts.iter().any(|text| text == "ZOOM OUTPUT"));
    assert!(texts.iter().any(|text| text == "LIVE"));
    assert!(texts.iter().any(|text| text == "30 FPS"));
    assert!(
        !output
            .shapes
            .iter()
            .any(|clipped| has_stroke(&clipped.shape))
    );
}

#[cfg(not(windows))]
#[test]
fn smoke_ui_preview_cli_selects_each_settings_page() {
    for tab in SettingsTab::ALL {
        let args = vec![
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            tab.preview_name().to_owned(),
        ];
        assert_eq!(
            parse_ui_preview_request(&args).unwrap(),
            Some(UiPreviewRequest {
                target: UiPreviewTarget::Settings(tab),
            })
        );
    }
    for step in SetupStep::ALL {
        let args = vec![
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            format!("setup-{}", step.number()),
        ];
        assert_eq!(
            parse_ui_preview_request(&args).unwrap(),
            Some(UiPreviewRequest {
                target: UiPreviewTarget::Setup(step),
            })
        );
    }
    let default_page = vec!["StageSwap".to_owned(), "--ui-preview".to_owned()];
    assert_eq!(
        parse_ui_preview_request(&default_page).unwrap(),
        Some(UiPreviewRequest {
            target: UiPreviewTarget::Settings(SettingsTab::General),
        })
    );
    let localized = vec![
        "StageSwap".to_owned(),
        "--ui-preview".to_owned(),
        "general".to_owned(),
        "--ui-language".to_owned(),
        "fr-CA".to_owned(),
    ];
    assert_eq!(ui_preview_locale(&localized).unwrap(), Locale::French);
    let unsupported = vec![
        "StageSwap".to_owned(),
        "--ui-preview".to_owned(),
        "--ui-language".to_owned(),
        "de-DE".to_owned(),
    ];
    assert!(parse_ui_preview_request(&unsupported).is_err());
    for (name, kind) in [
        ("dialog-exit", AppDialogKind::Exit),
        ("dialog-clear-logs", AppDialogKind::ClearLogs),
        ("dialog-reference-capture", AppDialogKind::ReferenceCapture),
        ("dialog-admin", AppDialogKind::Admin),
        (
            "dialog-replace-baseline",
            AppDialogKind::ReplaceAdminBaseline,
        ),
        ("dialog-load-admin-config", AppDialogKind::LoadAdminConfig),
        ("dialog-remove-baseline", AppDialogKind::RemoveAdminBaseline),
    ] {
        let args = vec![
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            name.to_owned(),
        ];
        assert_eq!(
            parse_ui_preview_request(&args).unwrap(),
            Some(UiPreviewRequest {
                target: UiPreviewTarget::Dialog(kind),
            })
        );
    }
    for (name, state) in [
        ("notifications", NotificationPreviewState::Stacked),
        ("notifications-empty", NotificationPreviewState::Empty),
        ("notifications-critical", NotificationPreviewState::Critical),
        ("notifications-updates", NotificationPreviewState::Updates),
    ] {
        let args = vec![
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            name.to_owned(),
        ];
        assert_eq!(
            parse_ui_preview_request(&args).unwrap(),
            Some(UiPreviewRequest {
                target: UiPreviewTarget::Notifications(state),
            })
        );
    }
    assert!(
        parse_ui_preview_request(&["StageSwap".to_owned()])
            .unwrap()
            .is_none()
    );
    assert!(
        parse_ui_preview_request(&[
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            "unknown".to_owned(),
        ])
        .is_err()
    );
    assert!(
        parse_ui_preview_request(&[
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            "setup-6".to_owned(),
        ])
        .is_err()
    );
    for (value, expected) in [
        ("captured", SetupReferencePreviewState::Captured),
        ("empty", SetupReferencePreviewState::Empty),
        ("review", SetupReferencePreviewState::Review),
        ("missing-screen", SetupReferencePreviewState::MissingScreen),
    ] {
        assert_eq!(
            parse_setup_reference_preview_state(&[
                "StageSwap".to_owned(),
                "--ui-setup-reference-state".to_owned(),
                value.to_owned(),
            ])
            .unwrap(),
            Some(expected)
        );
    }
    assert!(
        parse_setup_reference_preview_state(&[
            "StageSwap".to_owned(),
            "--ui-setup-reference-state".to_owned(),
            "unknown".to_owned(),
        ])
        .is_err()
    );
}

#[cfg(not(windows))]
#[test]
fn smoke_ui_screenshot_cli_requires_an_absolute_png_path() {
    let path = "/tmp/stageswap-setup.png";
    assert_eq!(
        parse_ui_screenshot_path(&[
            "StageSwap".to_owned(),
            "--ui-screenshot".to_owned(),
            path.to_owned(),
        ])
        .unwrap(),
        Some(PathBuf::from(path))
    );
    assert!(
        parse_ui_screenshot_path(&[
            "StageSwap".to_owned(),
            "--ui-screenshot".to_owned(),
            "setup.png".to_owned(),
        ])
        .is_err()
    );
    assert!(
        parse_ui_screenshot_path(&[
            "StageSwap".to_owned(),
            "--ui-screenshot".to_owned(),
            "/tmp/setup.jpg".to_owned(),
        ])
        .is_err()
    );
}

#[cfg(not(windows))]
#[test]
fn flow_ui_preview_seeds_alerts_and_repeats_activity_notifications() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
    });

    assert_eq!(app.notifications.entries().count(), 3);
    assert_eq!(app.notifications.toasts().count(), 2);
    assert_eq!(
        app.notifications
            .entries()
            .filter(|entry| entry.tone == NotificationTone::Critical)
            .count(),
        2
    );
    assert!(
        app.notifications
            .entries()
            .any(|entry| entry.source == NotificationSource::Updates)
    );

    let first_due = app
        .ui_preview
        .as_ref()
        .and_then(|preview| preview.next_notification_at)
        .unwrap();
    let context = egui::Context::default();
    app.tick_ui_preview_notifications(&context, first_due);

    assert_eq!(app.notifications.entries().count(), 4);
    assert_eq!(
        app.notifications
            .entries()
            .filter(|entry| entry.source == NotificationSource::Preview)
            .count(),
        1
    );
    assert!(
        app.notifications
            .entries()
            .any(|entry| entry.tone == NotificationTone::Information)
    );
    assert_eq!(app.notifications.toasts().count(), 2);

    let second_due = app
        .ui_preview
        .as_ref()
        .and_then(|preview| preview.next_notification_at)
        .unwrap();
    assert_eq!(second_due, first_due + UI_PREVIEW_NOTIFICATION_INTERVAL);
    app.tick_ui_preview_notifications(&context, second_due);
    assert_eq!(
        app.notifications
            .entries()
            .filter(|entry| entry.source == NotificationSource::Preview)
            .count(),
        2
    );
}

#[cfg(not(windows))]
#[test]
fn flow_ui_preview_fixed_notification_states_do_not_start_periodic_activity() {
    for state in [
        NotificationPreviewState::Empty,
        NotificationPreviewState::Critical,
        NotificationPreviewState::Updates,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Notifications(state),
        });

        assert!(
            app.ui_preview
                .as_ref()
                .unwrap()
                .next_notification_at
                .is_none()
        );
        assert!(
            !app.notifications
                .entries()
                .any(|entry| entry.source == NotificationSource::Preview)
        );
    }
}

#[cfg(not(windows))]
#[test]
fn flow_notification_popover_closes_on_outside_click() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
    });
    let context = egui::Context::default();
    ui_icon::install_fonts(&context);
    let viewport = egui::vec2(1280.0, 720.0);

    let render = |app: &mut SwitcherApp, input: egui::RawInput| {
        let _ = context.run_ui(input, |ui| {
            app.root_ui(ui);
        });
    };

    app.notification_center_open = true;
    render(
        &mut app,
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
            ..egui::RawInput::default()
        },
    );
    assert!(app.notification_center_open);

    render(
        &mut app,
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
            events: vec![
                egui::Event::PointerMoved(Pos2::new(900.0, 100.0)),
                egui::Event::PointerButton {
                    pos: Pos2::new(900.0, 100.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: Pos2::new(900.0, 100.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..egui::RawInput::default()
        },
    );
    assert!(!app.notification_center_open);
}

#[cfg(not(windows))]
#[test]
fn smoke_setup_demo_preview_state_is_deterministic() {
    assert_eq!(
        parse_setup_demo_preview_state(&[
            "StageSwap".to_owned(),
            "--ui-setup-demo-state".to_owned(),
            "matching".to_owned(),
        ])
        .unwrap(),
        Some(SetupDemoPreviewState::Matching)
    );
    assert_eq!(
        parse_setup_demo_preview_state(&[
            "StageSwap".to_owned(),
            "--ui-setup-demo-state".to_owned(),
            "non-matching".to_owned(),
        ])
        .unwrap(),
        Some(SetupDemoPreviewState::Changed)
    );
    assert!(
        parse_setup_demo_preview_state(&[
            "StageSwap".to_owned(),
            "--ui-setup-demo-state".to_owned(),
            "unknown".to_owned(),
        ])
        .is_err()
    );
}

#[test]
fn flow_setup_reference_requires_a_decision_only_for_unresolved_candidates() {
    let now = Instant::now();
    assert!(!setup_reference_requires_decision(
        SetupReferenceCaptureState::Idle
    ));
    assert!(setup_reference_requires_decision(
        SetupReferenceCaptureState::PreparingCandidate { started_at: now }
    ));
    assert!(setup_reference_requires_decision(
        SetupReferenceCaptureState::CapturingCandidate {
            started_at: now,
            previous_candidate_sequence: None,
        }
    ));
    assert!(setup_reference_requires_decision(
        SetupReferenceCaptureState::Review { captured_at: now }
    ));
    assert!(setup_reference_requires_decision(
        SetupReferenceCaptureState::SavingCandidate {
            started_at: now,
            previous_reference_sequence: None,
        }
    ));
    assert!(setup_reference_requires_decision(
        SetupReferenceCaptureState::SaveFailed {
            previous_reference_sequence: None,
        }
    ));
    assert!(!setup_reference_requires_decision(
        SetupReferenceCaptureState::Confirmed
    ));
}

#[test]
fn flow_setup_reference_capture_action_distinguishes_first_capture_from_retry() {
    assert_eq!(
        setup_reference_capture_label(false),
        "Capture reference image"
    );
    assert_eq!(setup_reference_capture_label(true), "Capture again");
}

#[cfg(not(windows))]
#[test]
fn smoke_ui_preview_frame_decodes_embedded_images_as_bgra() {
    let bytes = include_bytes!("../../assets/setup-webcam-example.png");
    let source = image::load_from_memory(bytes).unwrap().to_rgba8();
    let frame = ui_preview_frame(7, "setup-webcam-example", bytes);

    assert_eq!(
        frame.size,
        stageswap_core::Size::new(source.width(), source.height())
    );
    assert_eq!(frame.stride, source.width() * 4);
    assert_eq!(frame.sequence, 7);
    let source_pixel = &source.as_raw()[..4];
    assert_eq!(
        &frame.pixels()[..4],
        &[
            source_pixel[2],
            source_pixel[1],
            source_pixel[0],
            source_pixel[3]
        ]
    );
}

#[cfg(not(windows))]
#[test]
fn smoke_ui_preview_snapshot_has_realistic_ready_inputs_and_frames() {
    let snapshot = ui_preview_snapshot();
    assert_eq!(snapshot.run_state, RunState::Running);
    assert_eq!(snapshot.mode, OutputMode::Automatic);
    assert_eq!(snapshot.detection, DetectionState::Matching);
    assert_eq!(snapshot.webcam_state, DeviceState::Ready);
    assert_eq!(snapshot.screen_state, DeviceState::Ready);
    assert_eq!(snapshot.virtual_camera_state, DeviceState::Ready);
    assert!(snapshot.previews.webcam.is_some());
    assert!(snapshot.previews.screen.is_some());
    assert!(snapshot.previews.reference.is_some());
    assert!(snapshot.previews.final_output.is_some());
    assert_eq!(snapshot.video_devices.len(), 2);
    assert_eq!(snapshot.monitors.len(), 2);
    let webcam = snapshot.previews.webcam.as_ref().unwrap();
    let screen = snapshot.previews.screen.as_ref().unwrap();
    let reference = snapshot.previews.reference.as_ref().unwrap();
    let output = snapshot.previews.final_output.as_ref().unwrap();
    assert_eq!(webcam.size, stageswap_core::Size::new(1672, 941));
    assert_eq!(screen.size, stageswap_core::Size::new(1672, 941));
    assert!(Arc::ptr_eq(webcam, output));
    assert!(Arc::ptr_eq(screen, reference));
}

#[cfg(not(windows))]
#[test]
fn smoke_primary_ui_surfaces_render_contained_without_panic() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Settings(SettingsTab::General),
    });
    let context = egui::Context::default();
    ui_icon::install_fonts(&context);
    let viewport = egui::vec2(MIN_WINDOW_HEIGHT * WINDOW_ASPECT_RATIO, MIN_WINDOW_HEIGHT);

    let render_root = |app: &mut SwitcherApp, viewport: Vec2| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
            ..egui::RawInput::default()
        };
        let output = context.run_ui(input, |ui| {
            let content = app.root_ui(ui);
            assert!(ui.max_rect().contains_rect(content));
        });
        assert!(!output.shapes.is_empty());
    };

    app.active_dialog = None;
    app.view = AppView::Dashboard;
    render_root(&mut app, viewport);

    for state in [
        NotificationPreviewState::Empty,
        NotificationPreviewState::Critical,
        NotificationPreviewState::Updates,
        NotificationPreviewState::Stacked,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut notification_app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Notifications(state),
        });
        notification_app.active_dialog = None;
        render_root(&mut notification_app, viewport);
    }

    for locale in Locale::ALL {
        let directory = tempfile::tempdir().unwrap();
        let mut config = ui_preview_config();
        config.interface_language = locale.tag().into();
        let mut notification_app =
            SwitcherApp::new(config, Vec::new(), ConfigStore::new(directory.path()))
                .with_ui_preview(UiPreviewRequest {
                    target: UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
                });
        notification_app.active_dialog = None;
        render_root(&mut notification_app, egui::vec2(1066.0, 600.0));
    }

    for tab in SettingsTab::ALL {
        app.active_dialog = None;
        app.view = AppView::Settings;
        app.settings_tab = tab;
        app.settings_opened_at = None;
        app.settings_section_changed_at = None;
        render_root(&mut app, viewport);
    }

    for step in SetupStep::ALL {
        app.active_dialog = None;
        app.view = AppView::SetupGuide;
        app.setup_session = Some(SetupSession::preview(step));
        app.setup_reference_capture = SetupReferenceCaptureState::Idle;
        render_root(&mut app, viewport);
    }

    app.view = AppView::Settings;
    app.admin_profile_status = Some(AdminProfileStatus {
        auto_restore_on_launch: true,
        reference_included: true,
    });
    for kind in [
        AppDialogKind::Exit,
        AppDialogKind::ClearLogs,
        AppDialogKind::ReferenceCapture,
        AppDialogKind::Admin,
    ] {
        app.active_dialog = Some(ActiveDialog {
            kind,
            opened_at: Instant::now() - DIALOG_ENTRANCE_DURATION,
            focus_safe_action: true,
        });
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
            ..egui::RawInput::default()
        };
        let output = context.run_ui(input, |_ui| app.dialog(&context));
        assert!(!output.shapes.is_empty());
        assert!(app.dialog_is(kind));
    }

    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..egui::RawInput::default()
    };
    let _ = context.run_ui(input, |_ui| app.dialog(&context));
    assert!(app.active_dialog.is_none());
}

#[cfg(not(windows))]
#[test]
fn flow_ui_preview_reference_commands_exercise_capture_retake_and_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Setup(SetupStep::Reference),
    });
    let original = app.snapshot().previews.reference.clone().unwrap();

    app.setup_reference_capture = SetupReferenceCaptureState::CapturingCandidate {
        started_at: Instant::now(),
        previous_candidate_sequence: None,
    };
    app.send_setup_reference_command(Command::CaptureReferenceCandidate);
    let captured = app.snapshot();
    let first_candidate = captured.previews.reference_candidate.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        captured.previews.reference.as_ref().unwrap(),
        &original
    ));
    assert_eq!(
        first_candidate.pixels(),
        captured.previews.screen.as_ref().unwrap().pixels()
    );
    assert_ne!(first_candidate.sequence, original.sequence);
    app.update_setup_reference_capture(&captured);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Review { .. }
    ));

    app.send_setup_reference_command(Command::DiscardReferenceCandidate);
    let discarded = app.snapshot();
    assert!(discarded.previews.reference_candidate.is_none());
    assert!(Arc::ptr_eq(
        discarded.previews.reference.as_ref().unwrap(),
        &original
    ));

    app.send_setup_reference_command(Command::CaptureReferenceCandidate);
    let candidate = app.snapshot().previews.reference_candidate.clone().unwrap();
    app.setup_reference_capture = SetupReferenceCaptureState::SavingCandidate {
        started_at: Instant::now(),
        previous_reference_sequence: Some(original.sequence),
    };
    app.send_setup_reference_command(Command::ConfirmReferenceCandidate);
    let confirmed = app.snapshot();
    assert!(confirmed.previews.reference_candidate.is_none());
    assert!(Arc::ptr_eq(
        confirmed.previews.reference.as_ref().unwrap(),
        &candidate
    ));
    app.update_setup_reference_capture(&confirmed);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Confirmed
    ));
}

#[test]
fn flow_version_picker_keeps_latest_offer_and_requires_an_explicit_installable_selection() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        AppConfig {
            update_channel: UpdateChannel::Beta,
            ..AppConfig::default()
        },
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    let releases = update::preview_versions();
    let latest = releases[0].clone();
    let older = releases[1].clone();
    app.updates.status = UpdateStatus::Available(latest.clone());
    app.updates.versions = update::VersionCatalog::Ready(releases);
    assert!(app.selected_version_release().is_none());
    app.updates.selected_version = Some(older.version);
    assert_eq!(app.selected_version_release(), Some(older.clone()));
    assert_eq!(app.updates.status, UpdateStatus::Available(latest.clone()));
    app.updates.picker_open = true;
    let context = egui::Context::default();
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        app.update_version_picker(ui.ctx())
    });
    let _ = context.run_ui(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
            ..egui::RawInput::default()
        },
        |ui| app.update_version_picker(ui.ctx()),
    );
    assert!(!app.updates.picker_open);
    assert!(app.updates.selected_version.is_none());
    assert_eq!(app.updates.status, UpdateStatus::Available(latest));

    let mut current = older.clone();
    current.version = update::ReleaseVersion::parse(env!("CARGO_PKG_VERSION")).unwrap();
    app.updates.selected_version = Some(current.version);
    app.updates.versions = update::VersionCatalog::Ready(vec![current]);
    assert!(app.selected_version_release().is_none());
    app.updates.selected_version = Some(older.version);
    app.updates.versions = update::VersionCatalog::Failed("offline".into());
    assert!(app.selected_version_release().is_none());
    app.updates.versions = update::VersionCatalog::Loading;
    assert!(app.update_busy());
    assert!(app.selected_version_release().is_none());
    app.updates.versions = update::VersionCatalog::Ready(vec![older]);
    app.config.update_channel = UpdateChannel::Stable;
    assert!(app.selected_version_release().is_none());
}

#[test]
fn flow_busy_command_feedback_preserves_reference_dialog_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        AppConfig::default(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    app.setup_reference_capture = SetupReferenceCaptureState::Review {
        captured_at: Instant::now(),
    };

    assert!(!app.handle_command_dispatch(CommandDispatch::Busy));

    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Review { .. }
    ));
    assert!(
        app.notifications
            .entries()
            .any(|entry| entry.source == NotificationSource::Command)
    );
}

#[cfg(not(windows))]
#[test]
fn flow_shared_reference_modal_preserves_cancels_and_confirms_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Setup(SetupStep::Reference),
    });
    let original = app.snapshot().previews.reference.clone().unwrap();

    app.begin_reference_capture();
    assert!(app.dialog_is(AppDialogKind::ReferenceCapture));
    let preparing = app.snapshot();
    app.update_setup_reference_capture(&preparing);
    let captured = app.snapshot();
    assert!(Arc::ptr_eq(
        captured.previews.reference.as_ref().unwrap(),
        &original
    ));
    assert!(captured.previews.reference_candidate.is_some());
    let first_candidate_sequence = captured
        .previews
        .reference_candidate
        .as_ref()
        .unwrap()
        .sequence;
    app.update_setup_reference_capture(&captured);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Review { .. }
    ));

    app.prepare_reference_capture();
    let cleared = app.snapshot();
    assert!(cleared.previews.reference_candidate.is_none());
    app.update_setup_reference_capture(&cleared);
    let retaken = app.snapshot();
    assert_eq!(
        retaken
            .previews
            .reference_candidate
            .as_ref()
            .unwrap()
            .sequence,
        first_candidate_sequence
    );
    app.update_setup_reference_capture(&retaken);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Review { .. }
    ));

    app.cancel_reference_capture();
    let cancelled = app.snapshot();
    assert!(app.active_dialog.is_none());
    assert!(cancelled.previews.reference_candidate.is_none());
    assert!(Arc::ptr_eq(
        cancelled.previews.reference.as_ref().unwrap(),
        &original
    ));

    app.begin_reference_capture();
    let preparing = app.snapshot();
    app.update_setup_reference_capture(&preparing);
    let captured = app.snapshot();
    let candidate = captured
        .previews
        .reference_candidate
        .as_ref()
        .unwrap()
        .clone();
    app.update_setup_reference_capture(&captured);
    app.confirm_reference_candidate();
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::SavingCandidate { .. }
    ));
    app.cancel_reference_capture();
    assert!(app.dialog_is(AppDialogKind::ReferenceCapture));

    let confirmed = app.snapshot();
    assert!(confirmed.previews.reference_candidate.is_none());
    assert!(Arc::ptr_eq(
        confirmed.previews.reference.as_ref().unwrap(),
        &candidate
    ));
    app.setup_animations_enabled = false;
    let context = egui::Context::default();
    let _ = context.run_ui(egui::RawInput::default(), |_ui| app.dialog(&context));
    assert!(app.active_dialog.is_none());
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Confirmed
    ));
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::Ready)
    );
}

#[cfg(not(windows))]
#[test]
fn flow_reference_confirmation_does_not_advance_from_settings() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        ui_preview_config(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    )
    .with_ui_preview(UiPreviewRequest {
        target: UiPreviewTarget::Settings(SettingsTab::Matching),
    });
    app.setup_reference_capture = SetupReferenceCaptureState::Confirmed;
    app.open_dialog(AppDialogKind::ReferenceCapture);

    let context = egui::Context::default();
    let _ = context.run_ui(egui::RawInput::default(), |_ui| app.dialog(&context));

    assert!(app.active_dialog.is_none());
    assert!(app.setup_session.is_none());
    assert!(matches!(app.view, AppView::Settings));
    assert_eq!(app.settings_tab, SettingsTab::Matching);
}

#[test]
fn flow_clearing_logs_requires_explicit_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        AppConfig::default(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    app.log.write("info", "test", "KEEP", "keep this entry");
    let before = directory.path().join("before.jsonl");

    app.confirm_log_clear();
    app.log.export_to(&before).unwrap();
    assert!(std::fs::read_to_string(&before).unwrap().contains("KEEP"));

    app.request_log_clear();
    assert!(app.dialog_is(AppDialogKind::ClearLogs));
    app.dismiss_dialog();
    assert!(app.active_dialog.is_none());
    let after_cancel = directory.path().join("after-cancel.jsonl");
    app.log.export_to(&after_cancel).unwrap();
    assert!(
        std::fs::read_to_string(&after_cancel)
            .unwrap()
            .contains("KEEP")
    );

    app.request_log_clear();
    app.confirm_log_clear();
    assert!(app.active_dialog.is_none());
    let after_clear = directory.path().join("after-clear.jsonl");
    app.log.export_to(&after_clear).unwrap();
    let contents = std::fs::read_to_string(&after_clear).unwrap();
    assert!(!contents.contains("KEEP"));
    assert!(contents.contains("LOGS_CLEARED"));
}

#[test]
fn flow_settings_save_is_debounced_and_back_flushes_pending_changes() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
    let started_at = Instant::now();
    app.config.cursor_visible = true;
    app.config.selected_video_device_id = "new-camera".into();
    app.config.still_image_pip_enabled = true;
    app.config.still_image_pip_delay_seconds = 120;
    app.config.still_image_pip_layout = StillImagePipLayout::ScreenMain;
    app.config.still_image_pip_size = StillImagePipSize::Large;
    app.pending_settings_save = Some(started_at);

    assert!(!app.settings_save_due(started_at + SETTINGS_SAVE_DEBOUNCE / 2));
    assert!(app.settings_save_due(started_at + SETTINGS_SAVE_DEBOUNCE));

    app.view = AppView::Settings;
    app.close_settings();
    app.wait_for_settings_save();
    assert_eq!(app.view, AppView::Dashboard);
    assert!(app.pending_settings_save.is_none());
    let saved = store.load().config;
    assert!(saved.cursor_visible);
    assert!(saved.still_image_pip_enabled);
    assert_eq!(saved.still_image_pip_delay_seconds, 120);
    assert_eq!(
        saved.still_image_pip_layout,
        StillImagePipLayout::ScreenMain
    );
    assert_eq!(saved.still_image_pip_size, StillImagePipSize::Large);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if app.runtime.snapshot().selected_video_device_id == "new-camera" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "runtime did not receive the flushed settings"
        );
        std::thread::yield_now();
    }
}

#[test]
fn flow_accepted_monitor_selection_is_persisted_by_label() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
    let snapshot = AppSnapshot {
        selected_monitor: Some(stageswap_core::MonitorDescriptor {
            display_name: r"\\.\DISPLAY2".into(),
            label: "Stage Display".into(),
            ..stageswap_core::MonitorDescriptor::default()
        }),
        ..AppSnapshot::default()
    };

    app.sync_selected_monitor_preference(&snapshot);
    app.wait_for_settings_save();

    assert_eq!(app.config.selected_monitor_label, "Stage Display");
    assert_eq!(store.load().config.selected_monitor_label, "Stage Display");
}

#[test]
fn flow_opening_settings_does_not_rescan_when_automatic_rescans_are_disabled() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        AppConfig {
            automatic_monitor_rescans: false,
            ..AppConfig::default()
        },
        Vec::new(),
        ConfigStore::new(directory.path()),
    );

    app.open_settings();

    let deadline = Instant::now() + Duration::from_secs(2);
    let snapshot = loop {
        let snapshot = app.runtime.snapshot();
        if snapshot
            .recent_activity
            .iter()
            .any(|activity| activity == "Video device list refreshed")
        {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "runtime did not process the settings refresh"
        );
        std::thread::yield_now();
    };
    assert!(
        snapshot
            .recent_activity
            .iter()
            .all(|activity| activity != "Monitor rescan requested")
    );
}

#[test]
fn flow_settings_save_failure_preserves_the_active_value() {
    let directory = tempfile::tempdir().unwrap();
    let blocked_path = directory.path().join("not-a-directory");
    std::fs::write(&blocked_path, "blocking file").unwrap();
    let mut app = SwitcherApp::new(
        AppConfig::default(),
        Vec::new(),
        ConfigStore::new(&blocked_path),
    );
    let initial_notification_count = app.notifications.entries().count();
    app.config.show_notifications = false;
    app.queue_settings_save();
    app.flush_settings();
    app.wait_for_settings_save();

    assert!(!app.config.show_notifications);
    assert!(
        app.settings_save_error
            .as_deref()
            .is_some_and(|message| message.contains("Could not save settings"))
    );
    assert!(
        app.load_warnings
            .iter()
            .any(|warning| warning.contains("Could not save settings"))
    );
    assert_eq!(
        app.notifications.entries().count(),
        initial_notification_count
    );
}

#[test]
fn flow_verbose_logging_setting_uses_the_normal_settings_save_path() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());

    app.config.verbose_logging = true;
    app.queue_settings_save();
    app.flush_settings();
    app.wait_for_settings_save();

    assert!(app.settings_save_error.is_none());
    assert!(store.load().config.verbose_logging);
}

#[test]
fn flow_admin_baseline_captures_pending_settings_and_toggle_is_independent() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
    assert_eq!(app.admin_profile_status, None);

    app.config.selected_video_device_id = "pending-admin-camera".into();
    app.pending_settings_save = Some(Instant::now());
    app.save_admin_baseline();
    assert_eq!(
        app.admin_profile_status,
        Some(AdminProfileStatus {
            auto_restore_on_launch: false,
            reference_included: false,
        })
    );

    app.set_admin_auto_restore(true);
    assert!(app.admin_profile_status.unwrap().auto_restore_on_launch);
    save_config(
        &store,
        &AppConfig {
            selected_video_device_id: "temporary-user-camera".into(),
            reference_image_path: store.reference_path().display().to_string(),
            ..AppConfig::default()
        },
    )
    .unwrap();
    assert_eq!(
        restore_admin_profile(&AdminProfileStore::new(directory.path())).unwrap(),
        AdminRestoreOutcome::Restored
    );
    assert_eq!(
        store.load().config.selected_video_device_id,
        "pending-admin-camera"
    );

    app.set_admin_auto_restore(false);
    app.config.selected_video_device_id = "replacement-admin-camera".into();
    app.save_admin_baseline();
    assert!(!app.admin_profile_status.unwrap().auto_restore_on_launch);
    app.remove_admin_baseline();
    assert_eq!(app.admin_profile_status, None);
    assert_eq!(
        AdminProfileStore::new(directory.path()).status().unwrap(),
        None
    );
}

#[test]
fn flow_manual_admin_load_updates_working_config_runtime_and_reference() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
        .save(store.reference_path())
        .unwrap();
    let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
    app.view = AppView::Settings;
    let admin = AppConfig {
        selected_video_device_id: "admin-camera".into(),
        selected_monitor_label: "Admin display".into(),
        similarity_threshold: 0.91,
        cursor_visible: true,
        output_mode: OutputMode::ForceScreen,
        reference_image_path: store.reference_path().display().to_string(),
        ..AppConfig::default()
    };
    app.config = admin.clone();
    app.save_admin_baseline();
    assert!(!app.admin_profile_status.unwrap().auto_restore_on_launch);

    image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]))
        .save(store.reference_path())
        .unwrap();
    let user = AppConfig {
        selected_video_device_id: "user-camera".into(),
        reference_image_path: store.reference_path().display().to_string(),
        ..AppConfig::default()
    };
    save_config(&store, &user).unwrap();
    app.config = user;
    app.pending_settings_save = Some(Instant::now());
    app.open_dialog(AppDialogKind::LoadAdminConfig);

    app.load_admin_config();

    assert_eq!(app.view, AppView::Settings);
    assert!(app.dialog_is(AppDialogKind::Admin));
    assert_eq!(app.config, admin);
    assert_eq!(store.load().config, admin);
    assert!(app.pending_settings_save.is_none());
    assert!(app.settings_save_error.is_none());
    assert!(!app.admin_profile_status.unwrap().auto_restore_on_launch);

    let deadline = Instant::now() + Duration::from_secs(2);
    let snapshot = loop {
        let snapshot = app.runtime.snapshot();
        if snapshot.selected_video_device_id == "admin-camera"
            && snapshot.mode == OutputMode::ForceScreen
            && snapshot.previews.reference.is_some()
            && snapshot
                .recent_activity
                .iter()
                .any(|activity| activity == "Monitor rescan requested")
        {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "runtime did not apply the manually loaded admin config"
        );
        std::thread::yield_now();
    };
    assert_eq!(
        &snapshot.previews.reference.unwrap().pixels()[..4],
        &[0, 0, 255, 255]
    );
}

#[test]
fn flow_startup_admin_restore_failures_preserve_the_working_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    let user = AppConfig {
        selected_video_device_id: "user-camera".into(),
        ..AppConfig::default()
    };
    save_config(&store, &user).unwrap();
    std::fs::write(
        AdminProfileStore::new(directory.path()).profile_path(),
        "invalid admin profile",
    )
    .unwrap();

    let loaded = load_config_with_admin_restore(&store);
    assert_eq!(loaded.config, user);
    assert!(
        loaded
            .warnings
            .iter()
            .any(|warning| warning.contains("Could not restore the admin configuration"))
    );
}

#[test]
fn flow_only_a_secondary_double_click_activates_the_admin_logo() {
    fn click_input(time: f64, button: egui::PointerButton) -> egui::RawInput {
        let position = Pos2::new(20.0, 20.0);
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(140.0, 140.0))),
            events: vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: position,
                    button,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..egui::RawInput::default()
        }
    }

    fn render_logo(context: &egui::Context, input: egui::RawInput) -> bool {
        let mut activated = false;
        let _ = context.run_ui(input, |ui| {
            let (_, response) = ui.allocate_exact_size(egui::vec2(96.0, 96.0), Sense::click());
            activated = admin_logo_activated(&response);
        });
        activated
    }

    let secondary = egui::Context::default();
    assert!(!render_logo(
        &secondary,
        click_input(0.0, egui::PointerButton::Secondary)
    ));
    assert!(render_logo(
        &secondary,
        click_input(0.1, egui::PointerButton::Secondary)
    ));

    let primary = egui::Context::default();
    assert!(!render_logo(
        &primary,
        click_input(0.0, egui::PointerButton::Primary)
    ));
    assert!(!render_logo(
        &primary,
        click_input(0.1, egui::PointerButton::Primary)
    ));
}

#[test]
fn flow_five_primary_diagnostics_clicks_toggle_disco_without_mixed_sequences() {
    let start = Instant::now();
    let mut gesture = DiscoDiagnosticsGesture::default();
    for click in 0..4 {
        assert!(!gesture.register_primary_click(start + Duration::from_millis(click * 200)));
    }
    assert!(gesture.register_primary_click(start + Duration::from_millis(800)));

    let mut slow_gesture = DiscoDiagnosticsGesture::default();
    for click in 0..5 {
        assert!(!slow_gesture.register_primary_click(
            start + Duration::from_secs(3) + Duration::from_millis(click * 800)
        ));
    }

    for click in 0..4 {
        assert!(!gesture.register_primary_click(
            start + Duration::from_secs(4) + Duration::from_millis(click * 100)
        ));
    }
    // A click on another control resets the hidden Diagnostics sequence.
    gesture.reset();
    assert!(!gesture.register_primary_click(start + Duration::from_secs(5)));
}

#[test]
fn flow_manual_setup_guide_navigation_and_dismissal_return_to_general_settings() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store);
    app.setup_animations_enabled = false;
    app.view = AppView::Settings;
    app.settings_tab = SettingsTab::General;

    app.start_setup_guide(SetupReturnView::Settings);
    assert_eq!(app.view, AppView::SetupGuide);
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::HowItWorks)
    );

    app.apply_setup_action(SetupAction::Previous);
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::HowItWorks)
    );
    app.apply_setup_action(SetupAction::Next);
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::Webcam)
    );
    app.apply_setup_action(SetupAction::GoTo(SetupStep::Reference));
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::Reference)
    );
    app.setup_reference_capture = SetupReferenceCaptureState::Review {
        captured_at: Instant::now(),
    };
    app.apply_setup_action(SetupAction::Next);
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::Reference)
    );
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Review { .. }
    ));
    app.apply_setup_action(SetupAction::GoTo(SetupStep::Ready));
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::Reference)
    );
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Review { .. }
    ));
    app.apply_setup_action(SetupAction::GoTo(SetupStep::Screen));
    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::Screen)
    );
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Idle
    ));
    app.apply_setup_action(SetupAction::Close);

    assert!(app.setup_session.is_none());
    assert_eq!(app.view, AppView::Settings);
    assert_eq!(app.settings_tab, SettingsTab::General);
    assert!(
        !SetupStateStore::new(directory.path())
            .initialize(false)
            .show_setup_guide
    );
}

#[test]
fn flow_setup_reference_step_requires_reference_before_continue() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        AppConfig::default(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    app.setup_animations_enabled = false;
    app.setup_session = Some(SetupSession::preview(SetupStep::Reference));
    app.setup_reference_capture = SetupReferenceCaptureState::Idle;

    app.apply_setup_action(SetupAction::Next);

    assert_eq!(
        app.setup_session.map(|session| session.step),
        Some(SetupStep::Reference)
    );
}

#[test]
fn flow_setup_guide_keyboard_navigation_matches_the_visible_actions() {
    fn action_for(key: egui::Key, step: SetupStep, next_enabled: bool) -> Option<SetupAction> {
        let context = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..egui::RawInput::default()
        };
        let mut action = None;
        let _ = context.run_ui(input, |_ui| {
            action = setup_guide_keyboard_action(&context, step, next_enabled);
        });
        action
    }

    assert_eq!(
        action_for(egui::Key::Escape, SetupStep::Webcam, true),
        Some(SetupAction::Close)
    );
    assert_eq!(
        action_for(egui::Key::ArrowLeft, SetupStep::Webcam, true),
        Some(SetupAction::Previous)
    );
    assert_eq!(
        action_for(egui::Key::ArrowLeft, SetupStep::HowItWorks, true),
        None
    );
    assert_eq!(
        action_for(egui::Key::ArrowRight, SetupStep::Ready, true),
        Some(SetupAction::Next)
    );
    assert_eq!(
        action_for(egui::Key::Enter, SetupStep::Screen, true),
        Some(SetupAction::Next)
    );
    assert_eq!(
        action_for(egui::Key::Space, SetupStep::Reference, true),
        Some(SetupAction::Next)
    );
    assert_eq!(
        action_for(egui::Key::Enter, SetupStep::Reference, false),
        None
    );
    assert_eq!(
        action_for(egui::Key::ArrowLeft, SetupStep::Reference, false),
        Some(SetupAction::Previous)
    );
}

#[test]
fn flow_finishing_setup_guide_starts_automatic_mode_and_opens_dashboard() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        AppConfig {
            output_mode: OutputMode::ForceScreen,
            start_automatically: false,
            ..AppConfig::default()
        },
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    app.start_setup_guide(SetupReturnView::Settings);
    app.setup_session.as_mut().unwrap().go_to(SetupStep::Ready);
    app.apply_setup_action(SetupAction::Next);

    assert_eq!(app.view, AppView::Dashboard);
    assert!(app.setup_session.is_none());
    assert_eq!(app.config.output_mode, OutputMode::Automatic);
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.snapshot().run_state != RunState::Running && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.snapshot().run_state, RunState::Running);
}

#[test]
fn flow_setup_guide_dismissal_is_independent_from_user_and_admin_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path());
    let config = AppConfig {
        selected_video_device_id: "setup-independent-camera".into(),
        ..AppConfig::default()
    };
    save_config(&store, &config).unwrap();
    let admin_store = AdminProfileStore::new(directory.path());
    admin_store.save(&config).unwrap();
    let admin_before = std::fs::read(admin_store.profile_path()).unwrap();

    let mut app = SwitcherApp::new(config.clone(), Vec::new(), store.clone());
    app.start_setup_guide(SetupReturnView::Dashboard);
    app.apply_setup_action(SetupAction::Close);

    assert_eq!(store.load().config, config);
    assert_eq!(
        std::fs::read(admin_store.profile_path()).unwrap(),
        admin_before
    );
}

#[cfg(not(windows))]
#[test]
fn flow_setup_reference_candidate_tracks_capture_review_save_and_timeouts() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = SwitcherApp::new(
        AppConfig::default(),
        Vec::new(),
        ConfigStore::new(directory.path()),
    );
    let mut snapshot = ui_preview_snapshot();
    let reference_sequence = snapshot.previews.reference.as_ref().unwrap().sequence;
    let candidate = snapshot.previews.screen.clone();
    snapshot.previews.reference = None;
    snapshot.previews.reference_candidate = candidate.clone();

    app.setup_reference_capture = SetupReferenceCaptureState::CapturingCandidate {
        started_at: Instant::now(),
        previous_candidate_sequence: candidate
            .as_ref()
            .map(|frame| frame.sequence.wrapping_sub(1)),
    };
    app.update_setup_reference_capture(&snapshot);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Review { .. }
    ));

    snapshot.previews.reference_candidate = None;
    app.setup_reference_capture = SetupReferenceCaptureState::CapturingCandidate {
        started_at: Instant::now() - REFERENCE_CAPTURE_TIMEOUT,
        previous_candidate_sequence: None,
    };
    app.update_setup_reference_capture(&snapshot);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::CaptureFailed
    ));

    snapshot.previews.reference = ui_preview_snapshot().previews.reference;
    app.setup_reference_capture = SetupReferenceCaptureState::SavingCandidate {
        started_at: Instant::now(),
        previous_reference_sequence: Some(reference_sequence.wrapping_sub(1)),
    };
    app.update_setup_reference_capture(&snapshot);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::Confirmed
    ));

    snapshot.previews.reference_candidate = candidate;
    app.setup_reference_capture = SetupReferenceCaptureState::SavingCandidate {
        started_at: Instant::now() - REFERENCE_CAPTURE_TIMEOUT,
        previous_reference_sequence: Some(reference_sequence),
    };
    app.update_setup_reference_capture(&snapshot);
    assert!(matches!(
        app.setup_reference_capture,
        SetupReferenceCaptureState::SaveFailed { .. }
    ));
}

#[test]
fn smoke_embedded_setup_images_have_the_approved_dimensions() {
    for bytes in [
        include_bytes!("../../assets/setup-reference-example.png").as_slice(),
        include_bytes!("../../assets/setup-webcam-example.png").as_slice(),
        include_bytes!("../../assets/setup-screen-example.png").as_slice(),
    ] {
        let image = image::load_from_memory(bytes).unwrap();
        assert_eq!((image.width(), image.height()), (1672, 941));
    }
}
