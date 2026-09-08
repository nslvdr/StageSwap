use super::*;

/// Keeps preview resources and enlargement state together for visibility changes.
#[derive(Default)]
pub(super) struct PreviewState {
    pub(super) textures: HashMap<&'static str, PreviewTexture>,
    pub(super) converters: HashMap<PreviewKind, PreviewConverter>,
    pub(super) enlarged: Option<PreviewKind>,
}

impl SwitcherApp {
    pub(super) fn preview_cell(
        &mut self,
        ui: &mut egui::Ui,
        kind: PreviewKind,
        frame: Option<&Arc<Frame>>,
        size: [f32; 2],
        actual_output: Source,
        fps: Option<u32>,
    ) -> Rect {
        let preview_rect = self.preview(
            ui,
            kind,
            frame,
            size,
            actual_output,
            PreviewOptions::dashboard(kind, fps),
        );
        let click = ui
            .interact(
                preview_rect,
                ui.make_persistent_id((kind.key(), "dashboard-preview-click")),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if click.clicked() {
            self.previews.enlarged = Some(kind);
        }
        ui.add_space(8.0);
        preview_caption(ui, kind);
        ui.add_space(16.0);
        preview_rect
    }

    pub(super) fn preview(
        &mut self,
        ui: &mut egui::Ui,
        kind: PreviewKind,
        frame: Option<&Arc<Frame>>,
        maximum: [f32; 2],
        actual_output: Source,
        options: PreviewOptions,
    ) -> Rect {
        let available = Vec2::new(maximum[0].min(ui.available_width()), maximum[1]);
        let preview_rect = match options.style {
            PreviewStyle::CinemaPeek => {
                ui.allocate_ui_with_layout(
                    available,
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| self.render_preview_content(ui, kind, frame, available, options, true),
                )
                .response
                .rect
            }
            PreviewStyle::Card => {
                let contour = preview_contour(kind, actual_output, self.snapshot().output_layout);
                let disco_enabled = self.snapshot().disco_enabled;
                let disco_elapsed =
                    Instant::now().saturating_duration_since(self.ui_animation_started_at);
                let active_amount = ui.ctx().animate_bool_with_time(
                    ui.make_persistent_id((kind.key(), "active-contour")),
                    contour == PreviewContour::Active,
                    0.14,
                );
                let disco_offset = match kind {
                    PreviewKind::Webcam => 0.0,
                    PreviewKind::Screen => 0.22,
                    PreviewKind::Reference => 0.46,
                    PreviewKind::Output => 0.7,
                };
                let contour_color = if disco_enabled {
                    disco_ui_color(disco_elapsed, disco_offset)
                } else {
                    match contour {
                        PreviewContour::Live => LIVE_RED,
                        PreviewContour::Active | PreviewContour::Neutral => {
                            mix_color(PREVIEW_NEUTRAL, ACTIVE_GREEN, active_amount)
                        }
                    }
                };
                let contour_width = if disco_enabled { 4.0 } else { 3.0 };
                let frame_fill = if disco_enabled {
                    mix_color(Color32::from_rgb(12, 14, 18), contour_color, 0.14)
                } else {
                    ui_theme::PREVIEW_BACKGROUND
                };
                let preview_frame = egui::Frame::new()
                    .fill(frame_fill)
                    .stroke(Stroke::new(contour_width, contour_color))
                    .corner_radius(8)
                    .inner_margin(3);
                let frame_margin = preview_frame.total_margin();
                let inner_size = (available
                    - egui::vec2(
                        frame_margin.left + frame_margin.right,
                        frame_margin.top + frame_margin.bottom,
                    ))
                .max(egui::vec2(1.0, 1.0));
                preview_frame
                    .show(ui, |ui| {
                        ui.allocate_ui_with_layout(
                            inner_size,
                            egui::Layout::centered_and_justified(egui::Direction::TopDown),
                            |ui| {
                                self.render_preview_content(
                                    ui, kind, frame, inner_size, options, false,
                                );
                            },
                        );
                    })
                    .response
                    .rect
            }
        };
        if options.show_fps {
            paint_fps_overlay(ui, preview_rect, options.fps);
        }
        preview_rect
    }

    pub(super) fn render_preview_content(
        &mut self,
        ui: &mut egui::Ui,
        kind: PreviewKind,
        frame: Option<&Arc<Frame>>,
        size: Vec2,
        options: PreviewOptions,
        rounded: bool,
    ) {
        if let Some(frame) = frame {
            let texture_size = preview_texture_size(
                frame.as_ref(),
                size,
                ui.ctx().pixels_per_point(),
                options.texture_limit,
            );
            let prepared = {
                let converter = self
                    .previews
                    .converters
                    .entry(kind)
                    .or_insert_with(|| PreviewConverter::new(kind.key()));
                converter.submit(
                    Arc::clone(frame),
                    texture_size,
                    options.refresh_interval,
                    Instant::now(),
                );
                converter.take_ready()
            };
            if let Some(prepared) = prepared {
                if let Some(texture) = self.previews.textures.get_mut(kind.key()) {
                    if texture.source != prepared.id || texture.size != prepared.size {
                        texture.texture.set(prepared.image, TextureOptions::LINEAR);
                        texture.source = prepared.id;
                        texture.size = prepared.size;
                    }
                } else {
                    let texture =
                        ui.ctx()
                            .load_texture(kind.key(), prepared.image, TextureOptions::LINEAR);
                    self.previews.textures.insert(
                        kind.key(),
                        PreviewTexture {
                            source: prepared.id,
                            size: prepared.size,
                            texture,
                        },
                    );
                }
            }
            if let Some(texture) = self.previews.textures.get(kind.key()) {
                let mut image =
                    egui::Image::new((texture.texture.id(), size)).maintain_aspect_ratio(true);
                if rounded {
                    image = image.corner_radius(8);
                }
                ui.add(image);
            } else {
                ui.label(
                    RichText::new(tr(ui, "Preparing preview…"))
                        .size(12.0)
                        .color(ui_theme::TEXT_DIM),
                );
            }
        } else {
            ui.label(
                RichText::new(tr(ui, options.empty_message))
                    .size(12.0)
                    .color(ui_theme::TEXT_DIM),
            );
        }
    }
}

pub(super) fn preview_contour(
    kind: PreviewKind,
    actual_output: Source,
    output_layout: OutputLayout,
) -> PreviewContour {
    match kind {
        PreviewKind::Output => PreviewContour::Live,
        PreviewKind::Webcam | PreviewKind::Screen
            if matches!(
                output_layout,
                OutputLayout::WebcamMainScreenPip | OutputLayout::ScreenMainWebcamPip
            ) =>
        {
            PreviewContour::Active
        }
        PreviewKind::Webcam if actual_output == Source::Camera => PreviewContour::Active,
        PreviewKind::Screen if actual_output == Source::Screen => PreviewContour::Active,
        PreviewKind::Webcam | PreviewKind::Screen | PreviewKind::Reference => {
            PreviewContour::Neutral
        }
    }
}

pub(super) fn preview_caption(ui: &mut egui::Ui, kind: PreviewKind) {
    let font = FontId::proportional(11.0);
    let text_color = Color32::LIGHT_GRAY;
    let localized_label = tr(ui, kind.label());
    let label = ui
        .painter()
        .layout_no_wrap(localized_label.into_owned(), font.clone(), text_color);
    let label_width = label.size().x;
    let live = (kind == PreviewKind::Output).then(|| {
        ui.painter()
            .layout_no_wrap(tr(ui, "LIVE").into_owned(), font.clone(), LIVE_RED)
    });
    let icon_size = 13.0;
    let live_width = live.as_ref().map_or(0.0, |galley| 12.0 + galley.size().x);
    let total_width = icon_size + 6.0 + label_width + live_width;
    let height = label.size().y.max(icon_size);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_width, height), Sense::hover());
    let painter = ui.painter();
    let icon_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    ui_icon::paint(painter, icon_rect, kind.icon(), text_color);
    let label_pos = Pos2::new(
        icon_rect.right() + 6.0,
        rect.center().y - label.size().y / 2.0,
    );
    painter.galley(label_pos, label, text_color);
    if let Some(live) = live {
        let live_x = label_pos.x + label_width + 12.0;
        painter.circle_filled(Pos2::new(live_x - 5.0, rect.center().y), 2.5, LIVE_RED);
        painter.galley(
            Pos2::new(live_x, rect.center().y - live.size().y / 2.0),
            live,
            LIVE_RED,
        );
    }
}

pub(super) fn paint_fps_overlay(ui: &egui::Ui, preview_rect: Rect, fps: Option<u32>) {
    let text = fps.map_or_else(|| "-- FPS".to_owned(), |value| format!("{value} FPS"));
    let font = FontId::monospace(10.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text, font, Color32::from_rgb(232, 235, 240));
    let size = galley.size() + egui::vec2(10.0, 6.0);
    let overlay_max = preview_rect.max - egui::vec2(6.0, 6.0);
    let overlay = Rect::from_min_size(overlay_max - size, size);
    ui.painter()
        .rect_filled(overlay, 4, Color32::from_rgba_unmultiplied(6, 8, 12, 184));
    ui.painter().galley(
        overlay.min + egui::vec2(5.0, 3.0),
        galley,
        Color32::from_rgb(232, 235, 240),
    );
}

pub(super) fn preview_texture_size(
    frame: &Frame,
    maximum: Vec2,
    pixels_per_point: f32,
    texture_limit: [u32; 2],
) -> [usize; 2] {
    let maximum_width =
        ((maximum.x * pixels_per_point).round().max(1.0) as u32).min(texture_limit[0]);
    let maximum_height =
        ((maximum.y * pixels_per_point).round().max(1.0) as u32).min(texture_limit[1]);
    let scale = f64::min(
        f64::min(
            f64::from(maximum_width) / f64::from(frame.size.width),
            f64::from(maximum_height) / f64::from(frame.size.height),
        ),
        1.0,
    );
    [
        (f64::from(frame.size.width) * scale).round().max(1.0) as usize,
        (f64::from(frame.size.height) * scale).round().max(1.0) as usize,
    ]
}

pub(super) fn frame_image(frame: &Frame, target: [usize; 2]) -> egui::ColorImage {
    let source_x = (0..target[0])
        .map(|x| x * frame.size.width as usize / target[0] * 4)
        .collect::<Vec<_>>();
    let source_rows = (0..target[1])
        .map(|y| y * frame.size.height as usize / target[1] * frame.stride as usize)
        .collect::<Vec<_>>();
    let mut pixels = Vec::with_capacity(target[0] * target[1]);
    for source_row in source_rows {
        for &source_x in &source_x {
            let offset = source_row + source_x;
            pixels.push(Color32::from_rgba_unmultiplied(
                frame.pixels()[offset + 2],
                frame.pixels()[offset + 1],
                frame.pixels()[offset],
                frame.pixels()[offset + 3],
            ));
        }
    }
    egui::ColorImage::new(target, pixels)
}
