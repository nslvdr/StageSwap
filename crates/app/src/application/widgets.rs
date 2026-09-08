use super::*;

pub(super) fn translated_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let label = tr(ui, label);
    ui.button(label.as_ref())
}

pub(super) fn translated_small_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let label = tr(ui, label);
    ui.small_button(label.as_ref())
}

pub(super) fn controls_section_heading(ui: &mut egui::Ui, icon: UiIcon, title: &str) -> Rect {
    let title = tr(ui, title);
    let heading = ui.horizontal(|ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
        ui_icon::paint(ui.painter(), icon_rect, icon, SETTINGS_BLUE);
        ui.label(
            RichText::new(title.as_ref())
                .size(11.0)
                .strong()
                .color(ui_theme::TEXT_MUTED),
        );
    });
    ui.add_space(6.0);
    heading.response.rect
}

pub(super) fn controls_section_divider(ui: &mut egui::Ui) -> Rect {
    ui.add_space(2.0);
    let divider = ui.separator().rect;
    ui.add_space(2.0);
    divider
}

pub(super) fn health_state_group(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &'static str,
    current: DeviceState,
) -> Rect {
    let choices = HEALTH_STATES.map(|state| IndicatorChoice {
        icon: device_state_icon(state),
        label: friendly_device_state(state),
        current: state == current,
        tone: device_state_tone(state),
        span: 1,
    });
    indicator_group(ui, icon, label, &choices, None)
}

pub(super) fn detection_state_group(ui: &mut egui::Ui, current: DetectionState) -> Rect {
    let choices = [
        IndicatorChoice {
            icon: UiIcon::Question,
            label: "Checking",
            current: current == DetectionState::Unknown,
            tone: detection_indicator_tone(DetectionState::Unknown),
            span: 1,
        },
        IndicatorChoice {
            icon: UiIcon::Check,
            label: "No media",
            current: current == DetectionState::Matching,
            tone: detection_indicator_tone(DetectionState::Matching),
            span: 1,
        },
        IndicatorChoice {
            icon: UiIcon::Error,
            label: "Media detected",
            current: current == DetectionState::NotMatching,
            tone: detection_indicator_tone(DetectionState::NotMatching),
            span: 1,
        },
        IndicatorChoice {
            icon: UiIcon::Unavailable,
            label: "Reference image missing",
            current: current == DetectionState::ReferenceMissing,
            tone: detection_indicator_tone(DetectionState::ReferenceMissing),
            span: 1,
        },
    ];
    indicator_group(ui, UiIcon::Target, "Media detection", &choices, None)
}

pub(super) fn detection_indicator_tone(state: DetectionState) -> IndicatorTone {
    match state {
        DetectionState::Unknown | DetectionState::NotMatching => IndicatorTone::Amber,
        DetectionState::Matching => IndicatorTone::Green,
        DetectionState::ReferenceMissing => IndicatorTone::Red,
    }
}

pub(super) fn screen_mix_group(ui: &mut egui::Ui, screen_mix: f64) -> Rect {
    let active = if screen_mix <= 0.01 {
        0
    } else if screen_mix >= 0.99 {
        2
    } else {
        1
    };
    let choices = [
        IndicatorChoice {
            icon: UiIcon::Camera,
            label: "Webcam only",
            current: active == 0,
            tone: IndicatorTone::Green,
            span: 1,
        },
        IndicatorChoice {
            icon: UiIcon::Layers,
            label: "Crossfading",
            current: active == 1,
            tone: IndicatorTone::Amber,
            span: 2,
        },
        IndicatorChoice {
            icon: UiIcon::Monitor,
            label: "Secondary screen",
            current: active == 2,
            tone: IndicatorTone::Green,
            span: 1,
        },
    ];
    let percentage = format!("{}%", (screen_mix * 100.0).round());
    indicator_group(
        ui,
        UiIcon::Layers,
        "Screen mix",
        &choices,
        Some((1, &percentage)),
    )
}

pub(super) fn indicator_group(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &'static str,
    choices: &[IndicatorChoice],
    chip_value: Option<(usize, &str)>,
) -> Rect {
    let gap = 4.0;
    let chip_size = 28.0;
    let slots = choices
        .iter()
        .map(|choice| usize::from(choice.span))
        .sum::<usize>();
    let chips_width = chip_size * slots as f32 + gap * (slots as f32 - 1.0);
    let heading_width = (ui.available_width() - chips_width - gap).max(64.0);
    let row = ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.horizontal(|ui| {
            indicator_heading(ui, icon, label, None, heading_width, chip_size);
            for (index, choice) in choices.iter().enumerate() {
                let width =
                    chip_size * f32::from(choice.span) + gap * (f32::from(choice.span) - 1.0);
                let value = chip_value
                    .filter(|(value_index, _)| *value_index == index)
                    .map(|(_, value)| value);
                indicator_chip(ui, label, *choice, width, value);
            }
        });
    });
    ui.add_space(6.0);
    row.response.rect
}

pub(super) fn indicator_heading(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &'static str,
    value: Option<&str>,
    width: f32,
    height: f32,
) {
    let localized_label = tr(ui, label);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let icon_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - 7.0),
        egui::vec2(14.0, 14.0),
    );
    ui_icon::paint(ui.painter(), icon_rect, icon, Color32::LIGHT_GRAY);
    ui.painter().text(
        Pos2::new(icon_rect.right() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        localized_label.as_ref(),
        FontId::proportional(14.0),
        Color32::LIGHT_GRAY,
    );
    if let Some(value) = value {
        ui.painter().text(
            Pos2::new(rect.right(), rect.center().y),
            Align2::RIGHT_CENTER,
            value,
            FontId::proportional(11.0),
            Color32::GRAY,
        );
    }
}

pub(super) fn indicator_chip(
    ui: &mut egui::Ui,
    group: &'static str,
    choice: IndicatorChoice,
    width: f32,
    value: Option<&str>,
) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::hover());
    let amount = ui.ctx().animate_bool_with_time(
        ui.make_persistent_id((group, choice.label)),
        choice.current,
        0.12,
    );
    let (fill, stroke_color, icon_color) = indicator_palette(choice.tone, amount);
    ui.painter().rect_filled(rect, 5, fill);
    ui.painter().rect_stroke(
        rect,
        5,
        Stroke::new(1.0 + amount, stroke_color),
        StrokeKind::Inside,
    );
    if let Some(value) = value {
        let font = FontId::monospace(10.5);
        let galley = ui
            .painter()
            .layout_no_wrap(value.to_owned(), font, icon_color);
        let icon_size = 14.0;
        let gap = 5.0;
        let content_width = icon_size + gap + galley.size().x;
        let icon_rect = Rect::from_min_size(
            Pos2::new(
                rect.center().x - content_width / 2.0,
                rect.center().y - icon_size / 2.0,
            ),
            egui::vec2(icon_size, icon_size),
        );
        ui_icon::paint(ui.painter(), icon_rect, choice.icon, icon_color);
        ui.painter().galley(
            Pos2::new(
                icon_rect.right() + gap,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            icon_color,
        );
    } else {
        ui_icon::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), egui::vec2(15.0, 15.0)),
            choice.icon,
            icon_color,
        );
    }
    response.on_hover_text(tr(ui, choice.label));
}

pub(super) fn indicator_palette(
    tone: IndicatorTone,
    active_amount: f32,
) -> (Color32, Color32, Color32) {
    let inactive_fill = Color32::from_rgb(31, 34, 40);
    let inactive_stroke = Color32::from_rgb(91, 97, 108);
    let inactive_text = Color32::from_rgb(151, 158, 170);
    let (active_fill, active_stroke) = match tone {
        IndicatorTone::Green => (Color32::from_rgb(34, 81, 58), ACTIVE_GREEN),
        IndicatorTone::Amber => (Color32::from_rgb(82, 67, 33), TRANSITION_AMBER),
        IndicatorTone::Red => (Color32::from_rgb(86, 38, 42), LIVE_RED),
    };
    (
        mix_color(inactive_fill, active_fill, active_amount),
        mix_color(inactive_stroke, active_stroke, active_amount),
        mix_color(inactive_text, Color32::WHITE, active_amount),
    )
}

pub(super) fn device_state_icon(state: DeviceState) -> UiIcon {
    match state {
        DeviceState::Initializing => UiIcon::Loader,
        DeviceState::Ready => UiIcon::Check,
        DeviceState::Unavailable => UiIcon::Unavailable,
        DeviceState::Failed => UiIcon::Error,
    }
}

pub(super) fn device_state_tone(state: DeviceState) -> IndicatorTone {
    match state {
        DeviceState::Ready => IndicatorTone::Green,
        DeviceState::Initializing => IndicatorTone::Amber,
        DeviceState::Unavailable | DeviceState::Failed => IndicatorTone::Red,
    }
}

pub(super) fn friendly_device_state(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Initializing => "Initializing",
        DeviceState::Ready => "Ready",
        DeviceState::Unavailable => "Unavailable",
        DeviceState::Failed => "Failed",
    }
}

pub(super) fn app_title(ui: &mut egui::Ui, texture: egui::TextureId) {
    const ICON_SIZE: f32 = 22.0;
    const GAP: f32 = 7.0;
    let color = Color32::from_rgb(205, 211, 222);
    let font = FontId::proportional(14.0);
    let title = ui
        .painter()
        .layout_no_wrap("StageSwap".to_owned(), font, color);
    let size = egui::vec2(
        ICON_SIZE + GAP + title.size().x,
        title.size().y.max(ICON_SIZE),
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let icon_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - ICON_SIZE / 2.0),
        egui::vec2(ICON_SIZE, ICON_SIZE),
    );
    ui.painter().image(
        texture,
        icon_rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    let title_pos = Pos2::new(
        icon_rect.right() + GAP,
        rect.center().y - title.size().y / 2.0,
    );
    ui.painter().galley(title_pos, title, color);
}

pub(super) fn icon_text(ui: &mut egui::Ui, icon: UiIcon, text: &str, color: Color32, strong: bool) {
    let text = tr(ui, text);
    let font = FontId::proportional(if strong { 14.0 } else { 11.0 });
    let galley = ui.painter().layout_no_wrap(text.into_owned(), font, color);
    let icon_size = if strong { 15.0 } else { 13.0 };
    let size = egui::vec2(
        icon_size + 6.0 + galley.size().x,
        galley.size().y.max(icon_size),
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let icon_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    ui_icon::paint(ui.painter(), icon_rect, icon, color);
    ui.painter().galley(
        Pos2::new(
            icon_rect.right() + 6.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    text: &str,
    desired_size: Vec2,
    selected: bool,
    emphasized: bool,
) -> egui::Response {
    icon_button_impl(ui, icon, text, desired_size, selected, emphasized, None)
}

pub(super) fn icon_only_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    accessible_label: &str,
    desired_size: Vec2,
) -> egui::Response {
    let accessible_label = tr(ui, accessible_label);
    let response = icon_button_impl(ui, icon, "", desired_size, false, false, None);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            accessible_label.as_ref(),
        )
    });
    response
}

pub(super) fn accent_icon_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    text: &str,
    desired_size: Vec2,
    accent: Color32,
) -> egui::Response {
    icon_button_impl(ui, icon, text, desired_size, false, true, Some(accent))
}

pub(super) fn icon_button_impl(
    ui: &mut egui::Ui,
    icon: UiIcon,
    text: &str,
    desired_size: Vec2,
    selected: bool,
    emphasized: bool,
    accent: Option<Color32>,
) -> egui::Response {
    let text = tr(ui, text);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());
    let visuals = ui.style().interact(&response);
    let fill = if selected {
        ui.visuals().selection.bg_fill
    } else if let Some(accent) = accent {
        mix_color(
            ui_theme::SURFACE_DEEP,
            accent,
            if response.hovered() { 0.48 } else { 0.34 },
        )
    } else if emphasized && !response.hovered() {
        ui_theme::CONTROL
    } else {
        visuals.bg_fill
    };
    let stroke = if selected {
        Stroke::new(1.0, ui.visuals().selection.stroke.color)
    } else if let Some(accent) = accent {
        Stroke::new(1.25, accent)
    } else {
        visuals.bg_stroke
    };
    ui.painter().rect_filled(rect, 6, fill);
    ui.painter()
        .rect_stroke(rect, 6, stroke, StrokeKind::Inside);

    let color = if selected || emphasized || response.hovered() {
        Color32::WHITE
    } else {
        visuals.fg_stroke.color
    };
    let font = FontId::proportional(if emphasized { 14.0 } else { 12.0 });
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let icon_size = if emphasized { 16.0 } else { 14.0 };
    let text_gap = if text.is_empty() { 0.0 } else { 7.0 };
    let content_width = icon_size + text_gap + galley.size().x;
    let left = rect.center().x - content_width / 2.0;
    let icon_rect = Rect::from_min_size(
        Pos2::new(left, rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    ui_icon::paint(ui.painter(), icon_rect, icon, color);
    if !text.is_empty() {
        ui.painter().galley(
            Pos2::new(
                icon_rect.right() + text_gap,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn mix_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let mix =
        |left: u8, right: u8| (left as f32 + (right as f32 - left as f32) * amount).round() as u8;
    Color32::from_rgba_unmultiplied(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
        mix(from.a(), to.a()),
    )
}
