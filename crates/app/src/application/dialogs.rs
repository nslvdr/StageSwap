use super::*;

impl SwitcherApp {
    pub(super) fn open_dialog(&mut self, kind: AppDialogKind) {
        self.active_dialog = Some(ActiveDialog {
            kind,
            opened_at: Instant::now(),
            focus_safe_action: true,
        });
    }

    pub(super) fn dismiss_dialog(&mut self) {
        self.active_dialog = None;
    }

    pub(super) fn dialog_is(&self, kind: AppDialogKind) -> bool {
        self.active_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.kind == kind)
    }

    pub(super) fn begin_reference_capture(&mut self) {
        self.prepare_reference_capture();
        self.open_dialog(AppDialogKind::ReferenceCapture);
    }

    pub(super) fn prepare_reference_capture(&mut self) {
        if self.send_setup_reference_command(Command::DiscardReferenceCandidate) {
            self.setup_reference_capture = SetupReferenceCaptureState::PreparingCandidate {
                started_at: Instant::now(),
            };
        }
    }

    pub(super) fn capture_reference_candidate(&mut self) {
        let snapshot = self.snapshot();
        let next_state = SetupReferenceCaptureState::CapturingCandidate {
            started_at: Instant::now(),
            previous_candidate_sequence: snapshot
                .previews
                .reference_candidate
                .as_ref()
                .map(|frame| frame.sequence),
        };
        if self.send_setup_reference_command(Command::CaptureReferenceCandidate) {
            self.setup_reference_capture = next_state;
        }
    }

    pub(super) fn cancel_reference_capture(&mut self) {
        if matches!(
            self.setup_reference_capture,
            SetupReferenceCaptureState::SavingCandidate { .. }
        ) {
            return;
        }
        if self.send_setup_reference_command(Command::DiscardReferenceCandidate) {
            self.setup_reference_capture = SetupReferenceCaptureState::Idle;
            self.dismiss_dialog();
        }
    }

    pub(super) fn confirm_reference_candidate(&mut self) {
        let snapshot = self.snapshot();
        let next_state = SetupReferenceCaptureState::SavingCandidate {
            started_at: Instant::now(),
            previous_reference_sequence: snapshot
                .previews
                .reference
                .as_ref()
                .map(|frame| frame.sequence),
        };
        if self.send_setup_reference_command(Command::ConfirmReferenceCandidate) {
            self.setup_reference_capture = next_state;
        }
    }

    pub(super) fn dialog(&mut self, context: &egui::Context) {
        let Some(active) = self.active_dialog else {
            return;
        };
        let reference_snapshot = if active.kind == AppDialogKind::ReferenceCapture {
            let snapshot = self.snapshot();
            self.update_setup_reference_capture(&snapshot);
            if matches!(
                self.setup_reference_capture,
                SetupReferenceCaptureState::Confirmed
            ) {
                let advance_setup = self
                    .setup_session
                    .is_some_and(|session| session.step == SetupStep::Reference);
                self.dismiss_dialog();
                if advance_setup {
                    self.apply_setup_action(SetupAction::Next);
                }
                return;
            }
            Some(snapshot)
        } else {
            None
        };
        let reference_textures = if active.kind == AppDialogKind::ReferenceCapture {
            Some(self.setup_example_textures(context))
        } else {
            None
        };
        let progress = animation_progress(Some(active.opened_at), DIALOG_ENTRANCE_DURATION);
        if progress < 1.0 {
            context.request_repaint();
        }
        let available_width = context.content_rect().width();
        let width = active
            .kind
            .preferred_width()
            .min((available_width - 32.0).max(280.0));
        let frame = egui::Frame::new()
            .fill(ui_theme::SURFACE_DEEP)
            .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
            .corner_radius(12)
            .inner_margin(24)
            .shadow(egui::Shadow {
                offset: [0, 8],
                blur: 28,
                spread: 2,
                color: Color32::from_black_alpha(150),
            });
        let area = egui::Modal::default_area(egui::Id::new(("stageswap-dialog", active.kind)))
            .anchor(
                Align2::CENTER_CENTER,
                egui::vec2(0.0, 8.0 * (1.0 - progress)),
            );
        let status = self.admin_profile_status;
        let response = egui::Modal::new(egui::Id::new(("stageswap-dialog", active.kind)))
            .area(area)
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha((145.0 * progress) as u8))
            .show(context, |ui| {
                ui.set_width(width);
                ui.set_opacity(0.72 + progress * 0.28);
                if active.kind == AppDialogKind::ReferenceCapture {
                    self.reference_capture_dialog_content(
                        ui,
                        reference_snapshot
                            .as_ref()
                            .expect("reference snapshot exists"),
                        reference_textures
                            .as_ref()
                            .expect("reference textures exist"),
                        active.focus_safe_action,
                    )
                } else {
                    dialog_content(ui, active.kind, status, active.focus_safe_action)
                }
            });
        if let Some(dialog) = self
            .active_dialog
            .as_mut()
            .filter(|dialog| dialog.opened_at == active.opened_at)
        {
            dialog.focus_safe_action = false;
        }

        let action = response
            .inner
            .or_else(|| response.should_close().then_some(DialogAction::Dismiss));
        match action {
            None => {}
            Some(DialogAction::Dismiss) => {
                if active.kind == AppDialogKind::ReferenceCapture {
                    self.cancel_reference_capture();
                } else {
                    self.dismiss_dialog();
                }
            }
            Some(DialogAction::CancelReferenceCapture) => self.cancel_reference_capture(),
            Some(DialogAction::RetakeReferenceCapture) => {
                self.prepare_reference_capture();
            }
            Some(DialogAction::ConfirmReferenceCandidate) => {
                self.confirm_reference_candidate();
            }
            Some(DialogAction::Exit) => {
                self.exit_requested = true;
                self.dismiss_dialog();
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(DialogAction::ClearLogs) => self.confirm_log_clear(),
            Some(DialogAction::SaveAdminBaseline) => self.save_admin_baseline(),
            Some(DialogAction::ReplaceAdminBaseline) => {
                self.open_dialog(AppDialogKind::ReplaceAdminBaseline);
            }
            Some(DialogAction::LoadAdminConfig) => {
                if active.kind == AppDialogKind::LoadAdminConfig {
                    self.load_admin_config();
                } else {
                    self.open_dialog(AppDialogKind::LoadAdminConfig);
                }
            }
            Some(DialogAction::RemoveAdminBaseline) => {
                if active.kind == AppDialogKind::RemoveAdminBaseline {
                    self.remove_admin_baseline();
                } else {
                    self.open_dialog(AppDialogKind::RemoveAdminBaseline);
                }
            }
            Some(DialogAction::SetAdminAutoRestore(enabled)) => {
                self.set_admin_auto_restore(enabled);
            }
        }
    }

    pub(super) fn reference_capture_dialog_content(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        textures: &SetupExampleTextures,
        focus_safe_action: bool,
    ) -> Option<DialogAction> {
        dialog_header(ui, AppDialogKind::ReferenceCapture);
        ui.add_space(12.0);
        ui.label(
            RichText::new(tr(
                ui,
                "Make sure this image is the screen JW Library shows when no media is playing.",
            ))
            .size(14.0)
            .line_height(Some(21.0))
            .color(ui_theme::TEXT_SECONDARY),
        );
        ui.add_space(16.0);

        let stacked = ui.available_width() < REFERENCE_DIALOG_STACK_BREAKPOINT;
        if stacked {
            self.reference_capture_candidate_surface(ui, snapshot, ui.available_width());
            ui.add_space(12.0);
            compact_reference_example_card(ui, textures, ui.available_width());
        } else {
            let rail_width = 220.0;
            let candidate_width =
                (ui.available_width() - SETUP_REFERENCE_COLUMN_GAP - rail_width).max(1.0);
            ui.spacing_mut().item_spacing.x = SETUP_REFERENCE_COLUMN_GAP;
            let row_top = ui.available_rect_before_wrap().top();
            let mut candidate_preview = Rect::NOTHING;
            ui.horizontal_top(|ui| {
                allocate_reference_dialog_column(ui, candidate_width, |ui| {
                    ui.set_width(candidate_width);
                    candidate_preview =
                        self.reference_capture_candidate_surface(ui, snapshot, candidate_width);
                });
                let rail_height =
                    (candidate_preview.bottom() - row_top).max(SETUP_REFERENCE_CARD_HEIGHT);
                allocate_reference_dialog_sized_column(ui, rail_width, rail_height, |ui| {
                    ui.set_width(rail_width);
                    ui.set_height(rail_height);
                    ui.add_space(reference_dialog_example_top_offset(
                        row_top,
                        candidate_preview,
                    ));
                    self.setup_reference_example_rail(ui, textures, rail_width);
                });
            });
        }

        match self.setup_reference_capture {
            SetupReferenceCaptureState::CaptureFailed => {
                ui.add_space(10.0);
                setup_compact_state(
                    ui,
                    UiIcon::Error,
                    "StageSwap couldn’t capture the screen. Check the screen preview and try again.",
                    LIVE_RED,
                );
            }
            SetupReferenceCaptureState::SaveFailed { .. } => {
                ui.add_space(10.0);
                setup_compact_state(
                    ui,
                    UiIcon::Error,
                    "StageSwap couldn’t save this reference. Try again or retake the image.",
                    LIVE_RED,
                );
            }
            _ => {}
        }
        ui.add_space(18.0);

        match self.setup_reference_capture {
            SetupReferenceCaptureState::CapturingCandidate { .. }
            | SetupReferenceCaptureState::PreparingCandidate { .. }
            | SetupReferenceCaptureState::Idle => {
                reference_dialog_actions(ui, focus_safe_action, false, None)
            }
            SetupReferenceCaptureState::Review { .. } => reference_dialog_actions(
                ui,
                focus_safe_action,
                true,
                Some(("Use this image", DialogAction::ConfirmReferenceCandidate)),
            ),
            SetupReferenceCaptureState::SavingCandidate { .. } => {
                setup_pending_capture_button(
                    ui,
                    "Saving reference…",
                    egui::vec2(ui.available_width(), 40.0),
                );
                None
            }
            SetupReferenceCaptureState::CaptureFailed => reference_dialog_actions(
                ui,
                focus_safe_action,
                false,
                Some(("Try again", DialogAction::RetakeReferenceCapture)),
            ),
            SetupReferenceCaptureState::SaveFailed { .. } => reference_dialog_actions(
                ui,
                focus_safe_action,
                true,
                Some(("Try again", DialogAction::ConfirmReferenceCandidate)),
            ),
            SetupReferenceCaptureState::Confirmed => None,
        }
    }

    pub(super) fn reference_capture_candidate_surface(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        width: f32,
    ) -> Rect {
        let candidate_visible = matches!(
            self.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
                | SetupReferenceCaptureState::SavingCandidate { .. }
                | SetupReferenceCaptureState::SaveFailed { .. }
        );
        let tone = if candidate_visible {
            TRANSITION_AMBER
        } else {
            SETTINGS_BLUE
        };
        ui.set_width(width);
        ui.horizontal(|ui| {
            setup_control_label(ui, UiIcon::Capture, "Your captured image", tone);
            if candidate_visible {
                setup_reference_state_badge(ui, "TO CONFIRM", tone);
            }
        });
        ui.add_space(6.0);
        let preview_height = (width / WINDOW_ASPECT_RATIO).min(260.0);
        let preview_rect = self.preview(
            ui,
            PreviewKind::Screen,
            candidate_visible
                .then_some(snapshot.previews.reference_candidate.as_ref())
                .flatten(),
            [width, preview_height],
            snapshot.actual_output,
            PreviewOptions::settings(
                if matches!(
                    self.setup_reference_capture,
                    SetupReferenceCaptureState::CapturingCandidate { .. }
                ) {
                    "Capturing the current frame…"
                } else {
                    "No captured image available for review."
                },
            ),
        );
        ui.painter()
            .rect_stroke(preview_rect, 8, Stroke::new(3.0, tone), StrokeKind::Inside);
        let flash_alpha = match self.setup_reference_capture {
            SetupReferenceCaptureState::Review { captured_at } => setup_reference_flash_alpha(
                Instant::now().saturating_duration_since(captured_at),
                self.setup_animations_enabled,
            ),
            _ => 0.0,
        };
        if flash_alpha > 0.0 {
            ui.ctx().request_repaint();
            ui.painter().rect_filled(
                preview_rect.shrink(3.0),
                6,
                SETUP_SIGNAL_WHITE.gamma_multiply(flash_alpha * 0.42),
            );
        }
        preview_rect
    }
}

pub(super) fn allocate_reference_dialog_column(
    ui: &mut egui::Ui,
    width: f32,
    content: impl FnOnce(&mut egui::Ui),
) -> Rect {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 1.0),
        egui::Layout::top_down(egui::Align::LEFT),
        content,
    )
    .response
    .rect
}

pub(super) fn allocate_reference_dialog_sized_column(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    content: impl FnOnce(&mut egui::Ui),
) -> Rect {
    allocate_reference_dialog_column(ui, width, |ui| {
        ui.set_height(height);
        content(ui);
    })
}

pub(super) fn reference_dialog_example_top_offset(row_top: f32, preview_rect: Rect) -> f32 {
    (preview_rect.center().y - row_top - SETUP_REFERENCE_CARD_HEIGHT / 2.0).max(0.0)
}

pub(super) fn compact_reference_example_card(
    ui: &mut egui::Ui,
    textures: &SetupExampleTextures,
    width: f32,
) {
    egui::Frame::new()
        .fill(SETUP_SIGNAL_DECK)
        .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            let content_width = (width - 24.0).max(1.0);
            ui.set_width(content_width);
            let thumbnail_width = content_width.min(132.0);
            ui.horizontal_top(|ui| {
                setup_reference_example_thumbnail(
                    ui,
                    textures.reference.id(),
                    egui::vec2(thumbnail_width, thumbnail_width / WINDOW_ASPECT_RATIO),
                );
                ui.vertical(|ui| {
                    setup_control_label(
                        ui,
                        UiIcon::Image,
                        "Example reference image",
                        SETTINGS_BLUE,
                    );
                    ui.add_space(6.0);
                    setup_capture_help(
                        ui,
                        "The screen JW Library shows when no media is playing should look like this example.",
                    );
                });
            });
        });
}

pub(super) fn reference_dialog_actions(
    ui: &mut egui::Ui,
    focus_safe_action: bool,
    show_retake: bool,
    primary: Option<(&'static str, DialogAction)>,
) -> Option<DialogAction> {
    let mut action = None;
    let action_count = 1 + usize::from(show_retake) + usize::from(primary.is_some());
    let stacked = ui.available_width() < 480.0;
    let button_width = if stacked {
        ui.available_width()
    } else {
        dialog_action_width(
            ui.available_width(),
            ui.spacing().item_spacing.x,
            action_count,
        )
    };
    let mut draw = |ui: &mut egui::Ui| {
        let cancel = dialog_button(
            ui,
            UiIcon::Trash,
            "Cancel",
            DialogButtonTone::DangerOutline,
            button_width,
        );
        if focus_safe_action {
            cancel.request_focus();
        }
        if cancel.clicked() {
            action = Some(DialogAction::CancelReferenceCapture);
        }
        if show_retake
            && dialog_button(
                ui,
                UiIcon::Refresh,
                "Retake",
                DialogButtonTone::Secondary,
                button_width,
            )
            .clicked()
        {
            action = Some(DialogAction::RetakeReferenceCapture);
        }
        if let Some((label, primary_action)) = primary
            && dialog_button(
                ui,
                if primary_action == DialogAction::ConfirmReferenceCandidate {
                    UiIcon::CheckCircle
                } else {
                    UiIcon::Refresh
                },
                label,
                DialogButtonTone::Primary,
                button_width,
            )
            .clicked()
        {
            action = Some(primary_action);
        }
    };
    if stacked {
        ui.vertical(|ui| draw(ui));
    } else {
        ui.horizontal(|ui| draw(ui));
    }
    action
}

pub(super) fn dialog_content(
    ui: &mut egui::Ui,
    kind: AppDialogKind,
    admin_status: Option<AdminProfileStatus>,
    focus_safe_action: bool,
) -> Option<DialogAction> {
    dialog_header(ui, kind);
    ui.add_space(18.0);
    if kind == AppDialogKind::Admin {
        return admin_dialog_content(ui, admin_status, focus_safe_action);
    }
    debug_assert_ne!(kind, AppDialogKind::ReferenceCapture);

    let body = match kind {
        AppDialogKind::Exit => {
            "StageSwap will stop publishing. The virtual camera stays installed and shows the StageSwap off screen until the app starts again."
        }
        AppDialogKind::ClearLogs => {
            "This permanently removes locally stored diagnostic logs. New logs will continue to be recorded."
        }
        AppDialogKind::ReferenceCapture => unreachable!(),
        AppDialogKind::ReplaceAdminBaseline => {
            "Replace the saved admin config with the setup currently shown in Settings?"
        }
        AppDialogKind::LoadAdminConfig => {
            "Replace the current settings and reference image with the saved admin config? Current session changes will be lost."
        }
        AppDialogKind::RemoveAdminBaseline => {
            "Auto-restore will turn off. Your current settings and reference image will stay unchanged."
        }
        AppDialogKind::Admin => unreachable!(),
    };
    ui.label(
        RichText::new(tr(ui, body))
            .size(14.0)
            .line_height(Some(21.0))
            .color(ui_theme::TEXT_SECONDARY),
    );
    ui.add_space(24.0);

    let (safe_icon, safe_label, primary_icon, primary_label, primary_action, primary_tone) =
        match kind {
            AppDialogKind::Exit => (
                UiIcon::Window,
                "Stay open",
                UiIcon::SignOut,
                "Exit StageSwap",
                DialogAction::Exit,
                DialogButtonTone::Danger,
            ),
            AppDialogKind::ClearLogs => (
                UiIcon::Folder,
                "Keep logs",
                UiIcon::Trash,
                "Clear logs",
                DialogAction::ClearLogs,
                DialogButtonTone::Danger,
            ),
            AppDialogKind::ReferenceCapture => unreachable!(),
            AppDialogKind::ReplaceAdminBaseline => (
                UiIcon::Check,
                "Keep saved configuration",
                UiIcon::Save,
                "Save current configuration",
                DialogAction::SaveAdminBaseline,
                DialogButtonTone::Primary,
            ),
            AppDialogKind::LoadAdminConfig => (
                UiIcon::Check,
                "Keep current config",
                UiIcon::Load,
                "Load saved configuration",
                DialogAction::LoadAdminConfig,
                DialogButtonTone::Primary,
            ),
            AppDialogKind::RemoveAdminBaseline => (
                UiIcon::Check,
                "Keep saved configuration",
                UiIcon::Trash,
                "Delete saved configuration",
                DialogAction::RemoveAdminBaseline,
                DialogButtonTone::Danger,
            ),
            AppDialogKind::Admin => unreachable!(),
        };
    let mut action = None;
    let button_width = dialog_action_width(ui.available_width(), ui.spacing().item_spacing.x, 2);
    ui.horizontal(|ui| {
        let safe = dialog_button(
            ui,
            safe_icon,
            safe_label,
            DialogButtonTone::Secondary,
            button_width,
        );
        if focus_safe_action {
            safe.request_focus();
        }
        if safe.clicked() {
            action = Some(DialogAction::Dismiss);
        }
        if dialog_button(ui, primary_icon, primary_label, primary_tone, button_width).clicked() {
            action = Some(primary_action);
        }
    });
    action
}

pub(super) fn dialog_header(ui: &mut egui::Ui, kind: AppDialogKind) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), Sense::hover());
        let accent = kind.accent();
        ui.painter().circle_filled(
            rect.center(),
            18.0,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 38),
        );
        ui_icon::paint(ui.painter(), rect.shrink(9.0), kind.icon(), accent);
        ui.add_space(4.0);
        ui.label(
            RichText::new(tr(ui, kind.title()))
                .size(19.0)
                .strong()
                .color(ui_theme::TEXT_PRIMARY),
        );
    });
}

pub(super) fn admin_dialog_content(
    ui: &mut egui::Ui,
    status: Option<AdminProfileStatus>,
    focus_safe_action: bool,
) -> Option<DialogAction> {
    ui.label(
        RichText::new(tr(
            ui,
            "Keep a protected local copy of the current settings and reference image for managed setups.",
        ))
        .size(14.0)
        .line_height(Some(21.0))
        .color(ui_theme::TEXT_SECONDARY),
    );
    ui.add_space(20.0);
    match status {
        None => {
            ui.label(
                RichText::new(tr(ui, "No admin config is saved."))
                    .size(13.0)
                    .color(ui_theme::TEXT_SUBTLE),
            );
            ui.add_space(22.0);
            let mut action = None;
            let button_width = ui.available_width();
            if dialog_button(
                ui,
                UiIcon::Save,
                "Save current configuration",
                DialogButtonTone::Primary,
                button_width,
            )
            .clicked()
            {
                action = Some(DialogAction::SaveAdminBaseline);
            }
            let back = dialog_button(
                ui,
                UiIcon::Back,
                "Back",
                DialogButtonTone::Secondary,
                button_width,
            );
            if focus_safe_action {
                back.request_focus();
            }
            if back.clicked() {
                action = Some(DialogAction::Dismiss);
            }
            action
        }
        Some(status) => {
            let reference = if status.reference_included {
                "Settings and reference image saved in the admin config"
            } else {
                "Admin config saved without a reference image"
            };
            icon_text(
                ui,
                UiIcon::Check,
                reference,
                Color32::from_rgb(116, 205, 157),
                false,
            );
            ui.add_space(14.0);
            let mut auto_restore = status.auto_restore_on_launch;
            settings_toggle_row_without_separator(
                ui,
                &mut auto_restore,
                "Auto-restore on launch",
                "Replace session changes with this admin config whenever StageSwap starts.",
            );
            if auto_restore != status.auto_restore_on_launch {
                return Some(DialogAction::SetAdminAutoRestore(auto_restore));
            }
            ui.add_space(18.0);
            let mut action = None;
            let button_width = ui.available_width();
            if dialog_button(
                ui,
                UiIcon::Save,
                "Save current configuration",
                DialogButtonTone::Primary,
                button_width,
            )
            .clicked()
            {
                action = Some(DialogAction::ReplaceAdminBaseline);
            }
            if dialog_button(
                ui,
                UiIcon::Load,
                "Load saved configuration",
                DialogButtonTone::Primary,
                button_width,
            )
            .clicked()
            {
                action = Some(DialogAction::LoadAdminConfig);
            }
            if dialog_button(
                ui,
                UiIcon::Trash,
                "Delete saved configuration",
                DialogButtonTone::Danger,
                button_width,
            )
            .clicked()
            {
                action = Some(DialogAction::RemoveAdminBaseline);
            }
            let back = dialog_button(
                ui,
                UiIcon::Back,
                "Back",
                DialogButtonTone::Secondary,
                button_width,
            );
            if focus_safe_action {
                back.request_focus();
            }
            if back.clicked() {
                action = Some(DialogAction::Dismiss);
            }
            action
        }
    }
}

pub(super) fn dialog_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &str,
    tone: DialogButtonTone,
    width: f32,
) -> egui::Response {
    let label = tr(ui, label);
    let (base_fill, base_stroke, text_color) = match tone {
        DialogButtonTone::Secondary => (
            ui_theme::CONTROL,
            Stroke::new(1.0, ui_theme::BORDER),
            ui_theme::TEXT_PRIMARY,
        ),
        DialogButtonTone::Primary => (SETTINGS_BLUE, Stroke::NONE, Color32::WHITE),
        DialogButtonTone::Danger => (Color32::from_rgb(174, 58, 69), Stroke::NONE, Color32::WHITE),
        DialogButtonTone::DangerOutline => {
            (ui_theme::SURFACE_DEEP, Stroke::new(1.0, LIVE_RED), LIVE_RED)
        }
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), Sense::click());
    let interaction = if response.is_pointer_button_down_on() {
        0.14
    } else if response.hovered() {
        0.08
    } else {
        0.0
    };
    let fill = mix_color(base_fill, Color32::WHITE, interaction);
    let stroke = if response.has_focus() {
        if matches!(tone, DialogButtonTone::DangerOutline) {
            Stroke::new(2.0, LIVE_RED)
        } else {
            Stroke::new(1.5, ui_theme::TEXT_PRIMARY)
        }
    } else {
        base_stroke
    };
    ui.painter().rect_filled(rect, 7, fill);
    ui.painter()
        .rect_stroke(rect, 7, stroke, StrokeKind::Inside);

    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), FontId::proportional(14.0), text_color);
    let icon_size = 15.0;
    let gap = 7.0;
    let content_width = icon_size + gap + galley.size().x;
    let icon_rect = Rect::from_min_size(
        Pos2::new(
            rect.center().x - content_width / 2.0,
            rect.center().y - icon_size / 2.0,
        ),
        egui::vec2(icon_size, icon_size),
    );
    ui_icon::paint(ui.painter(), icon_rect, icon, text_color);
    ui.painter().galley(
        Pos2::new(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        text_color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.as_ref())
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn dialog_action_width(available_width: f32, gap: f32, action_count: usize) -> f32 {
    ((available_width - gap * (action_count.saturating_sub(1) as f32)) / action_count as f32)
        .max(1.0)
}
