use super::*;

impl SwitcherApp {
    pub(super) fn dashboard(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.snapshot();
        if let Some(kind) = self.previews.enlarged {
            self.render_enlarged_dashboard_preview(ui, &snapshot, kind);
            return;
        }
        ui.add_space(2.0);
        let available_width = ui.available_width();
        let preview_width = available_width * 0.70;
        let workspace_height = ui.available_height().max(0.0);
        ui.horizontal_top(|ui| {
            let previews = ui.allocate_ui_with_layout(
                egui::vec2(preview_width, workspace_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.preview_workspace(ui, &snapshot, preview_width, workspace_height),
            );

            ui.separator();
            let controls_width = ui.available_width();
            let controls = ui.allocate_ui_with_layout(
                egui::vec2(controls_width, workspace_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.controls_workspace(ui, &snapshot, controls_width, workspace_height),
            );
            debug_assert!(controls.inner.footer.is_positive());
            debug_assert!(controls.inner.body.sections.iter().all(Rect::is_positive));
            debug_assert!(previews.inner.grid.is_positive());
        });
    }

    pub(super) fn render_enlarged_dashboard_preview(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        kind: PreviewKind,
    ) {
        let available = ui.available_size().max(egui::vec2(1.0, 1.0));
        let content_height =
            (available.y - CINEMA_PEEK_CAPTION_GAP - CINEMA_PEEK_CAPTION_HEIGHT).max(1.0);
        let fitted_width = available.x.min(content_height * WINDOW_ASPECT_RATIO);
        let preview_width = fitted_width * CINEMA_PEEK_SCALE;
        let preview_height = preview_width / WINDOW_ASPECT_RATIO;
        let content_height = preview_height + CINEMA_PEEK_CAPTION_GAP + CINEMA_PEEK_CAPTION_HEIGHT;
        let top_margin = ((available.y - content_height) / 2.0).max(0.0);
        ui.allocate_ui_with_layout(
            available,
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(top_margin);
                self.preview(
                    ui,
                    kind,
                    kind.frame(snapshot),
                    [preview_width, preview_height],
                    snapshot.actual_output,
                    PreviewOptions::enlarged(kind, kind.pipeline_fps(snapshot)),
                );
                ui.add_space(CINEMA_PEEK_CAPTION_GAP);
                preview_caption(ui, kind);
            },
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if ui
            .ctx()
            .input(|input| input.pointer.any_click() || input.key_pressed(egui::Key::Escape))
        {
            self.previews.enlarged = None;
        }
    }

    pub(super) fn preview_workspace(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        width: f32,
        height: f32,
    ) -> PreviewGridLayout {
        const FOOTER_HEIGHT: f32 = 40.0;
        const FOOTER_GAP: f32 = 10.0;
        let app_icon_texture = self.app_icon_texture(ui.ctx());
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::bottom_up(egui::Align::Center),
            |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(width, FOOTER_HEIGHT),
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        app_title(ui, app_icon_texture);
                    },
                );
                ui.add_space(FOOTER_GAP);
                let body_height = ui.available_height().max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, body_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| self.preview_grid(ui, snapshot, width, body_height),
                )
                .inner
            },
        )
        .inner
    }

    pub(super) fn app_icon_texture(&mut self, context: &egui::Context) -> egui::TextureId {
        self.app_icon_texture
            .get_or_insert_with(|| {
                let icon = app_icon::load(Some(512)).expect("embedded app icon should decode");
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                );
                context.load_texture("app-icon", image, TextureOptions::LINEAR)
            })
            .id()
    }

    pub(super) fn preview_grid(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        width: f32,
        height: f32,
    ) -> PreviewGridLayout {
        let column_gap = ui.spacing().item_spacing.x;
        let row_gap = ui.spacing().item_spacing.y;
        let label_height = ui.text_style_height(&egui::TextStyle::Small);
        let cell_width = ((width - column_gap) / 2.0).max(48.0);
        let row_budget = ((height - row_gap) / 2.0).max(48.0);
        let maximum_preview_height = (row_budget - label_height - 24.0).max(24.0);
        let rendered_width = cell_width
            .min(maximum_preview_height * WINDOW_ASPECT_RATIO)
            .max(24.0 * WINDOW_ASPECT_RATIO);
        let rendered_height = rendered_width / WINDOW_ASPECT_RATIO;
        let cell_height = rendered_height + 8.0 + label_height + 16.0;
        let grid_height = cell_height * 2.0 + row_gap;
        ui.add_space(((height - grid_height) / 2.0).max(0.0));

        let mut cells = [Rect::NOTHING; 4];
        let mut cell_index = 0;
        for row in [
            [
                (PreviewKind::Webcam, snapshot.previews.webcam.as_ref()),
                (PreviewKind::Screen, snapshot.previews.screen.as_ref()),
            ],
            [
                (PreviewKind::Reference, snapshot.previews.reference.as_ref()),
                (PreviewKind::Output, snapshot.previews.final_output.as_ref()),
            ],
        ] {
            ui.horizontal_top(|ui| {
                for (kind, frame) in row {
                    let cell = ui.allocate_ui_with_layout(
                        egui::vec2(cell_width, cell_height),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            self.preview_cell(
                                ui,
                                kind,
                                frame,
                                [rendered_width, rendered_height],
                                snapshot.actual_output,
                                kind.pipeline_fps(snapshot),
                            );
                        },
                    );
                    cells[cell_index] = cell.response.rect;
                    cell_index += 1;
                }
            });
        }
        PreviewGridLayout {
            grid: cells[0].union(cells[3]),
        }
    }

    pub(super) fn controls_workspace(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        width: f32,
        height: f32,
    ) -> ControlsWorkspaceLayout {
        const FOOTER_HEIGHT: f32 = 40.0;
        const FOOTER_GAP: f32 = 10.0;
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::bottom_up(egui::Align::Center),
            |ui| {
                let settings = icon_button(
                    ui,
                    UiIcon::Settings,
                    "Settings",
                    egui::vec2(width, FOOTER_HEIGHT),
                    false,
                    true,
                );
                if settings.clicked() {
                    self.open_settings();
                }
                ui.add_space(FOOTER_GAP);
                let body_height = ui.available_height().max(80.0);
                let body = ui.allocate_ui_with_layout(
                    egui::vec2(width, body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_size(egui::vec2(width, body_height));
                        self.controls_body(ui, snapshot)
                    },
                );
                ControlsWorkspaceLayout {
                    body: body.inner,
                    footer: settings.rect,
                }
            },
        )
        .inner
    }

    pub(super) fn controls_body(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
    ) -> DashboardControlsLayout {
        let health_heading = controls_section_heading(ui, UiIcon::Check, "Components health");
        let health_indicators = [
            health_state_group(ui, UiIcon::Camera, "Webcam", snapshot.webcam_state),
            health_state_group(
                ui,
                UiIcon::Monitor,
                "Secondary screen",
                snapshot.screen_state,
            ),
            health_state_group(
                ui,
                UiIcon::Broadcast,
                "Zoom output",
                snapshot.virtual_camera_state,
            ),
            detection_state_group(ui, snapshot.detection),
            screen_mix_group(ui, snapshot.transition.screen_mix),
        ];
        let health_section = health_heading.union(health_indicators[4]);
        let first_divider = controls_section_divider(ui);

        let main_heading = controls_section_heading(ui, UiIcon::Route, "Main controls");
        let automation_running =
            matches!(snapshot.run_state, RunState::Running | RunState::Starting);
        let (run_icon, run_label, run_accent) = if automation_running {
            (UiIcon::Stop, "Stop automatic switching", LIVE_RED)
        } else {
            (UiIcon::Play, "Start automatic switching", ACTIVE_GREEN)
        };
        let automation = accent_icon_button(
            ui,
            run_icon,
            run_label,
            egui::vec2(ui.available_width(), 36.0),
            run_accent,
        );
        if automation.clicked() {
            if automation_running {
                self.send(Command::Stop);
            } else {
                self.send(Command::Start);
            }
        }

        ui.add_space(8.0);
        let gap = 4.0;
        let row_height = 30.0;
        let automatic_width = 72.0;
        let icon_width = row_height;
        let heading_width =
            (ui.available_width() - automatic_width - icon_width * 3.0 - gap * 4.0).max(72.0);
        let output_mode = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.horizontal(|ui| {
                    indicator_heading(
                        ui,
                        UiIcon::Route,
                        "Output mode",
                        None,
                        heading_width,
                        row_height,
                    );
                    if icon_button(
                        ui,
                        UiIcon::Robot,
                        "Auto",
                        egui::vec2(automatic_width, row_height),
                        snapshot.mode == OutputMode::Automatic,
                        false,
                    )
                    .clicked()
                    {
                        self.set_mode(OutputMode::Automatic);
                    }
                    if icon_button(
                        ui,
                        UiIcon::Camera,
                        "",
                        egui::vec2(icon_width, row_height),
                        snapshot.mode == OutputMode::ForceCamera,
                        false,
                    )
                    .on_hover_text(localized_text(self.locale(), "Webcam"))
                    .clicked()
                    {
                        self.set_mode(OutputMode::ForceCamera);
                    }
                    if icon_button(
                        ui,
                        UiIcon::Monitor,
                        "",
                        egui::vec2(icon_width, row_height),
                        snapshot.mode == OutputMode::ForceScreen,
                        false,
                    )
                    .on_hover_text(localized_text(self.locale(), "Secondary screen"))
                    .clicked()
                    {
                        self.set_mode(OutputMode::ForceScreen);
                    }
                    if icon_button(
                        ui,
                        UiIcon::Layers,
                        "",
                        egui::vec2(icon_width, row_height),
                        snapshot.mode == OutputMode::ForcePip,
                        false,
                    )
                    .on_hover_text(localized_text(self.locale(), "Picture-in-picture"))
                    .clicked()
                    {
                        self.set_mode(OutputMode::ForcePip);
                    }
                })
                .response
                .rect
            })
            .inner;
        let main_section = main_heading.union(output_mode);
        let second_divider = controls_section_divider(ui);

        let other_heading = controls_section_heading(ui, UiIcon::Wrench, "Other");
        let capture = icon_button(
            ui,
            UiIcon::Capture,
            "Capture reference image",
            egui::vec2(ui.available_width(), 32.0),
            false,
            false,
        );
        if capture.clicked() {
            self.begin_reference_capture();
        }
        let rescan = icon_button(
            ui,
            UiIcon::Refresh,
            "Rescan screens",
            egui::vec2(ui.available_width(), 32.0),
            false,
            false,
        );
        if rescan.clicked() {
            self.send(Command::Rescan);
        }
        let other_actions = [capture.rect, rescan.rect];
        let other_section = other_heading.union(other_actions[1]);

        DashboardControlsLayout {
            sections: [health_section, main_section, other_section],
            section_headings: [health_heading, main_heading, other_heading],
            health_indicators,
            section_dividers: [first_divider, second_divider],
            other_actions,
        }
    }
}
