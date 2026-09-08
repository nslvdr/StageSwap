use super::*;

impl SwitcherApp {
    pub(super) fn queue_settings_save(&mut self) {
        self.pending_settings_save = Some(Instant::now());
    }

    pub(super) fn settings_save_due(&self, now: Instant) -> bool {
        self.pending_settings_save.is_some_and(|started_at| {
            now.saturating_duration_since(started_at) >= SETTINGS_SAVE_DEBOUNCE
        })
    }

    pub(super) fn flush_settings(&mut self) {
        if self.pending_settings_save.take().is_none() {
            return;
        }
        self.send(Command::UpdateSettings(Box::new(self.config.clone())));
        self.persist_settings();
    }

    pub(super) fn persist_settings(&mut self) {
        if let Some(writer) = &self.settings_writer {
            writer.submit(self.config.clone());
        } else {
            self.record_settings_save_error(
                "Could not save settings: settings writer is unavailable".into(),
            );
        }
    }

    pub(super) fn poll_settings_save(&mut self) {
        if let Some(result) = self
            .settings_writer
            .as_ref()
            .and_then(|writer| writer.poll())
        {
            match result {
                Ok(()) => self.settings_save_error = None,
                Err(error) => self.record_settings_save_error(error),
            }
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_settings_save(&mut self) {
        assert!(
            self.settings_writer
                .as_ref()
                .unwrap()
                .drain(Duration::from_secs(2))
        );
        self.poll_settings_save();
    }

    pub(super) fn record_settings_save_error(&mut self, message: String) {
        self.log
            .write("error", "configuration", "SETTINGS_SAVE_FAILED", &message);
        self.settings_save_error = Some(message.clone());
        self.push_notification_warning(NotificationSource::Configuration, message);
    }

    pub(super) fn sync_selected_monitor_preference(&mut self, snapshot: &AppSnapshot) {
        let Some(monitor) = snapshot.selected_monitor.as_ref() else {
            return;
        };
        if self.config.selected_monitor_label == monitor.label {
            return;
        }
        self.config
            .selected_monitor_label
            .clone_from(&monitor.label);
        self.persist_settings();
    }

    pub(super) fn open_settings(&mut self) {
        self.previews.enlarged = None;
        self.view = AppView::Settings;
        self.disco_diagnostics_gesture.reset();
        self.settings_opened_at = Some(Instant::now());
        self.settings_section_changed_at = Some(Instant::now());
        self.send(Command::RefreshVideoDevices);
        if self.config.automatic_monitor_rescans {
            self.send(Command::Rescan);
        }
    }

    pub(super) fn settings_view(&mut self, context: &egui::Context, ui: &mut egui::Ui) {
        if self.active_dialog.is_none()
            && context.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.close_settings();
            return;
        }

        let entrance = animation_progress(self.settings_opened_at, SETTINGS_ENTRANCE_DURATION);
        if entrance < 1.0 {
            context.request_repaint();
        }
        ui.painter()
            .rect_filled(ui.max_rect(), 0, ui_theme::BACKGROUND);
        ui.set_min_size(ui.available_size());
        ui.scope(|ui| {
            ui.set_opacity(0.72 + entrance * 0.28);
            if self.settings_workspace(ui) {
                self.close_settings();
            }
        });
    }

    pub(super) fn settings_workspace(&mut self, ui: &mut egui::Ui) -> bool {
        let workspace_height = ui.available_height().max(0.0);
        ui.horizontal_top(|ui| {
            let sidebar = ui.allocate_ui_with_layout(
                egui::vec2(SETTINGS_SIDEBAR_WIDTH, workspace_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.settings_sidebar(ui, workspace_height),
            );

            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), workspace_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.settings_content(ui),
            );
            debug_assert!(sidebar.inner.brand_icon.is_positive());
            debug_assert!(sidebar.inner.brand_title.is_positive());
            debug_assert!(sidebar.inner.brand_version.is_positive());
            debug_assert!(sidebar.inner.brand_separator.is_positive());
            debug_assert!(sidebar.inner.back.is_positive());
            debug_assert_eq!(
                sidebar.inner.primary_navigation.len(),
                SettingsTab::PRIMARY.len()
            );
            debug_assert!(sidebar.inner.updates.is_positive());
            debug_assert!(sidebar.inner.diagnostics.is_positive());
            sidebar.inner.go_back
        })
        .inner
    }

    pub(super) fn settings_sidebar(
        &mut self,
        ui: &mut egui::Ui,
        height: f32,
    ) -> SettingsSidebarLayout {
        let app_icon_texture = self.app_icon_texture(ui.ctx());
        let disco_enabled = self.snapshot().disco_enabled;
        let disco_elapsed = Instant::now().saturating_duration_since(self.ui_animation_started_at);
        let sidebar_fill = if disco_enabled {
            mix_color(
                SETTINGS_SIDEBAR_FILL,
                disco_ui_color(disco_elapsed, 0.58),
                0.12,
            )
        } else {
            SETTINGS_SIDEBAR_FILL
        };
        egui::Frame::new()
            .fill(sidebar_fill)
            .corner_radius(SETTINGS_SIDEBAR_CORNER_RADIUS)
            .inner_margin(egui::Margin::symmetric(10, 12))
            .show(ui, |ui| {
                ui.set_width(SETTINGS_SIDEBAR_WIDTH - 20.0);
                ui.set_min_height((height - 24.0).max(0.0));

                let brand_icon = ui
                    .allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 68.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(68.0, 68.0), Sense::click());
                            if disco_enabled {
                                for ring in 0..3 {
                                    let color = disco_ui_color(disco_elapsed, ring as f32 / 3.0);
                                    ui.painter().circle_stroke(
                                        rect.center(),
                                        rect.width() * (0.51 + ring as f32 * 0.035),
                                        Stroke::new(
                                            3.0 - ring as f32 * 0.7,
                                            Color32::from_rgba_unmultiplied(
                                                color.r(),
                                                color.g(),
                                                color.b(),
                                                (190 - ring * 45) as u8,
                                            ),
                                        ),
                                    );
                                }
                            }
                            ui.painter().image(
                                app_icon_texture,
                                rect,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                            if response.clicked()
                                || response.clicked_by(egui::PointerButton::Secondary)
                            {
                                self.disco_diagnostics_gesture.reset();
                            }
                            if admin_logo_activated(&response) {
                                self.open_admin_configuration();
                            }
                            rect
                        },
                    )
                    .inner;
                ui.add_space(6.0);
                let brand_title = ui
                    .allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 22.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.label(
                                RichText::new("StageSwap")
                                    .size(16.0)
                                    .strong()
                                    .color(Color32::from_rgb(220, 225, 234)),
                            )
                            .rect
                        },
                    )
                    .inner;
                ui.add_space(1.0);
                let brand_version = ui
                    .allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 16.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.label(
                                RichText::new(APP_VERSION_LABEL)
                                    .size(11.0)
                                    .color(Color32::from_rgb(132, 140, 154)),
                            )
                            .rect
                        },
                    )
                    .inner;
                ui.add_space(8.0);
                let brand_separator = ui.separator().rect;
                ui.add_space(8.0);
                let back = settings_back_button(ui);
                let go_back = back.clicked();
                if go_back {
                    self.disco_diagnostics_gesture.reset();
                }
                ui.add_space(10.0);
                ui.label(
                    RichText::new(tr(ui, "PREFERENCES"))
                        .size(10.0)
                        .strong()
                        .color(ui_theme::TEXT_DIM),
                );
                ui.add_space(8.0);
                let mut primary_navigation = Vec::with_capacity(SettingsTab::PRIMARY.len());
                for (index, (tab, icon)) in SettingsTab::PRIMARY.into_iter().enumerate() {
                    let label = tab.title(self.locale());
                    let response = settings_nav_button(
                        ui,
                        tab,
                        icon,
                        label.as_ref(),
                        self.settings_tab,
                        disco_enabled.then(|| disco_ui_color(disco_elapsed, index as f32 * 0.13)),
                    );
                    primary_navigation.push(response.rect);
                    if response.clicked() {
                        self.disco_diagnostics_gesture.reset();
                    }
                    if response.clicked() && self.settings_tab != tab {
                        self.settings_tab = tab;
                        self.dismiss_dialog();
                        self.settings_section_changed_at = Some(Instant::now());
                    }
                    if index + 1 < SettingsTab::PRIMARY.len() {
                        ui.add_space(3.0);
                    }
                }

                let (updates, diagnostics) = ui
                    .with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        let (tab, icon) = SettingsTab::DIAGNOSTICS;
                        let label = tab.title(self.locale());
                        let response = settings_nav_button(
                            ui,
                            tab,
                            icon,
                            label.as_ref(),
                            self.settings_tab,
                            disco_enabled.then(|| disco_ui_color(disco_elapsed, 0.76)),
                        );
                        if response.clicked_by(egui::PointerButton::Secondary) {
                            self.disco_diagnostics_gesture.reset();
                        }
                        if response.clicked_by(egui::PointerButton::Primary)
                            && self
                                .disco_diagnostics_gesture
                                .register_primary_click(Instant::now())
                        {
                            self.toggle_disco();
                        }
                        if response.clicked() && self.settings_tab != tab {
                            self.settings_tab = tab;
                            self.dismiss_dialog();
                            self.settings_section_changed_at = Some(Instant::now());
                        }
                        let diagnostics = response.rect;
                        ui.add_space(3.0);
                        let (tab, icon) = SettingsTab::UPDATES;
                        let label = tab.title(self.locale());
                        let response = settings_nav_button(
                            ui,
                            tab,
                            icon,
                            label.as_ref(),
                            self.settings_tab,
                            disco_enabled.then(|| disco_ui_color(disco_elapsed, 0.64)),
                        );
                        if response.clicked() && self.settings_tab != tab {
                            self.disco_diagnostics_gesture.reset();
                            self.settings_tab = tab;
                            self.dismiss_dialog();
                            self.settings_section_changed_at = Some(Instant::now());
                        }
                        (response.rect, diagnostics)
                    })
                    .inner;

                SettingsSidebarLayout {
                    brand_icon,
                    brand_title,
                    brand_version,
                    brand_separator,
                    back: back.rect,
                    primary_navigation,
                    updates,
                    diagnostics,
                    go_back,
                }
            })
            .inner
    }

    pub(super) fn settings_content(&mut self, ui: &mut egui::Ui) {
        let section_progress =
            animation_progress(self.settings_section_changed_at, SETTINGS_SECTION_DURATION);
        if section_progress < 1.0 {
            ui.ctx().request_repaint();
        }
        let before = self.config.clone();
        if let Some(message) = self.settings_save_error.clone() {
            settings_save_error_callout(ui, &message);
            ui.add_space(10.0);
        }
        egui::ScrollArea::vertical()
            .id_salt(("settings-content", self.settings_tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available_width = ui.available_width();
                let content_width = settings_content_width(available_width);
                ui.allocate_ui_with_layout(
                    egui::vec2(available_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(content_width.max(1.0), ui.available_height()),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_opacity(0.62 + section_progress * 0.38);
                                ui.add_space(18.0 + (1.0 - section_progress) * 5.0);
                                ui.horizontal(|ui| {
                                    let (icon_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(22.0, 22.0),
                                        Sense::hover(),
                                    );
                                    ui_icon::paint(
                                        ui.painter(),
                                        icon_rect,
                                        self.settings_tab.icon(),
                                        SETTINGS_BLUE,
                                    );
                                    ui.label(
                                        RichText::new(self.settings_tab.title(self.locale()))
                                            .size(23.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                });
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(self.settings_tab.description(self.locale()))
                                        .size(13.0)
                                        .color(ui_theme::TEXT_MUTED),
                                );
                                ui.add_space(18.0);
                                match self.settings_tab {
                                    SettingsTab::General => self.general_settings(ui),
                                    SettingsTab::Webcam => self.webcam_settings(ui),
                                    SettingsTab::Screen => self.screen_settings(ui),
                                    SettingsTab::Matching => self.matching_settings(ui),
                                    SettingsTab::Updates => self.updates_settings(ui),
                                    SettingsTab::Diagnostics => self.diagnostics_settings(ui),
                                }
                                ui.add_space(22.0);
                            },
                        );
                    },
                );
            });
        #[cfg(windows)]
        if self.config.start_with_windows != before.start_with_windows
            && self.portable_mode == stageswap_windows::PortableMode::Managed
            && let Err(error) = stageswap_windows::configure_startup(self.config.start_with_windows)
        {
            self.config.start_with_windows = before.start_with_windows;
            let message = format!("Could not update Start with Windows: {error}");
            self.push_notification_warning(NotificationSource::Configuration, message);
        }
        if self.config != before {
            if self.config.selected_video_device_id != before.selected_video_device_id {
                self.awaiting_video_device_id = Some(self.config.selected_video_device_id.clone());
            }
            self.queue_settings_save();
        }
    }

    pub(super) fn general_settings(&mut self, ui: &mut egui::Ui) {
        settings_info_card(
            ui,
            &[
                "StageSwap automatically switches what Zoom sees between the webcam and JW Library presentations. When the secondary screen matches the reference image, Zoom sees the webcam. When media is detected, Zoom sees the secondary screen. When no media is detected again, Zoom returns to the webcam.",
                "StageSwap is an independent, unofficial project and is not affiliated with or endorsed by the publisher of JW Library. The name JW Library is used only to describe compatibility.",
            ],
        );
        ui.add_space(12.0);
        let mut open_setup_guide = false;
        if settings_single_button_row(
            ui,
            "Guided setup",
            "Choose the webcam and secondary screen, then capture the screen JW Library shows when no media is playing.",
            "Open guided setup",
            206.0,
        )
        .clicked()
        {
            open_setup_guide = true;
        }
        if open_setup_guide {
            self.start_setup_guide(SetupReturnView::Settings);
            return;
        }

        let current_locale = self.locale();
        let mut selected_locale = current_locale;
        settings_fixed_control_row(
            ui,
            "Interface language",
            "Changes apply immediately.",
            152.0,
            |ui| {
                let combo = egui::ComboBox::from_id_salt("interface-language")
                    .width(152.0)
                    .selected_text(language_selector_text(selected_locale))
                    .show_ui(ui, |ui| {
                        for locale in Locale::ALL {
                            let option = ui.selectable_value(
                                &mut selected_locale,
                                locale,
                                language_selector_text(locale),
                            );
                            paint_language_flag(
                                ui.painter(),
                                language_flag_rect(option.rect),
                                locale,
                            );
                        }
                    });
                paint_language_flag(
                    ui.painter(),
                    language_flag_rect(combo.response.rect),
                    selected_locale,
                );
            },
        );
        if selected_locale != current_locale {
            self.config.interface_language = selected_locale.tag().into();
            set_ui_locale(ui.ctx(), selected_locale);
        }

        settings_section_gap(ui);
        settings_section_heading(
            ui,
            UiIcon::Play,
            "Startup",
            "Choose what StageSwap does after you sign in to Windows.",
        );
        #[cfg(windows)]
        if self.portable_mode == stageswap_windows::PortableMode::RunOnce {
            let mut disabled = false;
            ui.add_enabled_ui(false, |ui| {
                settings_toggle_row(
                    ui,
                    &mut disabled,
                    "Start with Windows",
                    "Install StageSwap to use a stable Windows startup path.",
                );
            });
            if translated_button(ui, "Install StageSwap to enable startup").clicked()
                && let Err(error) = stageswap_windows::request_install()
            {
                self.push_notification_warning(
                    NotificationSource::Configuration,
                    format!("Could not start installation: {error}"),
                );
            }
        } else {
            settings_toggle_row(
                ui,
                &mut self.config.start_with_windows,
                "Start with Windows",
                "Launch after Windows sign-in.",
            );
        }
        #[cfg(not(windows))]
        {
            settings_toggle_row(
                ui,
                &mut self.config.start_with_windows,
                "Start with Windows",
                "Launch after Windows sign-in.",
            );
        }
        settings_toggle_row(
            ui,
            &mut self.config.start_minimized,
            "Start minimized",
            "Open in the system tray.",
        );
        settings_toggle_row(
            ui,
            &mut self.config.start_automatically,
            "Start automatic switching on launch",
            "Start automatic switching when StageSwap launches.",
        );

        settings_section_gap(ui);
        settings_section_heading(
            ui,
            UiIcon::Window,
            "Window behavior",
            "Choose what happens when you close the StageSwap window.",
        );
        settings_toggle_row(
            ui,
            &mut self.config.close_to_tray,
            "Keep running in system tray",
            "Hide the window while StageSwap keeps running.",
        );
        settings_toggle_row_without_separator(
            ui,
            &mut self.config.confirm_exit,
            "Confirm before exit",
            "Ask before StageSwap fully exits.",
        );
        ui.separator();

        settings_section_gap(ui);
        settings_section_heading(
            ui,
            UiIcon::Bell,
            "Notifications",
            "Choose whether StageSwap shows critical alerts in the app.",
        );
        settings_toggle_row(
            ui,
            &mut self.config.show_notifications,
            "Show in-app notifications",
            "Show critical issues in the bell and as brief in-app banners.",
        );
    }

    pub(super) fn webcam_settings(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.snapshot();
        let selected_name = snapshot
            .video_devices
            .iter()
            .find(|device| device.id == self.config.selected_video_device_id)
            .map(|device| device.name.clone())
            .unwrap_or_else(|| {
                if self.config.selected_video_device_id.is_empty() {
                    localized_text(self.locale(), "No camera selected").into_owned()
                } else {
                    localized_text(self.locale(), "Saved camera is unavailable").into_owned()
                }
            });
        self.settings_preview_control_row(
            ui,
            SettingsPreview {
                kind: PreviewKind::Webcam,
                frame: snapshot.previews.webcam.as_ref(),
                label: "Selected webcam",
                empty_message: "No webcam frame — choose a camera or refresh the device list.",
                actual_output: snapshot.actual_output,
            },
            |app, ui| {
                ui.label(
                    RichText::new(tr(ui, "Camera"))
                        .size(12.0)
                        .color(ui_theme::TEXT_PRIMARY),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let geometry = selector_utility_geometry(
                        ui.available_width(),
                        ui.spacing().item_spacing.x,
                    );
                    egui::ComboBox::from_id_salt("webcam-selector")
                        .width(geometry.selector_width)
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut app.config.selected_video_device_id,
                                String::new(),
                                tr(ui, "No camera selected").as_ref(),
                            );
                            for device in snapshot.video_devices.iter() {
                                ui.selectable_value(
                                    &mut app.config.selected_video_device_id,
                                    device.id.clone(),
                                    &device.name,
                                );
                            }
                        });
                    if icon_only_button(
                        ui,
                        UiIcon::Refresh,
                        "Refresh camera devices",
                        egui::vec2(geometry.action_width, 32.0),
                    )
                    .on_hover_text(tr(ui, "Refresh camera devices"))
                    .clicked()
                    {
                        app.send(Command::RefreshVideoDevices);
                    }
                });
                ui.add_space(8.0);
                settings_toggle_row_without_separator(
                    ui,
                    &mut app.config.crop_webcam_to_16_9,
                    "Crop webcam to 16:9",
                    "Crop non-16:9 cameras to fill the frame.",
                );
            },
        );
    }

    pub(super) fn screen_settings(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.snapshot();
        let selected_monitor = snapshot.selected_monitor.as_ref().map_or_else(
            || localized_text(self.locale(), "No display selected").into_owned(),
            |monitor| monitor.label.clone(),
        );
        self.settings_preview_control_row(
            ui,
            SettingsPreview {
                kind: PreviewKind::Screen,
                frame: snapshot.previews.screen.as_ref(),
                label: "Live secondary screen",
                empty_message:
                    "No secondary screen image — choose a screen or use Tools in Diagnostics.",
                actual_output: snapshot.actual_output,
            },
            |app, ui| {
                ui.label(
                    RichText::new(tr(ui, "Display"))
                        .size(12.0)
                        .color(ui_theme::TEXT_PRIMARY),
                );
                ui.add_space(4.0);
                egui::ComboBox::from_id_salt("monitor-selector")
                    .width(ui.available_width())
                    .selected_text(&selected_monitor)
                    .show_ui(ui, |ui| {
                        for monitor in snapshot.monitors.iter() {
                            let label =
                                format!("{} — {}×{}", monitor.label, monitor.width, monitor.height);
                            if ui
                                .selectable_label(
                                    snapshot.selected_monitor.as_ref() == Some(monitor),
                                    label,
                                )
                                .clicked()
                            {
                                app.send(Command::SelectMonitor(monitor.clone()));
                            }
                        }
                    });
                ui.add_space(12.0);
                settings_group_label(ui, "Capture behavior");
                settings_toggle_row(
                    ui,
                    &mut app.config.cursor_visible,
                    "Include mouse cursor",
                    "New reference images use this setting; existing and imported images do not change.",
                );

                ui.add_space(12.0);
                settings_group_label(ui, "Automatic screen tools");
                settings_toggle_row(
                    ui,
                    &mut app.config.automatic_monitor_rescans,
                    "Find the JW Library screen automatically",
                    "Looks for the screen JW Library uses when StageSwap starts, when Settings opens, after the reference image changes, and every 30 seconds. It confirms the same screen twice before selecting it.",
                );
                settings_toggle_row(
                    ui,
                    &mut app.config.automatic_screen_capture_recovery,
                    "Automatically fix screen capture problems",
                    "Checks the selected screen every 30 seconds. If the image is black or missing twice in a row, StageSwap restarts screen capture.",
                );
            },
        );
    }

    pub(super) fn matching_settings(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.snapshot();
        self.settings_preview_control_row(
            ui,
            SettingsPreview {
                kind: PreviewKind::Reference,
                frame: snapshot.previews.reference.as_ref(),
                label: "Reference image",
                empty_message:
                    "No reference image — show the screen JW Library shows when no media is playing, then capture it.",
                actual_output: snapshot.actual_output,
            },
            |app, ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(tr(
                            ui,
                            "Checks 4×/s · confirms after 5 matches or 3 differences · 0.5s fade",
                        ))
                        .size(10.0)
                        .color(ui_theme::TEXT_SUBTLE),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let geometry = reference_control_geometry(
                        ui.available_width(),
                        ui.spacing().item_spacing.x,
                    );
                    if icon_button(
                        ui,
                        UiIcon::Capture,
                        "Capture reference image",
                        egui::vec2(geometry.action_width, 32.0),
                        false,
                        false,
                    )
                    .clicked()
                    {
                        app.begin_reference_capture();
                    }
                    if icon_button(
                        ui,
                        UiIcon::Image,
                        "Import image…",
                        egui::vec2(geometry.action_width, 32.0),
                        false,
                        false,
                    )
                    .clicked()
                    {
                        app.import_reference_dialog();
                    }
                });
                ui.add_space(12.0);
                let strictness = format!("{:.0}%", app.config.similarity_threshold * 100.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(tr(ui, "Required similarity"))
                            .size(12.0)
                            .color(ui_theme::TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if translated_small_button(ui, "Reset 98%").clicked() {
                            app.config.similarity_threshold = 0.98;
                        }
                        ui.label(RichText::new(strictness).monospace());
                    });
                });
                let geometry = reference_control_geometry(
                    ui.available_width(),
                    ui.spacing().item_spacing.x,
                );
                match_strictness_slider(
                    ui,
                    &mut app.config.similarity_threshold,
                    geometry.slider_width,
                );
                let explanation =
                    match_strictness_explanation(app.config.similarity_threshold);
                ui.add_space(3.0);
                let level = localized_text(app.locale(), explanation.level);
                let effect = localized_text(app.locale(), explanation.effect);
                settings_result_text(
                    ui,
                    &format_text(app.locale(), "{0} — {1}", &[level.as_ref(), effect.as_ref()]),
                );
            },
        );

        settings_section_gap(ui);
        settings_section_heading(
            ui,
            UiIcon::Layers,
            "Picture-in-picture",
            "Choose the PIP layout and optionally use it automatically for still images.",
        );
        settings_toggle_row(
            ui,
            &mut self.config.still_image_pip_enabled,
            "Use automatically for still images",
            "In Auto mode, show both feeds after a non-reference image remains unchanged.",
        );
        let enabled = self.config.still_image_pip_enabled;
        ui.add_enabled_ui(enabled, |ui| {
            let delay = match self.config.still_image_pip_delay_seconds {
                30 => tr(ui, "30 seconds"),
                45 => tr(ui, "45 seconds"),
                60 => tr(ui, "1 minute"),
                120 => tr(ui, "2 minutes"),
                300 => tr(ui, "5 minutes"),
                _ => tr(ui, "45 seconds"),
            };
            settings_fixed_control_row(
                ui,
                "Show picture-in-picture after",
                "Any screen movement restarts this timer.",
                180.0,
                |ui| {
                    egui::ComboBox::from_id_salt("still-image-pip-delay")
                        .width(180.0)
                        .selected_text(delay)
                        .show_ui(ui, |ui| {
                            for (seconds, label) in [
                                (30, "30 seconds"),
                                (45, "45 seconds"),
                                (60, "1 minute"),
                                (120, "2 minutes"),
                                (300, "5 minutes"),
                            ] {
                                ui.selectable_value(
                                    &mut self.config.still_image_pip_delay_seconds,
                                    seconds,
                                    tr(ui, label).as_ref(),
                                );
                            }
                        });
                },
            );
        });
        let main_view = match self.config.still_image_pip_layout {
            StillImagePipLayout::WebcamMain => tr(ui, "Webcam full screen"),
            StillImagePipLayout::ScreenMain => tr(ui, "Secondary screen full screen"),
        };
        settings_fixed_control_row(
            ui,
            "Main view",
            "Used by automatic and forced PIP; the other feed appears in the bottom-left inset.",
            220.0,
            |ui| {
                egui::ComboBox::from_id_salt("still-image-pip-layout")
                    .width(220.0)
                    .selected_text(main_view)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config.still_image_pip_layout,
                            StillImagePipLayout::WebcamMain,
                            tr(ui, "Webcam full screen").as_ref(),
                        );
                        ui.selectable_value(
                            &mut self.config.still_image_pip_layout,
                            StillImagePipLayout::ScreenMain,
                            tr(ui, "Secondary screen full screen").as_ref(),
                        );
                    });
            },
        );
        let pip_size = match self.config.still_image_pip_size {
            StillImagePipSize::Mini => tr(ui, "Mini"),
            StillImagePipSize::Medium => tr(ui, "Medium"),
            StillImagePipSize::Large => tr(ui, "Large"),
        };
        settings_fixed_control_row(
            ui,
            "PIP size",
            "Choose how much of the main view the inset covers.",
            180.0,
            |ui| {
                egui::ComboBox::from_id_salt("still-image-pip-size")
                    .width(180.0)
                    .selected_text(pip_size)
                    .show_ui(ui, |ui| {
                        for (size, label) in [
                            (StillImagePipSize::Mini, "Mini"),
                            (StillImagePipSize::Medium, "Medium"),
                            (StillImagePipSize::Large, "Large"),
                        ] {
                            ui.selectable_value(
                                &mut self.config.still_image_pip_size,
                                size,
                                tr(ui, label).as_ref(),
                            );
                        }
                    });
            },
        );
    }

    pub(super) fn diagnostics_settings(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.snapshot();
        settings_section_heading(
            ui,
            UiIcon::Check,
            "Component health",
            "Check whether each video component and media detection are working.",
        );
        settings_device_status(ui, UiIcon::Camera, "Webcam", snapshot.webcam_state);
        settings_device_status(ui, UiIcon::Monitor, "Screen capture", snapshot.screen_state);
        settings_device_status(
            ui,
            UiIcon::Broadcast,
            "Virtual camera",
            snapshot.virtual_camera_state,
        );
        settings_detection_status(ui, snapshot.detection);
        ui.add_space(6.0);
        settings_result_text(
            ui,
            component_health_guidance(
                snapshot.webcam_state,
                snapshot.screen_state,
                snapshot.virtual_camera_state,
                snapshot.detection,
            ),
        );
        if matches!(
            snapshot.webcam_component.last_failure_kind,
            Some(ComponentFailureKind::Webcam(
                WebcamFailureKind::AccessDenied | WebcamFailureKind::PrivacyDisabled
            ))
        ) && translated_button(ui, "Open camera privacy settings").clicked()
        {
            #[cfg(windows)]
            if let Err(error) = stageswap_windows::open_camera_privacy_settings() {
                self.push_notification_warning(NotificationSource::Configuration, error);
            }
        }

        settings_section_gap(ui);
        settings_section_heading(
            ui,
            UiIcon::Wrench,
            "Tools",
            "Rescan for the JW Library screen or restart a video component.",
        );
        ui.horizontal_wrapped(|ui| {
            if icon_button(
                ui,
                UiIcon::Refresh,
                "Rescan displays",
                egui::vec2(170.0, 34.0),
                false,
                false,
            )
            .clicked()
            {
                self.send(Command::Rescan);
            }
            for (icon, label, width, target) in SETTINGS_RECOVERY_TARGETS {
                if icon_button(ui, icon, label, egui::vec2(width, 34.0), false, false).clicked() {
                    self.send(Command::Restart(target));
                }
            }
        });
        settings_section_gap(ui);
        settings_section_heading(
            ui,
            UiIcon::Info,
            "Technical details",
            "View the devices, formats, and timing StageSwap is currently using.",
        );
        settings_info_row(
            ui,
            "Webcam device ID",
            if self.config.selected_video_device_id.is_empty() {
                "No camera selected"
            } else {
                &self.config.selected_video_device_id
            },
        );
        let display_details = snapshot.selected_monitor.as_ref().map_or_else(
            || "No display selected".to_owned(),
            |monitor| {
                format!(
                    "{} · {}×{} at ({}, {}) · {}",
                    monitor.label,
                    monitor.width,
                    monitor.height,
                    monitor.x,
                    monitor.y,
                    monitor.display_name
                )
            },
        );
        settings_info_row(ui, "Display", &display_details);
        settings_info_row(
            ui,
            "Webcam format",
            &match (
                snapshot.webcam_native_format.as_deref(),
                snapshot.webcam_output_format.as_deref(),
            ) {
                (Some(native), Some(output)) => format!("Native: {native} · Output: {output}"),
                _ => "Waiting for webcam format negotiation".to_owned(),
            },
        );
        settings_info_row(
            ui,
            "Video output",
            "CPU BGRA 1280×720 at 30 fps · aspect fit · black letterboxing",
        );
        settings_info_row(
            ui,
            "Capture buffers",
            &format!(
                "Webcam: {} dropped · {} fallbacks · Screen: {} dropped · {} fallbacks",
                snapshot.webcam_capture_dropped_frames,
                snapshot.webcam_capture_pool_fallbacks,
                snapshot.screen_capture_dropped_frames,
                snapshot.screen_capture_pool_fallbacks,
            ),
        );
        settings_info_row(
            ui,
            "Output timing",
            &format!(
                "{} missed 30 fps deadline(s) this session",
                snapshot.output_deadline_misses
            ),
        );
        settings_info_row(
            ui,
            "Transitions",
            "Reversible 500 ms fade using the live screen frame",
        );
        settings_info_row(
            ui,
            "Detection timing",
            "Every 250 ms · 5 matches · 3 mismatches · full scan every 30 seconds",
        );

        settings_section_gap(ui);
        settings_section_heading(
            ui,
            UiIcon::Folder,
            "Storage and logs",
            "Find saved settings and logs, or export logs for troubleshooting.",
        );
        settings_info_row(
            ui,
            "Configuration",
            &self.store.config_path().display().to_string(),
        );
        settings_info_row(ui, "Reference image", &self.config.reference_image_path);
        settings_info_row(
            ui,
            "Log directory",
            &self.log.directory().display().to_string(),
        );
        settings_toggle_row(
            ui,
            &mut self.config.verbose_logging,
            "Verbose diagnostic logging",
            "Include detailed runtime activity and periodic health checks in local logs. Enable this only while troubleshooting.",
        );
        settings_action_row(
            ui,
            "Diagnostic logs",
            "Logs are retained for 14 days.",
            |ui| {
                ui.horizontal(|ui| {
                    if translated_button(ui, "Open folder").clicked() {
                        self.open_log_directory();
                    }
                    if translated_button(ui, "Export…").clicked() {
                        self.export_logs();
                    }
                    if translated_button(ui, "Clear…").clicked() {
                        self.request_log_clear();
                    }
                });
            },
        );
    }

    pub(super) fn settings_preview_control_row(
        &mut self,
        ui: &mut egui::Ui,
        preview: SettingsPreview<'_>,
        add_controls: impl FnOnce(&mut Self, &mut egui::Ui),
    ) -> SettingsPreviewControls {
        let available = ui.available_width();
        let preview_rect = self.settings_single_preview(ui, preview);
        ui.add_space(16.0);
        let controls_rect = ui
            .allocate_ui_with_layout(
                egui::vec2(available, 1.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(available);
                    add_controls(self, ui);
                },
            )
            .response
            .rect;
        let layout = SettingsPreviewControls {
            preview: preview_rect,
            controls: controls_rect,
        };
        debug_assert!(layout.preview.is_positive());
        debug_assert!(layout.controls.is_positive());
        debug_assert!(layout.preview.bottom() < layout.controls.top());
        layout
    }

    pub(super) fn settings_single_preview(
        &mut self,
        ui: &mut egui::Ui,
        preview: SettingsPreview<'_>,
    ) -> Rect {
        let width = ui.available_width().min(SETTINGS_PREVIEW_WIDTH);
        let mut preview_rect = Rect::NOTHING;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), width / WINDOW_ASPECT_RATIO + 26.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                preview_rect = self.settings_preview_panel(ui, preview, width);
            },
        );
        preview_rect
    }

    pub(super) fn settings_preview_panel(
        &mut self,
        ui: &mut egui::Ui,
        preview: SettingsPreview<'_>,
        width: f32,
    ) -> Rect {
        let height = (width / WINDOW_ASPECT_RATIO).min(SETTINGS_PREVIEW_HEIGHT);
        ui.allocate_ui_with_layout(
            egui::vec2(width, height + 26.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let rect = self.preview(
                    ui,
                    preview.kind,
                    preview.frame,
                    [width, height],
                    preview.actual_output,
                    PreviewOptions::settings(preview.empty_message),
                );
                ui.add_space(7.0);
                icon_text(
                    ui,
                    preview.kind.icon(),
                    preview.label,
                    ui_theme::TEXT_SECONDARY,
                    false,
                );
                rect
            },
        )
        .inner
    }

    pub(super) fn close_settings(&mut self) {
        self.flush_settings();
        self.view = AppView::Dashboard;
        self.disco_diagnostics_gesture.reset();
        self.active_dialog = None;
        self.settings_opened_at = None;
        self.settings_section_changed_at = None;
    }
}

pub(super) fn settings_back_button(ui: &mut egui::Ui) -> egui::Response {
    let label = tr(ui, "Back to dashboard");
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    let fill = if response.hovered() {
        SETTINGS_NAV_HOVERED
    } else {
        SETTINGS_SIDEBAR_FILL
    };
    ui.painter().rect_filled(rect, 6, fill);
    let foreground = if response.hovered() {
        Color32::WHITE
    } else {
        ui_theme::TEXT_SECONDARY
    };
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 22.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    ui_icon::paint(ui.painter(), icon_rect, UiIcon::Back, foreground);
    ui.painter().text(
        Pos2::new(rect.left() + 42.0, rect.center().y),
        Align2::LEFT_CENTER,
        label.as_ref(),
        FontId::proportional(13.0),
        foreground,
    );
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn settings_nav_button(
    ui: &mut egui::Ui,
    tab: SettingsTab,
    icon: UiIcon,
    label: &str,
    selected: SettingsTab,
    disco_accent: Option<Color32>,
) -> egui::Response {
    let active = tab == selected;
    let amount =
        ui.ctx()
            .animate_bool_with_time(ui.make_persistent_id(("settings-nav", tab)), active, 0.14);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    let base = if response.hovered() {
        SETTINGS_NAV_HOVERED
    } else {
        SETTINGS_SIDEBAR_FILL
    };
    let selected_fill = disco_accent.map_or(SETTINGS_NAV_SELECTED, |accent| {
        mix_color(SETTINGS_NAV_SELECTED, accent, 0.42)
    });
    ui.painter()
        .rect_filled(rect, 6, mix_color(base, selected_fill, amount));
    if amount > 0.0 {
        let indicator_height = 16.0 + 10.0 * amount;
        let indicator = Rect::from_center_size(
            Pos2::new(rect.left() + 2.0, rect.center().y),
            egui::vec2(3.0, indicator_height),
        );
        let indicator_color = disco_accent.unwrap_or(SETTINGS_NAV_INDICATOR);
        ui.painter()
            .rect_filled(indicator, 2, indicator_color.gamma_multiply(amount));
    }
    let foreground = mix_color(ui_theme::TEXT_SECONDARY, Color32::WHITE, amount);
    let icon_foreground = if disco_accent.is_some() {
        // Keep the diagnostic disco treatment unchanged while the regular
        // settings navigation uses the blue accent.
        foreground
    } else {
        SETTINGS_BLUE
    };
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 22.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    ui_icon::paint(ui.painter(), icon_rect, icon, icon_foreground);
    ui.painter().text(
        Pos2::new(rect.left() + 42.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        foreground,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn settings_section_heading(
    ui: &mut egui::Ui,
    icon: UiIcon,
    title: &str,
    description: &str,
) -> Rect {
    let title = tr(ui, title);
    let description = tr(ui, description);
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(17.0, 17.0), Sense::hover());
            ui_icon::paint(ui.painter(), rect, icon, SETTINGS_BLUE);
            ui.label(
                RichText::new(title.as_ref())
                    .size(14.0)
                    .strong()
                    .color(ui_theme::TEXT_PRIMARY),
            );
        });
        ui.add_space(2.0);
        ui.add(
            egui::Label::new(
                RichText::new(description.as_ref())
                    .size(11.0)
                    .color(ui_theme::TEXT_SUBTLE),
            )
            .wrap(),
        );
    })
    .response
    .rect
}

pub(super) fn settings_info_card(ui: &mut egui::Ui, paragraphs: &[&str]) -> Rect {
    let paragraphs = paragraphs
        .iter()
        .map(|paragraph| tr(ui, paragraph))
        .collect::<Vec<_>>();
    egui::Frame::new()
        .fill(ui_theme::SURFACE)
        .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(17.0, 17.0), Sense::hover());
                ui_icon::paint(ui.painter(), icon_rect, UiIcon::Info, SETTINGS_BLUE);
                ui.vertical(|ui| {
                    for (index, paragraph) in paragraphs.iter().enumerate() {
                        if index > 0 {
                            ui.add_space(6.0);
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(paragraph.as_ref())
                                    .size(11.5)
                                    .line_height(Some(17.0))
                                    .color(ui_theme::TEXT_SECONDARY),
                            )
                            .wrap(),
                        );
                    }
                });
            });
        })
        .response
        .rect
}

pub(super) fn settings_current_version_card(
    ui: &mut egui::Ui,
    status: &str,
    update_status: &UpdateStatus,
) -> Rect {
    let title = tr(ui, "Current version");
    let (status_icon, status_color) = update_status_indicator(update_status);
    let card_width = ui.available_width();
    let card_height: f32 = 92.0;
    let (card, _) = ui.allocate_exact_size(egui::vec2(card_width, card_height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(card, 9, ui_theme::SURFACE_RAISED);
    painter.rect_stroke(
        card,
        9,
        Stroke::new(1.0, ui_theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let content = card.shrink2(egui::vec2(14.0, 12.0));
    let title_color = ui_theme::TEXT_MUTED;
    let status_text_color = ui_theme::TEXT_PRIMARY;
    let measured_status = painter.layout_no_wrap(
        status.to_owned(),
        FontId::proportional(11.0),
        status_text_color,
    );
    let desired_status_width = measured_status.size().x + 54.0;
    let maximum_status_width = (content.width() - 170.0).max(1.0);
    let status_width = desired_status_width.min(maximum_status_width);
    let status_galley = if desired_status_width <= maximum_status_width {
        measured_status
    } else {
        painter.layout(
            status.to_owned(),
            FontId::proportional(11.0),
            status_text_color,
            (status_width - 54.0).max(1.0),
        )
    };
    let status_height = (status_galley.size().y + 14.0)
        .max(32.0)
        .min(content.height());
    let status_rect = Rect::from_center_size(
        Pos2::new(content.right() - status_width / 2.0, content.center().y),
        egui::vec2(status_width, status_height),
    );
    let divider_x = status_rect.left() - 13.0;
    let divider = Rect::from_center_size(
        Pos2::new(divider_x, content.center().y),
        egui::vec2(1.0, 40.0),
    );
    painter.rect_filled(divider, 0.0, ui_theme::BORDER_SUBTLE);

    painter.rect_filled(
        status_rect,
        8,
        mix_color(ui_theme::SURFACE_RAISED, status_color, 0.12),
    );

    let title_galley =
        painter.layout_no_wrap(title.into_owned(), FontId::proportional(11.0), title_color);
    let version_galley = painter.layout_no_wrap(
        APP_VERSION_LABEL.to_owned(),
        FontId::proportional(23.0),
        Color32::WHITE,
    );
    let version_gap = 4.0;
    let version_text_height = title_galley.size().y + version_gap + version_galley.size().y;
    let version_area = Rect::from_min_max(
        content.min,
        Pos2::new((divider_x - 12.0).max(content.left()), content.bottom()),
    );
    let version_group_width = 28.0 + title_galley.size().x.max(version_galley.size().x);
    let version_group_left =
        (version_area.center().x - version_group_width / 2.0).max(version_area.left());
    let version_text_left = version_group_left + 28.0;
    let version_text_top = content.center().y - version_text_height / 2.0;
    painter.galley(
        Pos2::new(version_text_left, version_text_top),
        title_galley,
        title_color,
    );
    let version_top = version_text_top + version_text_height - version_galley.size().y;
    painter.galley(
        Pos2::new(version_text_left, version_top),
        version_galley,
        Color32::WHITE,
    );
    let version_icon = Rect::from_center_size(
        Pos2::new(version_group_left + 9.0, content.center().y),
        egui::vec2(18.0, 18.0),
    );
    ui_icon::paint(painter, version_icon, UiIcon::Info, SETTINGS_BLUE);

    let status_icon_rect = Rect::from_center_size(
        Pos2::new(status_rect.left() + 20.0, status_rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    ui_icon::paint(painter, status_icon_rect, status_icon, status_color);
    painter.galley(
        Pos2::new(
            status_icon_rect.right() + 10.0,
            status_rect.center().y - status_galley.size().y / 2.0,
        ),
        status_galley,
        status_text_color,
    );
    card
}

pub(super) fn update_status_indicator(status: &UpdateStatus) -> (UiIcon, Color32) {
    match status {
        UpdateStatus::Idle => (UiIcon::Clock, ui_theme::TEXT_MUTED),
        UpdateStatus::Checking => (UiIcon::Loader, TRANSITION_AMBER),
        UpdateStatus::UpToDate => (UiIcon::CheckCircle, ACTIVE_GREEN),
        UpdateStatus::Available(_) => (UiIcon::Download, SETTINGS_BLUE),
        UpdateStatus::Downloading(_) | UpdateStatus::Installing => (UiIcon::Loader, SETTINGS_BLUE),
        UpdateStatus::Failed(_) => (UiIcon::Error, LIVE_RED),
    }
}

pub(super) fn language_selector_text(locale: Locale) -> String {
    format!("      {}", locale.native_name())
}

pub(super) fn language_flag_rect(container: Rect) -> Rect {
    Rect::from_center_size(
        Pos2::new(container.left() + 17.0, container.center().y),
        egui::vec2(18.0, 12.0),
    )
}

pub(super) fn paint_language_flag(painter: &egui::Painter, rect: Rect, locale: Locale) {
    painter.rect_filled(rect, 1.5, Color32::WHITE);
    match locale {
        Locale::English => {
            let stripe_height = rect.height() / 7.0;
            for stripe in [0, 2, 4, 6] {
                let stripe_rect = Rect::from_min_max(
                    Pos2::new(rect.left(), rect.top() + stripe as f32 * stripe_height),
                    Pos2::new(
                        rect.right(),
                        rect.top() + (stripe + 1) as f32 * stripe_height,
                    ),
                );
                painter.rect_filled(stripe_rect, 0.0, Color32::from_rgb(190, 45, 55));
            }
            painter.rect_filled(
                Rect::from_min_max(
                    rect.min,
                    Pos2::new(
                        rect.left() + rect.width() * 0.44,
                        rect.top() + stripe_height * 4.0,
                    ),
                ),
                0.0,
                Color32::from_rgb(48, 70, 130),
            );
        }
        Locale::French => {
            let third = rect.width() / 3.0;
            painter.rect_filled(
                Rect::from_min_max(rect.min, Pos2::new(rect.left() + third, rect.bottom())),
                0.0,
                Color32::from_rgb(35, 73, 155),
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(rect.left() + third * 2.0, rect.top()), rect.max),
                0.0,
                Color32::from_rgb(220, 45, 55),
            );
        }
        Locale::Spanish => {
            let band_height = rect.height() * 0.25;
            let red = Color32::from_rgb(190, 35, 45);
            painter.rect_filled(
                Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + band_height)),
                0.0,
                red,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.bottom() - band_height),
                    rect.max,
                ),
                0.0,
                red,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.top() + band_height),
                    Pos2::new(rect.right(), rect.bottom() - band_height),
                ),
                0.0,
                Color32::from_rgb(245, 198, 40),
            );
        }
    }
    painter.rect_stroke(
        rect,
        1.5,
        Stroke::new(0.75, Color32::from_black_alpha(110)),
        StrokeKind::Inside,
    );
}

pub(super) fn settings_save_error_callout(ui: &mut egui::Ui, message: &str) {
    let title = tr(ui, "Could not save settings");
    egui::Frame::new()
        .fill(Color32::from_rgb(67, 31, 38))
        .stroke(Stroke::new(1.0, LIVE_RED.gamma_multiply(0.72)))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                ui_icon::paint(ui.painter(), icon_rect, UiIcon::Error, LIVE_RED);
                ui.add_space(7.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(title.as_ref())
                            .size(12.0)
                            .strong()
                            .color(LIVE_RED),
                    );
                    ui.add_space(3.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(message)
                                .size(11.0)
                                .color(Color32::from_rgb(245, 221, 225)),
                        )
                        .wrap(),
                    );
                });
            });
        });
}

pub(super) fn settings_section_gap(ui: &mut egui::Ui) {
    ui.add_space(24.0);
}

pub(super) fn settings_content_width(available: f32) -> f32 {
    (available - 28.0).clamp(1.0, SETTINGS_CONTENT_WIDTH)
}

pub(super) fn settings_group_label(ui: &mut egui::Ui, label: &str) {
    let label = tr(ui, label);
    ui.label(
        RichText::new(label.as_ref())
            .size(11.5)
            .strong()
            .color(ui_theme::TEXT_SECONDARY),
    );
    ui.add_space(3.0);
}

pub(super) fn settings_result_text(ui: &mut egui::Ui, text: &str) -> Rect {
    let text = tr(ui, text);
    ui.add(
        egui::Label::new(
            RichText::new(text.as_ref())
                .size(10.0)
                .color(ui_theme::TEXT_MUTED),
        )
        .wrap(),
    )
    .rect
}

pub(super) fn update_channel_label(
    channel: UpdateChannel,
    locale: Locale,
) -> std::borrow::Cow<'static, str> {
    localized_text(
        locale,
        match channel {
            UpdateChannel::Stable => "Stable releases",
            UpdateChannel::Beta => "Beta",
        },
    )
}

pub(super) fn update_status_text(locale: Locale, status: &UpdateStatus) -> String {
    match status {
        UpdateStatus::Idle => localized_text(locale, "Waiting for an update check.").into_owned(),
        UpdateStatus::Checking => localized_text(locale, "Checking for updates…").into_owned(),
        UpdateStatus::UpToDate => localized_text(locale, "StageSwap is up to date.").into_owned(),
        UpdateStatus::Available(release) => {
            let template = if release.prerelease {
                "StageSwap {0} Beta is available on the selected channel."
            } else {
                "StageSwap {0} is available on the selected channel."
            };
            format_text(locale, template, &[&format!("v{}", release.version)])
        }
        UpdateStatus::Downloading(release) => format_text(
            locale,
            "Downloading and verifying StageSwap {0}…",
            &[&format!("v{}", release.version)],
        ),
        UpdateStatus::Installing => localized_text(
            locale,
            "The verified update is starting. StageSwap will restart shortly…",
        )
        .into_owned(),
        UpdateStatus::Failed(error) => format_text(locale, "Update failed: {0}", &[error]),
    }
}

pub(super) fn match_strictness_explanation(value: f64) -> MatchStrictnessExplanation {
    let percentage = (value * 100.0).round();
    if percentage >= 99.0 {
        MatchStrictnessExplanation {
            level: "Very high",
            effect: "Small visual changes may count as media.",
        }
    } else if percentage >= 97.0 {
        MatchStrictnessExplanation {
            level: "High",
            effect: "Minor rendering or cursor differences are ignored.",
        }
    } else if percentage >= 90.0 {
        MatchStrictnessExplanation {
            level: "Moderate",
            effect: "Larger visual differences may still count as no media.",
        }
    } else {
        MatchStrictnessExplanation {
            level: "Low",
            effect: "Significant changes may still count as no media.",
        }
    }
}

pub(super) fn settings_toggle_row(
    ui: &mut egui::Ui,
    value: &mut bool,
    title: &str,
    description: &str,
) {
    settings_toggle_row_with_description(ui, value, title, description, true)
}

pub(super) fn settings_toggle_row_without_separator(
    ui: &mut egui::Ui,
    value: &mut bool,
    title: &str,
    description: &str,
) {
    settings_toggle_row_with_description(ui, value, title, description, false)
}

pub(super) fn settings_toggle_row_with_description(
    ui: &mut egui::Ui,
    value: &mut bool,
    title: &str,
    description: &str,
    separator: bool,
) {
    const TEXT_CONTROL_GAP: f32 = 12.0;
    const CONTROL_WIDTH: f32 = 42.0;
    const VERTICAL_PADDING: f32 = 7.0;
    const TEXT_GAP: f32 = 3.0;
    let stable_title = title.to_owned();
    let title = tr(ui, title);
    let description = tr(ui, description);
    let width = ui.available_width();
    let text_width = (width - CONTROL_WIDTH - TEXT_CONTROL_GAP).max(80.0);
    let title_color = ui_theme::TEXT_PRIMARY;
    let description_color = ui_theme::TEXT_SUBTLE;
    let title_galley =
        ui.painter()
            .layout_no_wrap(title.to_string(), FontId::proportional(12.5), title_color);
    let description_galley = ui.painter().layout(
        description.to_string(),
        FontId::proportional(10.0),
        description_color,
        text_width,
    );
    let title_height = title_galley.size().y;
    let description_height = TEXT_GAP + description_galley.size().y;
    let text_height = title_height + description_height;
    let row_height = (text_height + VERTICAL_PADDING * 2.0).max(52.0);
    let (row, response) = ui.allocate_exact_size(egui::vec2(width, row_height), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let amount = ui.ctx().animate_bool_with_time(
        ui.make_persistent_id(("settings-switch", stable_title)),
        *value,
        0.12,
    );
    if amount > 0.0 && amount < 1.0 {
        ui.ctx().request_repaint();
    }
    let text_top = row.center().y - text_height / 2.0;
    let title_position = Pos2::new(row.left(), text_top);
    ui.painter()
        .galley(title_position, title_galley, title_color);
    let description_position = Pos2::new(row.left(), text_top + title_height + TEXT_GAP);
    ui.painter()
        .galley(description_position, description_galley, description_color);
    let (off, on) = settings_switch_colors();
    let track = settings_switch_geometry(row);
    ui.painter()
        .rect_filled(track, track.height() / 2.0, mix_color(off, on, amount));
    ui.painter().rect_stroke(
        track,
        track.height() / 2.0,
        Stroke::new(1.0, mix_color(ui_theme::BORDER, on, amount)),
        StrokeKind::Inside,
    );
    let thumb_x = egui::lerp((track.left() + 11.0)..=(track.right() - 11.0), amount);
    ui.painter()
        .circle_filled(Pos2::new(thumb_x, track.center().y), 8.0, Color32::WHITE);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *value,
            title.as_ref(),
        )
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if separator {
        ui.separator();
    }
}

pub(super) fn reference_control_geometry(available: f32, gap: f32) -> ReferenceControlGeometry {
    ReferenceControlGeometry {
        action_width: ((available - gap) / 2.0).max(80.0),
        slider_width: available.max(1.0),
    }
}

pub(super) fn match_strictness_slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    width: f32,
) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = width.max(1.0);
        ui.add(egui::Slider::new(value, 0.50..=1.0).show_value(false))
    })
    .inner
}

pub(super) fn selector_utility_geometry(available: f32, gap: f32) -> SelectorUtilityGeometry {
    let action_width = 32.0;
    SelectorUtilityGeometry {
        selector_width: (available - gap - action_width).max(80.0),
        action_width,
    }
}

pub(super) fn settings_switch_geometry(row: Rect) -> Rect {
    Rect::from_center_size(
        Pos2::new(row.right() - 21.0, row.center().y),
        egui::vec2(42.0, 22.0),
    )
}

pub(super) fn settings_control_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    let title = tr(ui, title);
    let description = tr(ui, description);
    let width = ui.available_width();
    if width <= 560.0 {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title.as_ref())
                    .size(13.0)
                    .color(ui_theme::TEXT_PRIMARY),
            );
            if !description.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(description.as_ref())
                            .size(10.5)
                            .color(ui_theme::TEXT_SUBTLE),
                    )
                    .wrap(),
                );
            }
            ui.add_space(5.0);
            add_control(ui);
        });
        ui.add_space(8.0);
        ui.separator();
        return;
    }

    let control_width = 300.0_f32.min((width * 0.46).max(120.0));
    let label_width = (width - control_width - 12.0).max(100.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 54.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(label_width, 44.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(title.as_ref())
                            .size(13.0)
                            .color(ui_theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(description.as_ref())
                            .size(10.5)
                            .color(ui_theme::TEXT_SUBTLE),
                    );
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(control_width, 44.0),
                egui::Layout::right_to_left(egui::Align::Center),
                add_control,
            );
        },
    );
    ui.separator();
}

pub(super) fn settings_action_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    add_action: impl FnOnce(&mut egui::Ui),
) {
    settings_control_row(ui, title, description, add_action);
}

pub(super) fn settings_fixed_control_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    control_width: f32,
    add_control: impl FnOnce(&mut egui::Ui),
) -> Rect {
    let stable_title = title.to_owned();
    let title = tr(ui, title);
    let description = tr(ui, description);
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, 54.0), Sense::hover());
    let control_size = egui::vec2(control_width.min(width).max(1.0), 32.0);
    let control_rect = Rect::from_center_size(
        Pos2::new(row.right() - control_size.x / 2.0, row.center().y),
        control_size,
    );
    let label_rect = Rect::from_min_max(
        row.min,
        Pos2::new((control_rect.left() - 12.0).max(row.left()), row.bottom()),
    );
    let mut label_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("settings-fixed-control-label", stable_title.clone()))
            .max_rect(label_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    label_ui.add_space(4.0);
    label_ui.label(
        RichText::new(title.as_ref())
            .size(13.0)
            .color(ui_theme::TEXT_PRIMARY),
    );
    label_ui.label(
        RichText::new(description.as_ref())
            .size(10.5)
            .color(ui_theme::TEXT_SUBTLE),
    );

    let mut control_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("settings-fixed-control", stable_title))
            .max_rect(control_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add_control(&mut control_ui);
    ui.separator();
    control_rect
}

pub(super) fn settings_single_button_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    button_label: &str,
    button_width: f32,
) -> egui::Response {
    let stable_title = title.to_owned();
    let title = tr(ui, title);
    let description = tr(ui, description);
    let button_label = tr(ui, button_label);
    let width = ui.available_width();
    if width <= 560.0 {
        let response = ui
            .vertical(|ui| {
                ui.label(
                    RichText::new(title.as_ref())
                        .size(13.0)
                        .color(ui_theme::TEXT_PRIMARY),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(description.as_ref())
                            .size(10.5)
                            .color(ui_theme::TEXT_SUBTLE),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.add_sized(
                    egui::vec2(ui.available_width(), 32.0),
                    egui::Button::new(button_label.as_ref()),
                )
            })
            .inner;
        ui.add_space(8.0);
        ui.separator();
        return response;
    }

    let button_size = egui::vec2(button_width.min(width).max(1.0), 32.0);
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, 54.0), Sense::hover());
    let button_rect = Rect::from_center_size(
        Pos2::new(row.right() - button_size.x / 2.0, row.center().y),
        button_size,
    );
    let label_rect = Rect::from_min_max(
        row.min,
        Pos2::new((button_rect.left() - 12.0).max(row.left()), row.bottom()),
    );
    let mut label_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("settings-single-button-label", stable_title))
            .max_rect(label_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    label_ui.add_space(4.0);
    label_ui.label(
        RichText::new(title.as_ref())
            .size(13.0)
            .color(ui_theme::TEXT_PRIMARY),
    );
    label_ui.add(
        egui::Label::new(
            RichText::new(description.as_ref())
                .size(10.5)
                .color(ui_theme::TEXT_SUBTLE),
        )
        .wrap(),
    );

    let response = ui.put(button_rect, egui::Button::new(button_label.as_ref()));
    ui.separator();
    response
}

pub(super) fn settings_info_row(ui: &mut egui::Ui, title: &str, value: &str) {
    settings_control_row(ui, title, "", |ui| {
        let value = tr(ui, value);
        ui.add(
            egui::Label::new(
                RichText::new(value.as_ref())
                    .monospace()
                    .size(10.5)
                    .color(ui_theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    });
}

pub(super) fn settings_device_status(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &str,
    state: DeviceState,
) {
    let (status_icon, color) = match state {
        DeviceState::Initializing => (UiIcon::Loader, TRANSITION_AMBER),
        DeviceState::Ready => (UiIcon::Check, ACTIVE_GREEN),
        DeviceState::Unavailable => (UiIcon::Unavailable, LIVE_RED),
        DeviceState::Failed => (UiIcon::Error, LIVE_RED),
    };
    settings_status_item(
        ui,
        icon,
        label,
        status_icon,
        friendly_device_state(state),
        color,
    );
}

pub(super) fn settings_detection_status(ui: &mut egui::Ui, state: DetectionState) {
    let (icon, status, color) = settings_detection_style(state);
    settings_status_item(ui, UiIcon::Target, "Media detection", icon, status, color);
}

pub(super) fn settings_detection_style(state: DetectionState) -> (UiIcon, &'static str, Color32) {
    match state {
        DetectionState::Unknown => (UiIcon::Question, "Checking", TRANSITION_AMBER),
        DetectionState::Matching => (UiIcon::Check, "No media", ACTIVE_GREEN),
        DetectionState::NotMatching => (UiIcon::Error, "Media detected", TRANSITION_AMBER),
        DetectionState::ReferenceMissing => {
            (UiIcon::Unavailable, "Reference image missing", LIVE_RED)
        }
    }
}

pub(super) fn settings_status_item(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &str,
    status_icon: UiIcon,
    status: &str,
    color: Color32,
) {
    ui.horizontal(|ui| {
        icon_text(ui, icon, label, ui_theme::TEXT_SECONDARY, false);
        ui.add_space(3.0);
        icon_text(ui, status_icon, status, color, false);
    });
}
