use super::*;

impl SwitcherApp {
    pub(super) fn notification_overlay(&mut self, context: &egui::Context) {
        let now = Instant::now();
        self.notifications.prune(now);
        if let Some(deadline) = self.notifications.next_toast_deadline() {
            context.request_repaint_after(deadline.saturating_duration_since(now));
        }

        if self.active_dialog.is_some() {
            self.notification_center_open = false;
            return;
        }

        let show_bell = matches!(self.view, AppView::Dashboard);
        if show_bell {
            let unread = self.notifications.unread_count();
            let mut clicked = false;
            egui::Area::new(egui::Id::new("notification-bell"))
                .anchor(Align2::LEFT_BOTTOM, Vec2::new(14.0, -14.0))
                .order(egui::Order::Foreground)
                .show(context, |ui| {
                    clicked = notification_bell(ui, unread);
                });
            let opened_by_bell = clicked && !self.notification_center_open;
            if clicked {
                self.notification_center_open = !self.notification_center_open;
                if self.notification_center_open {
                    self.notifications.mark_all_read();
                }
            }
            if self.notification_center_open
                && !opened_by_bell
                && self.notification_popover(context, now)
            {
                self.notification_center_open = false;
            }
        } else {
            self.notification_center_open = false;
        }

        if !self.notification_center_open {
            self.notification_toasts(context, now);
        }
    }

    pub(super) fn notification_popover(&mut self, context: &egui::Context, now: Instant) -> bool {
        let popover_width =
            (context.content_rect().width() - 48.0).clamp(1.0, NOTIFICATION_POPOVER_WIDTH);
        let notification_list_height =
            (context.content_rect().height() - 160.0).clamp(120.0, 360.0);
        let has_notifications = self.notifications.entries().next().is_some();
        let mut clear_all = false;
        let popover = egui::Area::new(egui::Id::new("notification-popover"))
            .anchor(Align2::LEFT_BOTTOM, Vec2::new(14.0, -54.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                egui::Frame::new()
                    .fill(ui_theme::SURFACE)
                    .stroke(Stroke::new(1.0, ui_theme::BORDER))
                    .corner_radius(12)
                    .inner_margin(8)
                    .show(ui, |ui| {
                        ui.set_width(popover_width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let (header_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), NOTIFICATION_POPOVER_HEADER_HEIGHT),
                            Sense::hover(),
                        );
                        let clear_all_rect = Rect::from_min_size(
                            Pos2::new(header_rect.right() - 92.0, header_rect.top()),
                            Vec2::new(92.0, NOTIFICATION_POPOVER_HEADER_HEIGHT),
                        );
                        let title_rect = Rect::from_min_max(
                            header_rect.min,
                            Pos2::new(clear_all_rect.left() - 10.0, header_rect.bottom()),
                        );
                        let mut title_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt("notification-popover-title")
                                .max_rect(title_rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        let (icon_rect, _) =
                            title_ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                        ui_icon::paint(title_ui.painter(), icon_rect, UiIcon::Bell, SETTINGS_BLUE);
                        title_ui.add_space(6.0);
                        title_ui.label(
                            RichText::new(tr(&title_ui, "Notifications"))
                                .size(14.0)
                                .strong()
                                .color(Color32::WHITE),
                        );

                        let mut clear_all_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt("notification-popover-clear-all")
                                .max_rect(clear_all_rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        let response = clear_all_ui.add_enabled_ui(has_notifications, |ui| {
                            icon_button(
                                ui,
                                UiIcon::Trash,
                                "Clear all",
                                Vec2::new(92.0, NOTIFICATION_POPOVER_HEADER_HEIGHT),
                                false,
                                false,
                            )
                        });
                        clear_all = response.inner.clicked();
                        ui.add_space(NOTIFICATION_POPOVER_SPACING);
                        ui.separator();
                        ui.add_space(NOTIFICATION_POPOVER_SPACING);
                        if self.notifications.entries().next().is_none() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(NOTIFICATION_POPOVER_SPACING);
                                let (icon_rect, _) =
                                    ui.allocate_exact_size(Vec2::new(24.0, 24.0), Sense::hover());
                                ui_icon::paint(
                                    ui.painter(),
                                    icon_rect,
                                    UiIcon::CheckCircle,
                                    ACTIVE_GREEN,
                                );
                                ui.add_space(NOTIFICATION_POPOVER_SPACING);
                                ui.label(
                                    RichText::new(tr(ui, "You’re all caught up"))
                                        .strong()
                                        .color(ui_theme::TEXT_PRIMARY),
                                );
                                ui.add_space(NOTIFICATION_POPOVER_SPACING);
                                ui.label(
                                    RichText::new(tr(ui, "No recent notifications."))
                                        .size(11.0)
                                        .color(ui_theme::TEXT_MUTED),
                                );
                                ui.add_space(NOTIFICATION_POPOVER_SPACING);
                            });
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("notification-list")
                                .auto_shrink([false, true])
                                .content_margin(0.0)
                                .max_height(notification_list_height)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = NOTIFICATION_POPOVER_SPACING;
                                    for item in self.notifications.entries() {
                                        notification_entry_card(ui, item, now);
                                    }
                                });
                        }
                    });
            });
        if clear_all {
            self.notifications.clear_all();
        }
        context.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|position| !popover.response.rect.contains(position))
        })
    }

    pub(super) fn notification_toasts(&self, context: &egui::Context, now: Instant) {
        let items: Vec<_> = self
            .notifications
            .toasts()
            .filter_map(|toast| self.notifications.entry(toast.notification_id))
            .cloned()
            .collect();
        if items.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("notification-toasts"))
            .anchor(Align2::RIGHT_TOP, Vec2::new(-16.0, 16.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                ui.set_width(
                    (context.content_rect().width() - 32.0).clamp(1.0, NOTIFICATION_TOAST_WIDTH),
                );
                ui.spacing_mut().item_spacing.y = 8.0;
                for item in &items {
                    notification_toast_card(ui, item, now);
                }
            });
    }
}

pub(super) fn notification_bell(ui: &mut egui::Ui, unread: usize) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::click());
    let icon_color = if response.hovered() {
        ui_theme::TEXT_PRIMARY
    } else {
        ui_theme::TEXT_SUBTLE
    };
    ui_icon::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::new(18.0, 18.0)),
        UiIcon::Bell,
        icon_color,
    );
    if unread > 0 {
        ui.painter().circle_filled(
            Pos2::new(rect.right() - 3.0, rect.top() + 3.0),
            3.5,
            LIVE_RED,
        );
    }
    response.on_hover_text(tr(ui, "Notifications")).clicked()
}

pub(super) fn notification_entry_card(ui: &mut egui::Ui, item: &NotificationItem, now: Instant) {
    let (icon, color) = notification_icon_and_color(item.source, item.tone);
    let available_width = (ui.available_width() - 18.0).max(1.0);
    egui::Frame::new()
        .fill(ui_theme::SURFACE)
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.28)))
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(9, 7))
        .show(ui, |ui| {
            ui.set_width(available_width);
            ui.horizontal_top(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                ui_icon::paint(ui.painter(), icon_rect, icon, color);
                ui.add_space(7.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(notification_title(ui, item.source))
                                .size(12.5)
                                .strong()
                                .color(ui_theme::TEXT_PRIMARY),
                        );
                        if item.unread {
                            ui.add_space(4.0);
                            let (dot, _) =
                                ui.allocate_exact_size(Vec2::new(6.0, 6.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 3.0, color);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(notification_age(ui, item.created_at, now))
                                    .size(10.0)
                                    .color(ui_theme::TEXT_MUTED),
                            );
                        });
                    });
                    ui.add_space(3.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&item.body)
                                .size(11.5)
                                .line_height(Some(15.0))
                                .color(ui_theme::TEXT_SECONDARY),
                        )
                        .wrap(),
                    );
                    if let Some(detail) = &item.detail {
                        ui.add_space(3.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(detail)
                                    .size(10.5)
                                    .line_height(Some(14.0))
                                    .color(ui_theme::TEXT_MUTED),
                            )
                            .wrap(),
                        );
                    }
                });
            });
        });
}

pub(super) fn notification_toast_card(ui: &mut egui::Ui, item: &NotificationItem, now: Instant) {
    let (icon, color) = notification_icon_and_color(item.source, item.tone);
    let available_width = (ui.available_width() - 20.0).max(1.0);
    egui::Frame::new()
        .fill(ui_theme::SURFACE)
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.50)))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(available_width);
            ui.horizontal_top(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(19.0, 19.0), Sense::hover());
                ui_icon::paint(ui.painter(), icon_rect, icon, color);
                ui.add_space(7.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(notification_title(ui, item.source))
                                .size(12.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(notification_age(ui, item.created_at, now))
                                    .size(10.0)
                                    .color(ui_theme::TEXT_MUTED),
                            );
                        });
                    });
                    ui.add_space(3.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&item.body)
                                .size(11.5)
                                .line_height(Some(15.0))
                                .color(ui_theme::TEXT_SECONDARY),
                        )
                        .wrap(),
                    );
                });
            });
        });
}

pub(super) fn notification_icon_and_color(
    source: NotificationSource,
    tone: NotificationTone,
) -> (UiIcon, Color32) {
    let icon = match source {
        NotificationSource::Startup | NotificationSource::Configuration => UiIcon::Settings,
        NotificationSource::DeviceWorker | NotificationSource::Publisher => UiIcon::Wrench,
        NotificationSource::VirtualCamera => UiIcon::Broadcast,
        NotificationSource::Webcam => UiIcon::Camera,
        NotificationSource::Screen => UiIcon::Monitor,
        NotificationSource::Matching => UiIcon::Target,
        NotificationSource::Reference => UiIcon::Image,
        NotificationSource::Command => UiIcon::Warning,
        NotificationSource::Updates => UiIcon::Download,
        NotificationSource::Preview => UiIcon::Info,
    };
    let color = match tone {
        NotificationTone::Critical => LIVE_RED,
        NotificationTone::Information => SETTINGS_BLUE,
    };
    (icon, color)
}

pub(super) fn notification_title(
    ui: &egui::Ui,
    source: NotificationSource,
) -> std::borrow::Cow<'static, str> {
    tr(
        ui,
        match source {
            NotificationSource::Startup => "Startup needs attention",
            NotificationSource::Configuration => "Configuration needs attention",
            NotificationSource::DeviceWorker => "Device services need attention",
            NotificationSource::Publisher => "Frame publisher needs attention",
            NotificationSource::VirtualCamera => "Zoom output needs attention",
            NotificationSource::Webcam => "Webcam needs attention",
            NotificationSource::Screen => "Screen capture needs attention",
            NotificationSource::Matching => "Matching needs attention",
            NotificationSource::Reference => "Reference image needs attention",
            NotificationSource::Command => "StageSwap needs attention",
            NotificationSource::Updates => "Update available",
            NotificationSource::Preview => "Preview activity",
        },
    )
}

pub(super) fn notification_age(ui: &egui::Ui, created_at: Instant, now: Instant) -> String {
    let seconds = now.saturating_duration_since(created_at).as_secs();
    if seconds < 60 {
        return tr(ui, "Just now").into_owned();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format_text(
            ui_locale(ui),
            if minutes == 1 {
                "{0} minute ago"
            } else {
                "{0} minutes ago"
            },
            &[&minutes.to_string()],
        );
    }
    let hours = minutes / 60;
    format_text(
        ui_locale(ui),
        if hours == 1 {
            "{0} hour ago"
        } else {
            "{0} hours ago"
        },
        &[&hours.to_string()],
    )
}
