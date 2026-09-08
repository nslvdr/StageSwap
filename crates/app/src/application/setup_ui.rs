use super::*;

impl SwitcherApp {
    pub(super) fn start_setup_guide(&mut self, return_view: SetupReturnView) {
        self.flush_settings();
        self.dismiss_dialog();
        self.view = AppView::SetupGuide;
        self.setup_session = Some(SetupSession::live(return_view));
        self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        self.send_setup_reference_command(Command::DiscardReferenceCandidate);
        self.send(Command::RefreshVideoDevices);
        if self.config.automatic_monitor_rescans {
            self.send(Command::Rescan);
        }
    }

    pub(super) fn apply_setup_action(&mut self, action: SetupAction) {
        let Some(mut session) = self.setup_session else {
            return;
        };
        let reference_requires_decision = session.step == SetupStep::Reference
            && setup_reference_requires_decision(self.setup_reference_capture);
        let reference_available = self.snapshot().previews.reference.is_some()
            || matches!(
                self.setup_reference_capture,
                SetupReferenceCaptureState::Confirmed
            );
        let reference_capture_required =
            session.step == SetupStep::Reference && !reference_available;
        let moves_forward = matches!(action, SetupAction::Next)
            || matches!(
                action,
                SetupAction::GoTo(destination)
                    if destination.number() > SetupStep::Reference.number()
            );
        if (reference_requires_decision || reference_capture_required) && moves_forward {
            return;
        }
        let leaves_reference = session.step == SetupStep::Reference
            && match action {
                SetupAction::Previous | SetupAction::Next | SetupAction::Close => true,
                SetupAction::GoTo(destination) => destination != SetupStep::Reference,
            };
        if leaves_reference && reference_requires_decision {
            if !self.send_setup_reference_command(Command::DiscardReferenceCandidate) {
                return;
            }
            self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        }
        match action {
            SetupAction::Previous => {
                if let Some(step) = session.step.previous() {
                    session.transition_to(step, self.setup_animations_enabled);
                    self.setup_session = Some(session);
                }
            }
            SetupAction::Next => {
                if let Some(step) = session.step.next() {
                    session.transition_to(step, self.setup_animations_enabled);
                    self.setup_session = Some(session);
                } else {
                    self.finish_setup_guide();
                }
            }
            SetupAction::GoTo(step) => {
                session.transition_to(step, self.setup_animations_enabled);
                self.setup_session = Some(session);
            }
            SetupAction::Close => self.dismiss_setup_guide(session.return_view),
        }
    }

    pub(super) fn mark_setup_guide_completed(&mut self) {
        if let Err(error) = self.setup_store.mark_completed() {
            let warning = format!(
                "Could not remember setup guide completion; you can still reopen it in Settings: {error}"
            );
            self.log
                .write("warning", "setup", "SETUP_COMPLETION_SAVE_FAILED", &warning);
            self.push_notification_warning(NotificationSource::Startup, warning);
        }
    }

    pub(super) fn finish_setup_guide(&mut self) {
        self.mark_setup_guide_completed();
        self.set_mode(OutputMode::Automatic);
        self.flush_settings();
        self.send(Command::Start);
        self.send_setup_reference_command(Command::DiscardReferenceCandidate);
        self.setup_session = None;
        self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        self.view = AppView::Dashboard;
    }

    pub(super) fn dismiss_setup_guide(&mut self, return_view: SetupReturnView) {
        self.mark_setup_guide_completed();
        self.send_setup_reference_command(Command::DiscardReferenceCandidate);
        self.setup_session = None;
        self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        match return_view {
            SetupReturnView::Dashboard => {
                self.view = AppView::Dashboard;
            }
            SetupReturnView::Settings => {
                self.view = AppView::Settings;
                self.settings_tab = SettingsTab::General;
                self.settings_opened_at = Some(Instant::now());
                self.settings_section_changed_at = Some(Instant::now());
            }
        }
    }

    pub(super) fn setup_guide_view(&mut self, context: &egui::Context, ui: &mut egui::Ui) {
        if self.setup_session.is_none() {
            self.view = AppView::Dashboard;
            return;
        }
        let now = Instant::now();
        let (step, entrance, page_opacity, transition_active) = {
            let session = self.setup_session.as_mut().expect("checked above");
            let entrance = if self.setup_animations_enabled {
                animation_progress(session.opened_at, SETUP_GUIDE_ENTRANCE_DURATION)
            } else {
                1.0
            };
            let page_opacity = if self.setup_animations_enabled {
                session.transition_opacity(now, SETUP_GUIDE_STEP_DURATION)
            } else {
                1.0
            };
            (
                session.step,
                entrance,
                page_opacity,
                session.pending_step.is_some(),
            )
        };
        if entrance < 1.0 || transition_active {
            context.request_repaint();
        }

        let snapshot = self.snapshot();
        self.update_setup_reference_capture(&snapshot);
        let textures = self.setup_example_textures(context);
        let reference_available = snapshot.previews.reference.is_some()
            || matches!(
                self.setup_reference_capture,
                SetupReferenceCaptureState::Confirmed
            );
        let next_enabled = step != SetupStep::Reference
            || (reference_available
                && !setup_reference_requires_decision(self.setup_reference_capture));
        let mut action = if transition_active {
            None
        } else {
            setup_guide_keyboard_action(context, step, next_enabled)
        };

        let full_rect = ui.max_rect();
        let background = ui.visuals().panel_fill;
        ui.painter().rect_filled(full_rect, 0, background);
        let content_width = (full_rect.width() - 64.0).clamp(360.0, SETUP_GUIDE_CONTENT_WIDTH);
        let footer_rect = Rect::from_min_max(
            Pos2::new(
                full_rect.left(),
                full_rect.bottom() - SETUP_GUIDE_FOOTER_HEIGHT,
            ),
            full_rect.max,
        );
        let body_rect = Rect::from_min_max(
            full_rect.min,
            Pos2::new(full_rect.right(), footer_rect.top()),
        );

        egui::Area::new(egui::Id::new(("setup-guide-content", step.number())))
            .fixed_pos(body_rect.center())
            .pivot(Align2::CENTER_CENTER)
            .movable(false)
            .constrain_to(body_rect.shrink(12.0))
            .show(context, |body_ui| {
                body_ui.set_width(content_width);
                body_ui.set_max_height((body_rect.height() - 24.0).max(200.0));
                body_ui.set_opacity(entrance * page_opacity);
                body_ui.with_layout(egui::Layout::top_down(egui::Align::Center), |body_ui| {
                    setup_step_title(body_ui, step);
                    body_ui.add_space(5.0);
                    body_ui.allocate_ui_with_layout(
                        egui::vec2(content_width.min(700.0), 1.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(tr(ui, step.explanation()))
                                        .size(14.0)
                                        .line_height(Some(20.0))
                                        .color(mix_color(background, SETUP_SIGNAL_WHITE, 0.66)),
                                )
                                .wrap()
                                .halign(egui::Align::Center),
                            );
                        },
                    );
                    body_ui.add_space(if body_rect.height() < 520.0 {
                        8.0
                    } else {
                        14.0
                    });
                    if step == SetupStep::HowItWorks {
                        self.setup_how_it_works(
                            body_ui,
                            &textures,
                            session_elapsed(self.setup_session, now),
                            self.setup_animations_enabled,
                            background,
                        );
                    } else {
                        let max_scroll_height = body_ui.available_height().max(160.0);
                        egui::ScrollArea::vertical()
                            .id_salt(("setup-guide-body", step.number()))
                            .auto_shrink([false, true])
                            .max_height(max_scroll_height)
                            .show(body_ui, |ui| {
                                ui.set_width(content_width);
                                if let Some(content_action) =
                                    self.setup_step_content(ui, step, &snapshot, &textures)
                                {
                                    action = Some(content_action);
                                }
                            });
                    }
                });
            });

        ui.painter().line_segment(
            [
                Pos2::new(footer_rect.left(), footer_rect.top()),
                Pos2::new(footer_rect.right(), footer_rect.top()),
            ],
            Stroke::new(1.0, mix_color(background, SETUP_SIGNAL_WHITE, 0.09)),
        );
        let footer_content_rect = Rect::from_center_size(
            footer_rect.center(),
            egui::vec2(content_width, footer_rect.height()),
        );
        let next_width = if step.is_last() { 180.0 } else { 146.0 };
        let back_width = 40.0;
        let actions_width = back_width + 10.0 + next_width;
        let left_rect = Rect::from_center_size(
            Pos2::new(
                footer_content_rect.left() + actions_width / 2.0,
                footer_content_rect.center().y,
            ),
            egui::vec2(actions_width, 40.0),
        );
        let actions_rect = Rect::from_center_size(
            Pos2::new(
                footer_content_rect.right() - actions_width / 2.0,
                footer_content_rect.center().y,
            ),
            egui::vec2(actions_width, 40.0),
        );
        let mut later_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("setup-guide-later")
                .max_rect(left_rect)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::TopDown,
                )),
        );
        later_ui.set_opacity(entrance);
        if setup_footer_button(
            &mut later_ui,
            UiIcon::Clock,
            "Set up later",
            egui::vec2(actions_width, 40.0),
            SetupFooterButtonStyle::Secondary,
            true,
        )
        .clicked()
        {
            action = Some(SetupAction::Close);
        }

        let progress_rect =
            Rect::from_center_size(footer_content_rect.center(), egui::vec2(292.0, 52.0));
        let mut progress_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("setup-guide-progress")
                .max_rect(progress_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        progress_ui.set_opacity(entrance);
        if let Some(destination) = setup_footer_progress(&mut progress_ui, step, next_enabled) {
            action = Some(SetupAction::GoTo(destination));
        }

        let mut actions_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("setup-guide-actions")
                .max_rect(actions_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        actions_ui.set_opacity(entrance);
        if setup_footer_button(
            &mut actions_ui,
            UiIcon::Back,
            "Back",
            egui::vec2(back_width, 40.0),
            SetupFooterButtonStyle::IconOnly,
            step.previous().is_some(),
        )
        .clicked()
        {
            action = Some(SetupAction::Previous);
        }
        actions_ui.add_space(10.0);
        let label = if step.is_last() {
            "Start StageSwap"
        } else {
            "Continue"
        };
        let icon = if step.is_last() {
            UiIcon::Play
        } else {
            UiIcon::ArrowRight
        };
        if setup_footer_button(
            &mut actions_ui,
            icon,
            label,
            egui::vec2(next_width, 40.0),
            SetupFooterButtonStyle::Primary {
                icon_after: !step.is_last(),
            },
            next_enabled,
        )
        .clicked()
        {
            action = Some(SetupAction::Next);
        }

        if let Some(action) = action {
            self.apply_setup_action(action);
        }
    }

    pub(super) fn setup_step_content(
        &mut self,
        ui: &mut egui::Ui,
        step: SetupStep,
        snapshot: &AppSnapshot,
        textures: &SetupExampleTextures,
    ) -> Option<SetupAction> {
        match step {
            SetupStep::HowItWorks => {
                self.setup_how_it_works(
                    ui,
                    textures,
                    None,
                    self.setup_animations_enabled,
                    ui.visuals().panel_fill,
                );
                None
            }
            SetupStep::Webcam => {
                self.setup_webcam_step(ui, snapshot);
                None
            }
            SetupStep::Screen => {
                self.setup_screen_step(ui, snapshot);
                None
            }
            SetupStep::Reference => self.setup_reference_step(ui, snapshot, textures),
            SetupStep::Ready => {
                self.setup_ready_step(ui, snapshot);
                None
            }
        }
    }

    pub(super) fn setup_how_it_works(
        &mut self,
        ui: &mut egui::Ui,
        textures: &SetupExampleTextures,
        elapsed: Option<Duration>,
        animations_enabled: bool,
        background: Color32,
    ) {
        if !animations_enabled {
            setup_static_switching_demo(ui, textures, background);
            return;
        }

        let preview_alpha = self.setup_demo_preview_alpha();
        let changed_alpha =
            preview_alpha.unwrap_or_else(|| elapsed.map_or(0.0, setup_demo_changed_alpha));
        if elapsed.is_some() && preview_alpha.is_none() {
            ui.ctx().request_repaint();
        }
        setup_animated_switching_demo(ui, textures, changed_alpha, background);
    }

    pub(super) fn setup_webcam_step(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let selected_available = snapshot
            .video_devices
            .iter()
            .any(|device| device.id == self.config.selected_video_device_id);
        let empty_message =
            if !self.config.selected_video_device_id.is_empty() && !selected_available {
                "This webcam is unavailable. Choose another one or refresh the list."
            } else {
                "No webcam found. Connect a webcam, then refresh the list."
            };
        let selected_name = snapshot
            .video_devices
            .iter()
            .find(|device| device.id == self.config.selected_video_device_id)
            .map(|device| device.name.clone())
            .unwrap_or_else(|| tr(ui, "No camera selected").into_owned());
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(SETUP_HARDWARE_PREVIEW_WIDTH);
                self.settings_preview_panel(
                    ui,
                    SettingsPreview {
                        kind: PreviewKind::Webcam,
                        frame: snapshot.previews.webcam.as_ref(),
                        label: "Webcam preview",
                        empty_message,
                        actual_output: snapshot.actual_output,
                    },
                    SETUP_HARDWARE_PREVIEW_WIDTH,
                );
                ui.add_space(13.0);
                setup_control_label(ui, UiIcon::Camera, "Webcam", SETTINGS_BLUE);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let geometry = selector_utility_geometry(
                        SETUP_HARDWARE_PREVIEW_WIDTH,
                        ui.spacing().item_spacing.x,
                    );
                    let previous = self.config.selected_video_device_id.clone();
                    egui::ComboBox::from_id_salt("setup-webcam-selector")
                        .width(geometry.selector_width)
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for device in snapshot.video_devices.iter() {
                                ui.selectable_value(
                                    &mut self.config.selected_video_device_id,
                                    device.id.clone(),
                                    &device.name,
                                );
                            }
                        });
                    if self.config.selected_video_device_id != previous {
                        self.awaiting_video_device_id =
                            Some(self.config.selected_video_device_id.clone());
                        self.queue_settings_save();
                    }
                    if icon_only_button(
                        ui,
                        UiIcon::Refresh,
                        "Refresh webcams",
                        egui::vec2(geometry.action_width, 32.0),
                    )
                    .on_hover_text(tr(ui, "Refresh webcams"))
                    .clicked()
                    {
                        self.send(Command::RefreshVideoDevices);
                    }
                });
                if snapshot.previews.webcam.is_some() && !selected_available {
                    ui.add_space(10.0);
                    setup_message(ui, empty_message, LIVE_RED);
                }
            },
        );
    }

    pub(super) fn setup_screen_step(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let saved_available = snapshot
            .monitors
            .iter()
            .any(|monitor| monitor.label == self.config.selected_monitor_label);
        let empty_message = if !self.config.selected_monitor_label.is_empty() && !saved_available {
            "This secondary screen is unavailable. Choose another one or rescan."
        } else {
            "No screen found. Connect the secondary screen used by JW Library, then rescan."
        };
        let selected_name = snapshot
            .selected_monitor
            .as_ref()
            .map(|monitor| monitor.label.clone())
            .unwrap_or_else(|| tr(ui, "No display selected").into_owned());
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(SETUP_HARDWARE_PREVIEW_WIDTH);
                self.settings_preview_panel(
                    ui,
                    SettingsPreview {
                        kind: PreviewKind::Screen,
                        frame: snapshot.previews.screen.as_ref(),
                        label: "Secondary screen preview",
                        empty_message,
                        actual_output: snapshot.actual_output,
                    },
                    SETUP_HARDWARE_PREVIEW_WIDTH,
                );
                ui.add_space(13.0);
                setup_control_label(ui, UiIcon::Monitor, "Secondary screen", SETTINGS_BLUE);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let geometry = selector_utility_geometry(
                        SETUP_HARDWARE_PREVIEW_WIDTH,
                        ui.spacing().item_spacing.x,
                    );
                    egui::ComboBox::from_id_salt("setup-screen-selector")
                        .width(geometry.selector_width)
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for monitor in snapshot.monitors.iter() {
                                let label = format!(
                                    "{} — {}×{}",
                                    monitor.label, monitor.width, monitor.height
                                );
                                if ui
                                    .selectable_label(
                                        snapshot.selected_monitor.as_ref() == Some(monitor),
                                        label,
                                    )
                                    .clicked()
                                {
                                    self.awaiting_monitor_label = Some(monitor.label.clone());
                                    self.send(Command::SelectMonitor(monitor.clone()));
                                }
                            }
                        });
                    if icon_only_button(
                        ui,
                        UiIcon::Refresh,
                        "Rescan screens",
                        egui::vec2(geometry.action_width, 32.0),
                    )
                    .on_hover_text(tr(ui, "Rescan screens"))
                    .clicked()
                    {
                        self.send(Command::Rescan);
                    }
                });
                if snapshot.previews.screen.is_some()
                    && (!saved_available || snapshot.selected_monitor.is_none())
                {
                    ui.add_space(10.0);
                    setup_message(ui, empty_message, LIVE_RED);
                }
            },
        );
    }

    pub(super) fn setup_reference_step(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        textures: &SetupExampleTextures,
    ) -> Option<SetupAction> {
        let mut action = None;
        let reference_available = snapshot.previews.reference.is_some();
        let screen_selected = snapshot.selected_monitor.is_some();
        let screen_ready = screen_selected && snapshot.previews.screen.is_some();
        let screen_link_label = if screen_selected {
            "Change display"
        } else {
            "Choose a display"
        };

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(SETUP_HARDWARE_PREVIEW_WIDTH);
                setup_reference_example_thumbnail(
                    ui,
                    textures.reference.id(),
                    egui::vec2(
                        SETUP_HARDWARE_PREVIEW_WIDTH,
                        SETUP_HARDWARE_PREVIEW_WIDTH / WINDOW_ASPECT_RATIO,
                    ),
                );
                ui.add_space(7.0);
                icon_text(
                    ui,
                    UiIcon::Image,
                    "Example reference image",
                    ui_theme::TEXT_SECONDARY,
                    false,
                );
                ui.add_space(13.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 18.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        setup_control_label(ui, UiIcon::Capture, "Reference image", SETTINGS_BLUE);
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if setup_link(ui, screen_link_label) {
                                    action = Some(SetupAction::GoTo(SetupStep::Screen));
                                }
                            },
                        );
                    },
                );
                ui.add_space(6.0);
                let capture_label = setup_reference_capture_label(reference_available);
                let capture_enabled = screen_ready
                    && !setup_reference_requires_decision(self.setup_reference_capture);
                let response = ui
                    .add_enabled_ui(capture_enabled, |ui| {
                        accent_icon_button(
                            ui,
                            UiIcon::Capture,
                            capture_label,
                            egui::vec2(ui.available_width(), 32.0),
                            SETTINGS_BLUE,
                        )
                    })
                    .inner;
                if response.clicked() {
                    self.begin_reference_capture();
                }
                if matches!(
                    self.setup_reference_capture,
                    SetupReferenceCaptureState::CaptureFailed
                ) {
                    ui.add_space(10.0);
                    setup_message(
                        ui,
                        "StageSwap couldn’t capture the screen. Check the screen preview and try again.",
                        LIVE_RED,
                    );
                }
            },
        );

        action
    }

    pub(super) fn setup_reference_example_rail(
        &mut self,
        ui: &mut egui::Ui,
        textures: &SetupExampleTextures,
        width: f32,
    ) {
        egui::Frame::new()
            .fill(SETUP_SIGNAL_DECK)
            .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
            .corner_radius(10)
            .inner_margin(14)
            .show(ui, |ui| {
                let content_width = (width - 28.0).max(1.0);
                ui.set_width(content_width);
                ui.set_height(SETUP_REFERENCE_CARD_HEIGHT - 28.0);
                setup_control_label(ui, UiIcon::Image, "Example reference image", SETTINGS_BLUE);
                ui.add_space(10.0);
                let thumbnail_size = egui::vec2(content_width, content_width / WINDOW_ASPECT_RATIO);
                setup_reference_example_thumbnail(ui, textures.reference.id(), thumbnail_size);
                ui.add_space(10.0);
                setup_capture_help(
                    ui,
                    "The screen JW Library shows when no media is playing should look like this example.",
                );
            });
    }

    pub(super) fn setup_ready_step(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let webcam_ready = snapshot.webcam_state == DeviceState::Ready
            && snapshot.previews.webcam.is_some()
            && !snapshot.selected_video_device_id.is_empty();
        let screen_ready = snapshot.screen_state == DeviceState::Ready
            && snapshot.previews.screen.is_some()
            && snapshot.selected_monitor.is_some();
        let reference_ready = snapshot.previews.reference.is_some();
        let rows = [
            (
                UiIcon::Camera,
                webcam_ready,
                "Webcam ready",
                "Webcam not selected",
            ),
            (
                UiIcon::Monitor,
                screen_ready,
                "Secondary screen ready",
                "Secondary screen not selected",
            ),
            (
                UiIcon::Image,
                reference_ready,
                "Reference image ready",
                "Reference image not captured",
            ),
        ];
        let width = ui.available_width().min(620.0);
        let row_count = rows.len();
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                egui::Frame::new()
                    .fill(SETUP_SIGNAL_DECK)
                    .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        ui.set_width(width - 32.0);
                        for (index, (icon, ready, ready_text, missing_text)) in
                            rows.into_iter().enumerate()
                        {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 48.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let (icon_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(20.0, 20.0),
                                        Sense::hover(),
                                    );
                                    ui_icon::paint(
                                        ui.painter(),
                                        icon_rect,
                                        if ready { UiIcon::Check } else { icon },
                                        if ready {
                                            ACTIVE_GREEN
                                        } else {
                                            TRANSITION_AMBER
                                        },
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(tr(
                                            ui,
                                            if ready { ready_text } else { missing_text },
                                        ))
                                        .size(13.0)
                                        .color(SETUP_SIGNAL_WHITE),
                                    );
                                },
                            );
                            if index + 1 < row_count {
                                ui.separator();
                            }
                        }
                    });
            },
        );
        ui.add_space(18.0);
        setup_warning_callout(
            ui,
            width,
            "IMPORTANT",
            "In Zoom, select Stageswap Camera as your camera before the meeting.",
        );
        if !(webcam_ready && screen_ready && reference_ready) {
            ui.add_space(18.0);
            setup_message(
                ui,
                "Some setup is missing. StageSwap will start, but Auto mode may not work as expected. You can finish the guided setup later in Settings.",
                TRANSITION_AMBER,
            );
        }
    }

    pub(super) fn update_setup_reference_capture(&mut self, snapshot: &AppSnapshot) {
        let now = Instant::now();
        match self.setup_reference_capture {
            SetupReferenceCaptureState::PreparingCandidate { started_at } => {
                if snapshot.previews.reference_candidate.is_none() {
                    self.capture_reference_candidate();
                } else if now.saturating_duration_since(started_at) >= REFERENCE_CAPTURE_TIMEOUT {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            SetupReferenceCaptureState::CapturingCandidate {
                started_at,
                previous_candidate_sequence,
            } => {
                let current_sequence = snapshot
                    .previews
                    .reference_candidate
                    .as_ref()
                    .map(|frame| frame.sequence);
                if current_sequence.is_some() && current_sequence != previous_candidate_sequence {
                    self.setup_reference_capture =
                        SetupReferenceCaptureState::Review { captured_at: now };
                } else if now.saturating_duration_since(started_at) >= REFERENCE_CAPTURE_TIMEOUT {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            SetupReferenceCaptureState::Review { .. } => {
                if snapshot.previews.reference_candidate.is_none() {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            SetupReferenceCaptureState::SavingCandidate {
                started_at,
                previous_reference_sequence,
            } => {
                let current_sequence = snapshot
                    .previews
                    .reference
                    .as_ref()
                    .map(|frame| frame.sequence);
                if current_sequence.is_some() && current_sequence != previous_reference_sequence {
                    self.setup_reference_capture = SetupReferenceCaptureState::Confirmed;
                } else if now.saturating_duration_since(started_at) >= REFERENCE_CAPTURE_TIMEOUT {
                    self.setup_reference_capture = SetupReferenceCaptureState::SaveFailed {
                        previous_reference_sequence,
                    };
                }
            }
            SetupReferenceCaptureState::SaveFailed {
                previous_reference_sequence,
            } => {
                let current_sequence = snapshot
                    .previews
                    .reference
                    .as_ref()
                    .map(|frame| frame.sequence);
                if current_sequence.is_some() && current_sequence != previous_reference_sequence {
                    self.setup_reference_capture = SetupReferenceCaptureState::Confirmed;
                } else if snapshot.previews.reference_candidate.is_none() {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            _ => {}
        }
    }

    pub(super) fn setup_example_textures(
        &mut self,
        context: &egui::Context,
    ) -> SetupExampleTextures {
        self.setup_example_textures
            .get_or_insert_with(|| SetupExampleTextures {
                reference: load_embedded_texture(
                    context,
                    "setup-reference-example",
                    include_bytes!("../../assets/setup-reference-example.png"),
                ),
                webcam: load_embedded_texture(
                    context,
                    "setup-webcam-example",
                    include_bytes!("../../assets/setup-webcam-example.png"),
                ),
                screen: load_embedded_texture(
                    context,
                    "setup-screen-example",
                    include_bytes!("../../assets/setup-screen-example.png"),
                ),
            })
            .clone()
    }
}

pub(super) fn session_elapsed(session: Option<SetupSession>, now: Instant) -> Option<Duration> {
    let session = session?;
    (session.step == SetupStep::HowItWorks)
        .then(|| {
            session
                .step_changed_at
                .map(|start| now.saturating_duration_since(start))
        })
        .flatten()
}

pub(super) fn setup_demo_changed_alpha(elapsed: Duration) -> f32 {
    let phase = elapsed.as_secs_f32() % SETUP_DEMO_LOOP_DURATION;
    if phase < SETUP_DEMO_HOLD_DURATION {
        0.0
    } else if phase < SETUP_DEMO_HOLD_DURATION + SETUP_DEMO_FADE_DURATION {
        (phase - SETUP_DEMO_HOLD_DURATION) / SETUP_DEMO_FADE_DURATION
    } else if phase < SETUP_DEMO_HOLD_DURATION * 2.0 + SETUP_DEMO_FADE_DURATION {
        1.0
    } else {
        1.0 - (phase - (SETUP_DEMO_HOLD_DURATION * 2.0 + SETUP_DEMO_FADE_DURATION))
            / SETUP_DEMO_FADE_DURATION
    }
    .clamp(0.0, 1.0)
}

pub(super) fn setup_reference_flash_alpha(elapsed: Duration, animations_enabled: bool) -> f32 {
    if animations_enabled && elapsed < SETUP_REFERENCE_FLASH_DURATION {
        1.0 - elapsed.as_secs_f32() / SETUP_REFERENCE_FLASH_DURATION.as_secs_f32()
    } else {
        0.0
    }
    .clamp(0.0, 1.0)
}

pub(super) fn setup_step_title(ui: &mut egui::Ui, step: SetupStep) -> Rect {
    let title = tr(ui, step.title()).into_owned();
    let title_width = ui
        .painter()
        .layout_no_wrap(
            title.clone(),
            FontId::proportional(30.0),
            SETUP_SIGNAL_WHITE,
        )
        .size()
        .x;
    let title_group_width = 28.0 + 8.0 + title_width;

    ui.allocate_ui_with_layout(
        egui::vec2(title_group_width, 36.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
            ui_icon::paint(
                ui.painter(),
                icon_rect.shrink(1.0),
                setup_step_icon(step),
                SETTINGS_BLUE,
            );
            ui.label(
                RichText::new(title)
                    .size(30.0)
                    .strong()
                    .color(SETUP_SIGNAL_WHITE),
            );
        },
    )
    .response
    .rect
}

pub(super) fn setup_guide_keyboard_action(
    context: &egui::Context,
    step: SetupStep,
    next_enabled: bool,
) -> Option<SetupAction> {
    context.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            return Some(SetupAction::Close);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
            return step.previous().map(|_| SetupAction::Previous);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
        {
            return next_enabled.then_some(SetupAction::Next);
        }
        None
    })
}

pub(super) fn load_embedded_texture(
    context: &egui::Context,
    name: &'static str,
    bytes: &[u8],
) -> TextureHandle {
    let image = image::load_from_memory(bytes)
        .unwrap_or_else(|error| panic!("{name} should be a valid embedded image: {error}"))
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    context.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        TextureOptions::LINEAR,
    )
}

pub(super) fn setup_footer_progress(
    ui: &mut egui::Ui,
    step: SetupStep,
    forward_enabled: bool,
) -> Option<SetupStep> {
    let background = ui.visuals().panel_fill;
    let step_text = format_text(
        ui_locale(ui),
        "Step {0} of {1}",
        &[
            &step.number().to_string(),
            &SetupStep::ALL.len().to_string(),
        ],
    );
    ui.label(
        RichText::new(step_text)
            .size(12.0)
            .strong()
            .color(mix_color(background, SETUP_SIGNAL_WHITE, 0.78)),
    );
    ui.add_space(3.0);

    let width = 268.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), Sense::hover());
    let first_x = rect.left() + 12.0;
    let last_x = rect.right() - 12.0;
    let center_y = rect.center().y;
    let step_gap = (last_x - first_x) / (SetupStep::ALL.len() - 1) as f32;
    let rail = Rect::from_min_max(
        Pos2::new(first_x, center_y - 1.0),
        Pos2::new(last_x, center_y + 1.0),
    );
    ui.painter()
        .rect_filled(rail, 1, mix_color(background, SETUP_SIGNAL_WHITE, 0.13));
    if step.number() > 1 {
        let active_x = first_x + step_gap * (step.number() - 1) as f32;
        ui.painter().rect_filled(
            Rect::from_min_max(rail.min, Pos2::new(active_x, rail.bottom())),
            1,
            SETTINGS_BLUE,
        );
    }

    let mut destination = None;
    for (index, candidate) in SetupStep::ALL.into_iter().enumerate() {
        let number = index + 1;
        let enabled = forward_enabled || number <= step.number();
        let center = Pos2::new(first_x + step_gap * index as f32, center_y);
        let hit_rect = Rect::from_center_size(center, egui::vec2(34.0, 24.0));
        let response = ui.interact(
            hit_rect,
            egui::Id::new(("setup-progress-step", number)),
            Sense::click(),
        );
        let is_current = candidate == step;
        let is_complete = number < step.number();
        let hovered = enabled && response.hovered() && !is_current;
        let node_fill = if is_current {
            SETTINGS_BLUE
        } else if is_complete {
            mix_color(SETTINGS_BLUE, SETUP_SIGNAL_WHITE, 0.08)
        } else if !enabled {
            mix_color(background, SETUP_SIGNAL_WHITE, 0.07)
        } else if hovered {
            mix_color(background, SETUP_SIGNAL_WHITE, 0.2)
        } else {
            mix_color(background, SETUP_SIGNAL_WHITE, 0.12)
        };
        let radius = if is_current { 9.0 } else { 6.0 };

        if is_current {
            ui.painter()
                .circle_filled(center, 12.0, SETTINGS_BLUE.gamma_multiply(0.18));
        }
        ui.painter().circle_filled(center, radius, node_fill);
        if !is_complete {
            ui.painter().text(
                center,
                Align2::CENTER_CENTER,
                number.to_string(),
                FontId::proportional(if is_current { 10.5 } else { 9.0 }),
                if is_current {
                    SETUP_SIGNAL_WHITE
                } else {
                    mix_color(background, SETUP_SIGNAL_WHITE, 0.62)
                },
            );
        } else {
            ui_icon::paint(
                ui.painter(),
                Rect::from_center_size(center, egui::vec2(8.0, 8.0)),
                UiIcon::Check,
                SETUP_SIGNAL_WHITE,
            );
        }

        let label = tr(ui, candidate.title());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                enabled,
                is_current,
                label.as_ref(),
            )
        });
        let response = response.on_hover_text(label);
        let response = if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        };
        if enabled && response.clicked() && !is_current {
            destination = Some(candidate);
        }
    }
    destination
}

pub(super) fn setup_rounded_image(
    ui: &egui::Ui,
    rect: Rect,
    texture: egui::TextureId,
    opacity: f32,
    background: Color32,
    corner_radius: u8,
) {
    egui::Image::new((texture, rect.size()))
        .corner_radius(corner_radius)
        .tint(Color32::WHITE.gamma_multiply(opacity))
        .paint_at(ui, rect);
    ui.painter().rect_stroke(
        rect,
        corner_radius,
        Stroke::new(1.0, mix_color(background, SETUP_SIGNAL_WHITE, 0.15)),
        StrokeKind::Inside,
    );
}

pub(super) fn setup_crossfade_image(
    ui: &egui::Ui,
    rect: Rect,
    first: egui::TextureId,
    second: egui::TextureId,
    blend: f32,
    background: Color32,
) {
    egui::Image::new((first, rect.size()))
        .corner_radius(9)
        .tint(Color32::WHITE.gamma_multiply(1.0 - blend))
        .paint_at(ui, rect);
    if blend > 0.0 {
        egui::Image::new((second, rect.size()))
            .corner_radius(9)
            .tint(Color32::WHITE.gamma_multiply(blend))
            .paint_at(ui, rect);
    }
    ui.painter().rect_stroke(
        rect,
        9,
        Stroke::new(1.0, mix_color(background, SETUP_SIGNAL_WHITE, 0.15)),
        StrokeKind::Inside,
    );
}

pub(super) fn setup_animated_switching_demo(
    ui: &mut egui::Ui,
    textures: &SetupExampleTextures,
    changed_alpha: f32,
    background: Color32,
) {
    let compact = ui.available_height() < 420.0;
    let input_width = if compact { 120.0 } else { 150.0 };
    let output_width = if compact { 300.0 } else { 360.0 };
    let input_height = input_width / WINDOW_ASPECT_RATIO;
    let output_height = output_width / WINDOW_ASPECT_RATIO;
    let caption_height = 16.0;
    let caption_gap = 4.0;
    let block_height = caption_height + caption_gap + input_height;
    let card_gap = if compact { 6.0 } else { 8.0 };
    let card_padding_x = 14.0;
    let card_padding_y = 8.0;
    let card_width = input_width + card_padding_x * 2.0;
    let card_height = block_height + card_padding_y * 2.0;
    let diagram_height = card_height * 2.0 + card_gap;
    let output_gap = if compact { 46.0 } else { 64.0 };
    let diagram_width = card_width + output_gap + output_width;
    let (diagram, _) =
        ui.allocate_exact_size(egui::vec2(diagram_width, diagram_height), Sense::hover());

    let content_left = diagram.left() + card_padding_x;
    let screen_caption = Rect::from_min_size(
        Pos2::new(content_left, diagram.top() + card_padding_y),
        egui::vec2(input_width, caption_height),
    );
    let screen = Rect::from_min_size(
        Pos2::new(content_left, screen_caption.bottom() + caption_gap),
        egui::vec2(input_width, input_height),
    );
    let screen_card = Rect::from_min_max(
        diagram.min,
        Pos2::new(diagram.left() + card_width, diagram.top() + card_height),
    );
    let webcam_caption = Rect::from_min_size(
        Pos2::new(
            content_left,
            screen_card.bottom() + card_gap + card_padding_y,
        ),
        egui::vec2(input_width, caption_height),
    );
    let webcam = Rect::from_min_size(
        Pos2::new(content_left, webcam_caption.bottom() + caption_gap),
        egui::vec2(input_width, input_height),
    );
    let webcam_card = Rect::from_min_max(
        Pos2::new(diagram.left(), screen_card.bottom() + card_gap),
        Pos2::new(
            diagram.left() + card_width,
            screen_card.bottom() + card_gap + card_height,
        ),
    );
    let output_block_height = caption_height + caption_gap + output_height;
    let output_caption = Rect::from_min_size(
        Pos2::new(
            diagram.right() - output_width,
            diagram.center().y - output_block_height / 2.0,
        ),
        egui::vec2(output_width, caption_height),
    );
    let output = Rect::from_min_size(
        Pos2::new(output_caption.left(), output_caption.bottom() + caption_gap),
        egui::vec2(output_width, output_height),
    );

    paint_setup_group_card(ui, screen_card, changed_alpha, background);
    paint_setup_group_card(ui, webcam_card, 1.0 - changed_alpha, background);

    paint_setup_centered_label(
        ui,
        screen_caption,
        "JW Library",
        mix_color(background, SETUP_SIGNAL_WHITE, 0.62),
    );
    paint_setup_centered_label(
        ui,
        webcam_caption,
        "Webcam",
        mix_color(background, SETUP_SIGNAL_WHITE, 0.62).gamma_multiply(1.0 - changed_alpha * 0.42),
    );
    paint_setup_centered_label(
        ui,
        output_caption,
        "Zoom",
        mix_color(background, SETUP_SIGNAL_WHITE, 0.76),
    );

    setup_crossfade_image(
        ui,
        screen,
        textures.reference.id(),
        textures.screen.id(),
        changed_alpha,
        background,
    );
    setup_rounded_image(
        ui,
        webcam,
        textures.webcam.id(),
        1.0 - changed_alpha * 0.42,
        background,
        8,
    );
    setup_crossfade_image(
        ui,
        output,
        textures.webcam.id(),
        textures.screen.id(),
        changed_alpha,
        background,
    );

    ui.painter().rect_stroke(
        output,
        9,
        Stroke::new(1.5, SETTINGS_BLUE.gamma_multiply(0.72)),
        StrokeKind::Inside,
    );

    ui.add_space(if compact { 4.0 } else { 8.0 });
    setup_demo_rule(ui, diagram_width.min(740.0), changed_alpha, background);
    ui.add_space(if compact { 2.0 } else { 6.0 });
    setup_plain_zoom_reminder(ui, diagram_width.min(620.0), background);
}

pub(super) fn paint_setup_group_card(
    ui: &egui::Ui,
    rect: Rect,
    active_strength: f32,
    background: Color32,
) {
    ui.painter()
        .rect_filled(rect, 10, mix_color(background, SETUP_SIGNAL_WHITE, 0.025));
    let neutral = mix_color(background, SETUP_SIGNAL_WHITE, 0.12);
    ui.painter().rect_stroke(
        rect,
        10,
        Stroke::new(
            1.0 + active_strength * 0.5,
            mix_color(neutral, SETTINGS_BLUE, active_strength * 0.78),
        ),
        StrokeKind::Inside,
    );
}

pub(super) fn paint_setup_centered_label(ui: &egui::Ui, rect: Rect, text: &str, color: Color32) {
    let text = tr(ui, text);
    let galley = ui
        .painter()
        .layout_no_wrap(text.into_owned(), FontId::proportional(11.5), color);
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

pub(super) fn paint_setup_active_outline(ui: &egui::Ui, rect: Rect, strength: f32) {
    if strength > 0.0 {
        ui.painter().rect_stroke(
            rect,
            8,
            Stroke::new(1.5, SETTINGS_BLUE.gamma_multiply(strength * 0.72)),
            StrokeKind::Inside,
        );
    }
}

pub(super) fn paint_setup_crossfade_text(
    ui: &egui::Ui,
    rect: Rect,
    first: &str,
    second: &str,
    blend: f32,
    font: FontId,
    color: Color32,
) {
    let first = tr(ui, first);
    let second = tr(ui, second);
    let first_color = color.gamma_multiply(1.0 - blend);
    let second_color = color.gamma_multiply(blend);
    paint_setup_text(ui, rect, first.as_ref(), font.clone(), first_color);
    if blend > 0.0 {
        paint_setup_text(ui, rect, second.as_ref(), font, second_color);
    }
}

pub(super) fn paint_setup_text(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
) {
    if let Some((left, right)) = text.split_once('→') {
        let left = ui
            .painter()
            .layout_no_wrap(left.trim().to_owned(), font.clone(), color);
        let right = ui
            .painter()
            .layout_no_wrap(right.trim().to_owned(), font, color);
        let arrow_width = 24.0;
        let total_width = left.size().x + arrow_width + right.size().x;
        let top = rect.center().y - left.size().y.max(right.size().y) / 2.0;
        let left_pos = Pos2::new(rect.center().x - total_width / 2.0, top);
        ui.painter().galley(left_pos, left.clone(), color);
        let arrow_center = Pos2::new(
            left_pos.x + left.size().x + arrow_width / 2.0,
            rect.center().y,
        );
        let arrow_start = Pos2::new(arrow_center.x - 6.0, arrow_center.y);
        let arrow_end = Pos2::new(arrow_center.x + 6.0, arrow_center.y);
        ui.painter()
            .line_segment([arrow_start, arrow_end], Stroke::new(1.3, color));
        ui.painter().line_segment(
            [arrow_end, arrow_end + egui::vec2(-3.5, -3.5)],
            Stroke::new(1.3, color),
        );
        ui.painter().line_segment(
            [arrow_end, arrow_end + egui::vec2(-3.5, 3.5)],
            Stroke::new(1.3, color),
        );
        ui.painter().galley(
            Pos2::new(left_pos.x + left.size().x + arrow_width, top),
            right,
            color,
        );
    } else {
        let galley = ui
            .painter()
            .layout(text.to_owned(), font, color, rect.width());
        let pos = Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(pos, galley, color);
    }
}

pub(super) fn setup_demo_rule(ui: &mut egui::Ui, width: f32, blend: f32, background: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), Sense::hover());
    paint_setup_crossfade_text(
        ui,
        rect,
        "No media in JW Library → Zoom sees the webcam",
        "Media detected in JW Library → Zoom sees the secondary screen",
        blend,
        FontId::proportional(13.0),
        mix_color(background, SETUP_SIGNAL_WHITE, 0.82),
    );
}

pub(super) fn setup_plain_zoom_reminder(ui: &mut egui::Ui, width: f32, background: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::hover());
    let color = mix_color(background, SETUP_SIGNAL_WHITE, 0.76);
    let text = tr(
        ui,
        "StageSwap sends the webcam or JW Library screen to Zoom through one virtual camera.",
    );
    let galley = ui
        .painter()
        .layout_no_wrap(text.into_owned(), FontId::proportional(12.5), color);
    let icon_size = 16.0;
    let gap = 8.0;
    let total_width = icon_size + gap + galley.size().x;
    let icon_rect = Rect::from_min_size(
        Pos2::new(
            rect.center().x - total_width / 2.0,
            rect.center().y - icon_size / 2.0,
        ),
        egui::vec2(icon_size, icon_size),
    );
    ui_icon::paint(ui.painter(), icon_rect, UiIcon::Broadcast, SETTINGS_BLUE);
    ui.painter().galley(
        Pos2::new(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

pub(super) fn setup_static_switching_demo(
    ui: &mut egui::Ui,
    textures: &SetupExampleTextures,
    background: Color32,
) {
    let width = ui.available_width().min(700.0);
    let row_height = (ui.available_height() / 2.0 - 18.0).clamp(105.0, 132.0);
    setup_static_signal_row(
        ui,
        width,
        row_height,
        "No media in JW Library → Zoom sees the webcam",
        textures,
        false,
        background,
    );
    ui.add_space(7.0);
    setup_static_signal_row(
        ui,
        width,
        row_height,
        "Media detected in JW Library → Zoom sees the secondary screen",
        textures,
        true,
        background,
    );
    ui.add_space(5.0);
    setup_plain_zoom_reminder(ui, width.min(620.0), background);
}

pub(super) fn setup_static_signal_row(
    ui: &mut egui::Ui,
    width: f32,
    row_height: f32,
    rule: &str,
    textures: &SetupExampleTextures,
    changed: bool,
    background: Color32,
) {
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, row_height), Sense::hover());
    let rule_rect = Rect::from_min_size(row.min, egui::vec2(width, 24.0));
    paint_setup_text(
        ui,
        rule_rect,
        tr(ui, rule).as_ref(),
        FontId::proportional(12.0),
        mix_color(background, SETUP_SIGNAL_WHITE, 0.82),
    );
    let media_width = ((row_height - 47.0) * WINDOW_ASPECT_RATIO).clamp(86.0, 104.0);
    let media_height = media_width / WINDOW_ASPECT_RATIO;
    let zoom_width = media_width * 1.2;
    let stage_width = media_width * 2.0 + zoom_width + 90.0;
    let stage_left = row.center().x - stage_width / 2.0;
    let caption_top = rule_rect.bottom() + 3.0;
    let image_top = caption_top + 17.0;
    let screen = Rect::from_min_size(
        Pos2::new(stage_left, image_top),
        egui::vec2(media_width, media_height),
    );
    let webcam = Rect::from_min_size(
        Pos2::new(screen.right() + 34.0, image_top),
        egui::vec2(media_width, media_height),
    );
    let output = Rect::from_min_size(
        Pos2::new(
            row.right() - (row.width() - stage_width) / 2.0 - zoom_width,
            image_top,
        ),
        egui::vec2(zoom_width, zoom_width / WINDOW_ASPECT_RATIO),
    );
    let screen_card = Rect::from_min_max(
        Pos2::new(screen.left() - 8.0, caption_top - 5.0),
        Pos2::new(screen.right() + 8.0, screen.bottom() + 8.0),
    );
    let webcam_card = Rect::from_min_max(
        Pos2::new(webcam.left() - 8.0, caption_top - 5.0),
        Pos2::new(webcam.right() + 8.0, webcam.bottom() + 8.0),
    );
    paint_setup_group_card(ui, screen_card, f32::from(changed), background);
    paint_setup_group_card(ui, webcam_card, f32::from(!changed), background);
    let label_color = mix_color(background, SETUP_SIGNAL_WHITE, 0.62);
    for (rect, label) in [(screen, "JW Library"), (webcam, "Webcam"), (output, "Zoom")] {
        paint_setup_centered_label(
            ui,
            Rect::from_min_size(
                Pos2::new(rect.left(), caption_top),
                egui::vec2(rect.width(), 15.0),
            ),
            label,
            label_color,
        );
    }
    setup_rounded_image(
        ui,
        screen,
        if changed {
            textures.screen.id()
        } else {
            textures.reference.id()
        },
        1.0,
        background,
        7,
    );
    setup_rounded_image(
        ui,
        webcam,
        textures.webcam.id(),
        if changed { 0.58 } else { 1.0 },
        background,
        7,
    );
    setup_rounded_image(
        ui,
        output,
        if changed {
            textures.screen.id()
        } else {
            textures.webcam.id()
        },
        1.0,
        background,
        7,
    );
    paint_setup_active_outline(ui, output, 1.0);
}

pub(super) fn setup_footer_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &str,
    size: Vec2,
    style: SetupFooterButtonStyle,
    enabled: bool,
) -> egui::Response {
    let primary = matches!(style, SetupFooterButtonStyle::Primary { .. });
    let icon_after = matches!(style, SetupFooterButtonStyle::Primary { icon_after: true });
    let icon_only = matches!(style, SetupFooterButtonStyle::IconOnly);
    let label = tr(ui, label);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let amount = if enabled { 1.0 } else { 0.34 };
    let hovered = enabled && response.hovered();
    let fill = if primary {
        if hovered {
            mix_color(SETTINGS_BLUE, SETUP_SIGNAL_WHITE, 0.12)
        } else {
            SETTINGS_BLUE
        }
    } else if hovered {
        SETUP_SIGNAL_DECK
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if primary {
        Stroke::NONE
    } else {
        Stroke::new(
            1.0,
            mix_color(SETUP_BOOTH_BLACK, SETUP_SIGNAL_WHITE, 0.18).gamma_multiply(amount),
        )
    };
    ui.painter()
        .rect_filled(rect, 7, fill.gamma_multiply(amount));
    ui.painter()
        .rect_stroke(rect, 7, stroke, StrokeKind::Inside);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            9,
            Stroke::new(2.0, SETTINGS_BLUE),
            StrokeKind::Outside,
        );
    }
    let color = SETUP_SIGNAL_WHITE.gamma_multiply(amount);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::proportional(13.5), color);
    let icon_size = 15.0;
    let icon_rect = if icon_only {
        Rect::from_center_size(rect.center(), egui::vec2(icon_size, icon_size))
    } else {
        let gap = 7.0;
        let content_width = icon_size + gap + galley.size().x;
        let content_left = rect.center().x - content_width / 2.0;
        let icon_left = if icon_after {
            content_left + galley.size().x + gap
        } else {
            content_left
        };
        Rect::from_min_size(
            Pos2::new(icon_left, rect.center().y - icon_size / 2.0),
            egui::vec2(icon_size, icon_size),
        )
    };
    ui_icon::paint(ui.painter(), icon_rect, icon, color);
    if !icon_only {
        let gap = 7.0;
        let text_left = if icon_after {
            icon_rect.left() - gap - galley.size().x
        } else {
            icon_rect.right() + gap
        };
        ui.painter().galley(
            Pos2::new(text_left, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.as_ref())
    });
    let response = if icon_only {
        response.on_hover_text(label)
    } else {
        response
    };
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(super) fn setup_reference_example_thumbnail(
    ui: &mut egui::Ui,
    texture: egui::TextureId,
    size: Vec2,
) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, 7, ui_theme::PREVIEW_BACKGROUND);
    ui.painter().image(
        texture,
        rect.shrink(2.0),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    ui.painter().rect_stroke(
        rect,
        7,
        Stroke::new(1.0, mix_color(SETUP_BOOTH_BLACK, SETUP_SIGNAL_WHITE, 0.16)),
        StrokeKind::Inside,
    );
}

pub(super) fn setup_reference_state_badge(ui: &mut egui::Ui, text: &str, tone: Color32) {
    let text = tr(ui, text);
    let galley = ui
        .painter()
        .layout_no_wrap(text.into_owned(), FontId::monospace(9.0), tone);
    let size = galley.size() + egui::vec2(12.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 4, tone.gamma_multiply(0.12));
    ui.painter().rect_stroke(
        rect,
        4,
        Stroke::new(1.0, tone.gamma_multiply(0.38)),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, tone);
}

pub(super) fn setup_capture_help(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(tr(ui, text))
                .size(11.5)
                .line_height(Some(16.0))
                .color(mix_color(ui.visuals().panel_fill, SETUP_SIGNAL_WHITE, 0.66)),
        )
        .wrap(),
    );
}

pub(super) fn setup_compact_state(ui: &mut egui::Ui, icon: UiIcon, text: &str, color: Color32) {
    let available = ui.available_width().max(1.0);
    let text = tr(ui, text);
    let text_width = (available - 21.0).max(1.0);
    let galley = ui.painter().layout(
        text.into_owned(),
        FontId::proportional(11.0),
        color,
        text_width,
    );
    let height = (galley.size().y + 6.0).max(30.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(available, height), Sense::hover());
    let icon_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - 7.0),
        egui::vec2(14.0, 14.0),
    );
    ui_icon::paint(ui.painter(), icon_rect, icon, color);
    let text_rect = Rect::from_min_max(Pos2::new(icon_rect.right() + 7.0, rect.top()), rect.max);
    ui.painter().galley(
        Pos2::new(
            text_rect.left(),
            text_rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

pub(super) fn setup_pending_capture_button(ui: &mut egui::Ui, label: &str, size: Vec2) -> Rect {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let fill = mix_color(SETUP_SIGNAL_DECK, SETTINGS_BLUE, 0.2);
    ui.painter().rect_filled(rect, 6, fill);
    ui.painter().rect_stroke(
        rect,
        6,
        Stroke::new(1.0, SETTINGS_BLUE.gamma_multiply(0.58)),
        StrokeKind::Inside,
    );
    let label = tr(ui, label);
    let color = SETUP_SIGNAL_WHITE.gamma_multiply(0.78);
    let galley = ui
        .painter()
        .layout_no_wrap(label.into_owned(), FontId::proportional(12.0), color);
    let spinner_size = 14.0;
    let gap = 7.0;
    let content_width = spinner_size + gap + galley.size().x;
    let spinner_rect = Rect::from_min_size(
        Pos2::new(
            rect.center().x - content_width / 2.0,
            rect.center().y - spinner_size / 2.0,
        ),
        egui::vec2(spinner_size, spinner_size),
    );
    let mut spinner_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("setup-reference-capture-spinner")
            .max_rect(spinner_rect)
            .layout(egui::Layout::centered_and_justified(
                egui::Direction::TopDown,
            )),
    );
    spinner_ui.add(egui::Spinner::new().size(spinner_size).color(SETTINGS_BLUE));
    ui.painter().galley(
        Pos2::new(
            spinner_rect.right() + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    rect
}

pub(super) fn setup_control_label(ui: &mut egui::Ui, icon: UiIcon, label: &str, accent: Color32) {
    ui.horizontal(|ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), Sense::hover());
        ui_icon::paint(ui.painter(), icon_rect, icon, accent);
        ui.label(
            RichText::new(tr(ui, label))
                .size(12.0)
                .strong()
                .color(SETUP_SIGNAL_WHITE),
        );
    });
}

pub(super) fn setup_message(ui: &mut egui::Ui, text: &str, color: Color32) {
    let frame = egui::Frame::new()
        .fill(mix_color(SETUP_SIGNAL_DECK, color, 0.08))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.3)))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(tr(ui, text))
                        .size(11.5)
                        .line_height(Some(17.0))
                        .color(mix_color(SETUP_BOOTH_BLACK, SETUP_SIGNAL_WHITE, 0.82)),
                )
                .wrap(),
            );
        });
    let stripe = Rect::from_min_max(
        Pos2::new(frame.response.rect.left(), frame.response.rect.top() + 7.0),
        Pos2::new(
            frame.response.rect.left() + 3.0,
            frame.response.rect.bottom() - 7.0,
        ),
    );
    ui.painter().rect_filled(stripe, 2, color);
}

pub(super) fn setup_link(ui: &mut egui::Ui, text: &str) -> bool {
    let text = tr(ui, text);
    ui.add(
        egui::Label::new(
            RichText::new(text.as_ref())
                .size(11.0)
                .underline()
                .color(SETTINGS_BLUE),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

pub(super) fn setup_warning_callout(ui: &mut egui::Ui, width: f32, title: &str, text: &str) {
    let title = tr(ui, title);
    let text = tr(ui, text);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 1.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            egui::Frame::new()
                .fill(mix_color(SETUP_SIGNAL_DECK, TRANSITION_AMBER, 0.14))
                .stroke(Stroke::new(1.5, TRANSITION_AMBER.gamma_multiply(0.72)))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_width((width - 32.0).max(1.0));
                    ui.horizontal_top(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
                        ui_icon::paint(ui.painter(), icon_rect, UiIcon::Warning, TRANSITION_AMBER);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(title.as_ref())
                                    .size(11.0)
                                    .strong()
                                    .color(TRANSITION_AMBER),
                            );
                            ui.add_space(3.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(text.as_ref())
                                        .size(13.0)
                                        .strong()
                                        .line_height(Some(19.0))
                                        .color(SETUP_SIGNAL_WHITE),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
        },
    );
}
