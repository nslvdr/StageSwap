use super::*;

impl SwitcherApp {
    pub(super) fn paint_disco_interface(
        &mut self,
        context: &egui::Context,
        content_rect: Rect,
        disco_enabled: bool,
    ) {
        let now = Instant::now();
        if !disco_enabled {
            return;
        }
        let elapsed = now.saturating_duration_since(self.ui_animation_started_at);
        let activation_elapsed = self
            .disco_ui_activated_at
            .map(|activated_at| now.saturating_duration_since(activated_at))
            .unwrap_or(Duration::from_secs(2));
        paint_disco_interface(
            context,
            content_rect,
            elapsed,
            activation_elapsed,
            disco_enabled,
        );
    }
}

pub(super) fn disco_ui_color(elapsed: Duration, offset: f32) -> Color32 {
    const COLORS: [Color32; 6] = [
        Color32::from_rgb(255, 28, 190),
        Color32::from_rgb(110, 70, 255),
        Color32::from_rgb(24, 176, 255),
        Color32::from_rgb(36, 245, 82),
        Color32::from_rgb(255, 224, 34),
        Color32::from_rgb(255, 70, 38),
    ];
    let position = ((elapsed.as_secs_f32() * 0.42 + offset).rem_euclid(1.0)) * COLORS.len() as f32;
    let index = position.floor() as usize % COLORS.len();
    mix_color(
        COLORS[index],
        COLORS[(index + 1) % COLORS.len()],
        position.fract(),
    )
}

pub(super) fn disco_flash_envelope(elapsed: Duration) -> f32 {
    let seconds = elapsed.as_secs_f32();
    let beat = (seconds / 1.2).fract();
    let primary = if beat < 0.1 { 1.0 - beat / 0.1 } else { 0.0 };
    let secondary = if (0.22..0.29).contains(&beat) {
        1.0 - (beat - 0.22) / 0.07
    } else {
        0.0
    };
    let major = (seconds / 3.0).fract();
    let major = if major < 0.055 {
        1.0 - major / 0.055
    } else {
        0.0
    };
    primary.max(secondary * 0.72).max(major)
}

pub(super) fn disco_ball_drop_progress(elapsed: Duration) -> f32 {
    let progress = (elapsed.as_secs_f32() / 1.35).clamp(0.0, 1.0);
    let shifted = progress - 1.0;
    1.0 + 2.701_58 * shifted.powi(3) + 1.701_58 * shifted.powi(2)
}

pub(super) fn disco_ball_center(rect: Rect, activation_elapsed: Duration) -> Pos2 {
    let progress = disco_ball_drop_progress(activation_elapsed);
    Pos2::new(
        rect.center().x,
        rect.top() - 72.0 + (rect.top() + 158.0 - (rect.top() - 72.0)) * progress,
    )
}

pub(super) fn paint_disco_ball(
    painter: &egui::Painter,
    rect: Rect,
    elapsed: Duration,
    activation_elapsed: Duration,
) {
    let center = disco_ball_center(rect, activation_elapsed);
    let radius = (rect.height() * 0.075).clamp(44.0, 62.0);
    let motor = Rect::from_center_size(
        Pos2::new(center.x, rect.top() + 11.0),
        egui::vec2(52.0, 22.0),
    );
    painter.rect_filled(motor, 5, Color32::from_rgb(54, 58, 68));
    painter.rect_stroke(
        motor,
        5,
        Stroke::new(1.5, Color32::from_rgb(154, 161, 174)),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_center_size(
            Pos2::new(center.x, motor.bottom() + 4.0),
            egui::vec2(13.0, 8.0),
        ),
        2,
        Color32::from_rgb(104, 111, 123),
    );
    if center.y - radius > motor.bottom() {
        painter.line_segment(
            [
                Pos2::new(center.x - 1.2, motor.bottom() + 8.0),
                Pos2::new(center.x - 1.2, center.y - radius),
            ],
            Stroke::new(3.2, Color32::from_rgb(72, 76, 86)),
        );
        painter.line_segment(
            [
                Pos2::new(center.x + 1.1, motor.bottom() + 8.0),
                Pos2::new(center.x + 1.1, center.y - radius),
            ],
            Stroke::new(1.1, Color32::from_rgb(196, 201, 211)),
        );
    }
    let t = elapsed.as_secs_f32();
    let flash = disco_flash_envelope(elapsed);
    let shimmer = (t * 2.1).sin().mul_add(0.5, 0.5);
    painter.circle_filled(
        center,
        radius + 38.0 + shimmer * 5.0,
        Color32::from_rgba_unmultiplied(230, 240, 255, 9),
    );
    painter.circle_filled(
        center,
        radius + 25.0 + shimmer * 3.0,
        Color32::from_rgba_unmultiplied(220, 235, 255, 17),
    );
    painter.circle_filled(
        center,
        radius + 14.0,
        Color32::from_rgba_unmultiplied(245, 250, 255, 34),
    );
    painter.circle_filled(center, radius + 5.0, Color32::from_rgb(45, 50, 62));
    painter.circle_filled(center, radius, Color32::from_rgb(72, 78, 91));

    const LATITUDE_BANDS: usize = 12;
    const LONGITUDE_BANDS: usize = 16;
    let project = |latitude: f32, longitude: f32| {
        Pos2::new(
            center.x + radius * latitude.cos() * longitude.sin(),
            center.y + radius * latitude.sin(),
        )
    };
    for row in 0..LATITUDE_BANDS {
        let latitude_top = -std::f32::consts::FRAC_PI_2
            + row as f32 * std::f32::consts::PI / LATITUDE_BANDS as f32;
        let latitude_bottom = -std::f32::consts::FRAC_PI_2
            + (row + 1) as f32 * std::f32::consts::PI / LATITUDE_BANDS as f32;
        let latitude = (latitude_top + latitude_bottom) * 0.5;
        for column in 0..LONGITUDE_BANDS {
            let longitude_left = -std::f32::consts::FRAC_PI_2
                + column as f32 * std::f32::consts::PI / LONGITUDE_BANDS as f32;
            let longitude_right = -std::f32::consts::FRAC_PI_2
                + (column + 1) as f32 * std::f32::consts::PI / LONGITUDE_BANDS as f32;
            let longitude = (longitude_left + longitude_right) * 0.5;
            let normal_x = latitude.cos() * longitude.sin();
            let normal_y = latitude.sin();
            let normal_z = latitude.cos() * longitude.cos();
            let diffuse = (normal_x * -0.38 + normal_y * -0.48 + normal_z * 0.79).clamp(0.0, 1.0);
            let specular = diffuse.powi(7);
            let reflection =
                ((longitude * 3.2 + latitude * 1.6 + t * 1.15).sin() * 0.5 + 0.5).powi(6);
            let moving_glint =
                ((longitude * 5.4 - latitude * 2.7 + t * 1.85).sin() * 0.5 + 0.5).powi(14);
            let rim = (1.0 - normal_z.max(0.0)).powi(3);
            let brightness = (58.0
                + diffuse * 142.0
                + specular * 105.0
                + reflection * 52.0
                + moving_glint * 112.0
                + rim * 26.0)
                .min(255.0) as u8;
            let silver = Color32::from_rgb(
                brightness.saturating_sub(5),
                brightness,
                brightness.saturating_add(5),
            );
            let accent = disco_ui_color(
                elapsed,
                (column as f32 / LONGITUDE_BANDS as f32
                    + row as f32 / LATITUDE_BANDS as f32 * 0.31
                    + t * 0.035)
                    .fract(),
            );
            let reflected = mix_color(
                silver,
                accent,
                (0.03 + reflection * 0.48 + moving_glint * 0.12).min(0.62),
            );
            let reflected = mix_color(
                reflected,
                Color32::WHITE,
                (specular * 0.74 + reflection * 0.24 + moving_glint * 0.92 + flash * 0.11)
                    .min(0.96),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    project(latitude_top, longitude_left),
                    project(latitude_top, longitude_right),
                    project(latitude_bottom, longitude_right),
                    project(latitude_bottom, longitude_left),
                ],
                reflected,
                Stroke::new(0.65, Color32::from_rgba_unmultiplied(24, 28, 36, 135)),
            ));
        }
    }
    painter.circle_stroke(
        center,
        radius + 1.5,
        Stroke::new(5.0, Color32::from_rgba_unmultiplied(245, 250, 255, 116)),
    );
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(
            2.5,
            mix_color(Color32::WHITE, disco_ui_color(elapsed, 0.12), 0.34),
        ),
    );
    if flash > 0.0 {
        painter.circle_stroke(
            center,
            radius + flash * 3.0,
            Stroke::new(
                2.0 + flash * 1.5,
                Color32::from_rgba_unmultiplied(255, 255, 255, (flash * 80.0) as u8),
            ),
        );
    }
    painter.circle_stroke(
        center,
        radius * 0.72,
        Stroke::new(1.4, Color32::from_rgba_unmultiplied(245, 250, 255, 150)),
    );
    painter.circle_filled(
        Pos2::new(center.x - radius * 0.3, center.y - radius * 0.34),
        radius * 0.24,
        Color32::from_rgba_unmultiplied(255, 255, 255, 105),
    );
    painter.circle_filled(
        Pos2::new(center.x - radius * 0.34, center.y - radius * 0.38),
        radius * 0.085,
        Color32::from_rgba_unmultiplied(255, 255, 255, 242),
    );
    let glint_center = Pos2::new(
        center.x + (t * 1.25).sin() * radius * 0.48,
        center.y - radius * 0.12 + (t * 0.8).cos() * radius * 0.23,
    );
    painter.circle_filled(
        glint_center,
        radius * 0.055,
        Color32::from_rgba_unmultiplied(255, 255, 255, 250),
    );
    painter.line_segment(
        [
            Pos2::new(glint_center.x - radius * 0.22, glint_center.y),
            Pos2::new(glint_center.x + radius * 0.22, glint_center.y),
        ],
        Stroke::new(2.2, Color32::from_rgba_unmultiplied(255, 255, 255, 205)),
    );
    painter.line_segment(
        [
            Pos2::new(glint_center.x, glint_center.y - radius * 0.22),
            Pos2::new(glint_center.x, glint_center.y + radius * 0.22),
        ],
        Stroke::new(2.2, Color32::from_rgba_unmultiplied(255, 255, 255, 205)),
    );
}

pub(super) fn paint_disco_interface(
    context: &egui::Context,
    rect: Rect,
    elapsed: Duration,
    activation_elapsed: Duration,
    enabled: bool,
) {
    let painter = context.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("disco-interface-overlay"),
    ));
    if enabled {
        let t = elapsed.as_secs_f32();
        let flash = disco_flash_envelope(elapsed);
        let pulse_color = disco_ui_color(elapsed, 0.88);
        let pulse_alpha = (10.0 + (t * 1.7).sin().mul_add(0.5, 0.5) * 16.0 + flash * 24.0) as u8;
        painter.rect_filled(
            rect,
            0,
            Color32::from_rgba_unmultiplied(
                pulse_color.r(),
                pulse_color.g(),
                pulse_color.b(),
                pulse_alpha,
            ),
        );
        if flash > 0.0 {
            painter.rect_filled(
                rect,
                0,
                Color32::from_rgba_unmultiplied(255, 255, 255, (flash * 4.0) as u8),
            );
        }

        let ball_center = disco_ball_center(rect, activation_elapsed);
        if flash > 0.0 {
            painter.circle_filled(
                ball_center,
                82.0 + flash * 45.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, (flash * 16.0) as u8),
            );
            for index in 0..20 {
                let angle = index as f32 * std::f32::consts::TAU / 20.0 + t * 0.18;
                let start = Pos2::new(
                    ball_center.x + angle.cos() * 58.0,
                    ball_center.y + angle.sin() * 58.0,
                );
                let end = Pos2::new(
                    ball_center.x + angle.cos() * (95.0 + flash * 65.0),
                    ball_center.y + angle.sin() * (95.0 + flash * 65.0),
                );
                painter.line_segment(
                    [start, end],
                    Stroke::new(
                        1.0 + flash * 1.2,
                        Color32::from_rgba_unmultiplied(255, 255, 255, (flash * 72.0) as u8),
                    ),
                );
            }
        }
        for index in 0..28 {
            let angle = t * 0.48 + index as f32 * std::f32::consts::TAU / 28.0;
            let length = rect.width().max(rect.height()) * 1.2;
            let end = Pos2::new(
                ball_center.x + angle.cos() * length,
                ball_center.y + angle.sin() * length,
            );
            let color = disco_ui_color(elapsed, index as f32 / 28.0);
            painter.line_segment(
                [ball_center, end],
                Stroke::new(
                    1.4 + (index % 4) as f32,
                    Color32::from_rgba_unmultiplied(
                        color.r(),
                        color.g(),
                        color.b(),
                        (72.0 + flash * 32.0) as u8,
                    ),
                ),
            );
        }

        for index in 0..13 {
            let origin_x =
                ball_center.x + (t * 0.31 + index as f32 * 1.4).sin() * rect.width() * 0.012;
            let target_x = rect.left()
                + rect.width() * (index as f32 + 0.5) / 13.0
                + (t * 0.47 + index as f32).cos() * rect.width() * 0.11;
            let half_width = rect.width() * (0.035 + (index % 3) as f32 * 0.012);
            let color = disco_ui_color(elapsed, index as f32 / 13.0);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(origin_x - 3.0, ball_center.y),
                    Pos2::new(origin_x + 3.0, ball_center.y),
                    Pos2::new(target_x + half_width, rect.bottom()),
                    Pos2::new(target_x - half_width, rect.bottom()),
                ],
                Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    (42.0 + flash * 22.0) as u8,
                ),
                Stroke::NONE,
            ));
        }

        let border = rect.shrink(2.0);
        for (index, segment) in [
            [border.left_top(), border.right_top()],
            [border.right_top(), border.right_bottom()],
            [border.right_bottom(), border.left_bottom()],
            [border.left_bottom(), border.left_top()],
        ]
        .into_iter()
        .enumerate()
        {
            painter.line_segment(
                segment,
                Stroke::new(
                    4.0 + (t * 2.0 + index as f32).sin().abs() * 2.0,
                    disco_ui_color(elapsed, index as f32 * 0.19),
                ),
            );
        }

        for ring in 0..5 {
            let color = disco_ui_color(elapsed, ring as f32 * 0.17);
            let radius = 82.0 + ring as f32 * 36.0 + (t * 1.4 + ring as f32).sin() * 9.0;
            painter.circle_stroke(
                ball_center,
                radius,
                Stroke::new(
                    8.0 - ring as f32,
                    Color32::from_rgba_unmultiplied(
                        color.r(),
                        color.g(),
                        color.b(),
                        48_u8.saturating_sub(ring * 6),
                    ),
                ),
            );
        }

        for index in 0..46 {
            let angle = t * (0.52 + (index % 4) as f32 * 0.035) + index as f32 * 2.399_963;
            let distance = 110.0 + (index % 11) as f32 * rect.width().min(rect.height()) * 0.055;
            let position = Pos2::new(
                ball_center.x + angle.cos() * distance,
                ball_center.y + angle.sin() * distance * 0.74,
            );
            let color = disco_ui_color(elapsed, index as f32 / 46.0);
            let pulse = (((t * 2.7 + index as f32 * 0.91).sin() * 0.5 + 0.5).powi(4)
                + flash * if index % 3 == 0 { 0.28 } else { 0.1 })
            .min(1.0);
            painter.circle_filled(
                position,
                9.0 + pulse * 16.0,
                Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    (20.0 + pulse * 46.0) as u8,
                ),
            );
            painter.circle_filled(
                position,
                1.8 + pulse * 3.8,
                Color32::from_rgba_unmultiplied(255, 255, 255, (125.0 + pulse * 130.0) as u8),
            );
        }

        for index in 0..6 {
            let angle = t * 0.19 + index as f32 * std::f32::consts::TAU / 6.0;
            let position = Pos2::new(
                ball_center.x + angle.cos() * rect.width() * 0.42,
                ball_center.y + angle.sin() * rect.height() * 0.46,
            );
            let color = disco_ui_color(elapsed, index as f32 / 6.0);
            painter.circle_filled(
                position,
                rect.width().min(rect.height()) * (0.13 + (index % 2) as f32 * 0.04),
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 34),
            );
        }

        let dance_floor_top = rect.bottom() - 54.0;
        for row in 0..3 {
            for column in 0..18 {
                let tile_width = rect.width() / 18.0;
                let tile = Rect::from_min_max(
                    Pos2::new(
                        rect.left() + column as f32 * tile_width + 1.0,
                        dance_floor_top + row as f32 * 18.0 + 1.0,
                    ),
                    Pos2::new(
                        rect.left() + (column + 1) as f32 * tile_width - 1.0,
                        dance_floor_top + (row + 1) as f32 * 18.0 - 1.0,
                    ),
                );
                let color =
                    disco_ui_color(elapsed, (column as f32 * 0.08 + row as f32 * 0.21).fract());
                let alpha = if (column + row + (t * 4.0) as usize).is_multiple_of(3) {
                    105
                } else {
                    48
                };
                painter.rect_filled(
                    tile,
                    2,
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
                );
            }
        }

        for index in 0..110 {
            let seed = ((index * 73 + 19) % 997) as f32 / 997.0;
            let speed = 0.08 + ((index * 41 + 7) % 100) as f32 / 720.0;
            let travel = (seed + t * speed).fract();
            let direction = if index % 2 == 0 { -1.0 } else { 1.0 };
            let spread = direction * travel * rect.width() * (0.14 + seed * 0.34);
            let sway = (t * (0.8 + seed) + index as f32 * 0.73).sin() * 18.0;
            let center = Pos2::new(
                ball_center.x + spread + sway,
                ball_center.y + 24.0 + travel * (rect.bottom() - ball_center.y + 30.0),
            );
            let angle = t * (1.1 + seed * 2.2) + index as f32;
            let length = 5.0 + ((index * 17) % 9) as f32;
            let delta = egui::vec2(angle.cos() * length, angle.sin() * length);
            let color = disco_ui_color(elapsed, seed);
            painter.line_segment(
                [center - delta, center + delta],
                Stroke::new(
                    2.0 + (index % 3) as f32 * 0.7,
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 220),
                ),
            );
        }

        for index in 0..56 {
            let orbit_angle = t * (0.42 + (index % 5) as f32 * 0.08) + index as f32 * 2.399_963;
            let orbit_radius = 72.0 + (index % 7) as f32 * 31.0;
            let center = Pos2::new(
                ball_center.x + orbit_angle.cos() * orbit_radius,
                ball_center.y + orbit_angle.sin() * orbit_radius * 0.62,
            );
            let pulse = ((t * 2.2 + index as f32 * 1.7).sin() * 0.5 + 0.5).powi(3);
            let radius = 3.0 + pulse * 8.0;
            let color = disco_ui_color(elapsed, index as f32 / 56.0);
            let color = Color32::from_rgba_unmultiplied(
                color.r(),
                color.g(),
                color.b(),
                (55.0 + pulse * 155.0) as u8,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - radius, center.y),
                    Pos2::new(center.x + radius, center.y),
                ],
                Stroke::new(1.3, color),
            );
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - radius),
                    Pos2::new(center.x, center.y + radius),
                ],
                Stroke::new(1.3, color),
            );
        }

        for index in 0..52 {
            let x = rect.left() + ((index * 149 + 43) % 1021) as f32 / 1021.0 * rect.width();
            let y = rect.top() + ((index * 263 + 71) % 1013) as f32 / 1013.0 * rect.height();
            let pulse = ((t * 2.5 + index as f32 * 1.31).sin() * 0.5 + 0.5).powi(5);
            let radius = 1.5 + pulse * 7.5;
            let center = Pos2::new(x, y);
            let color = disco_ui_color(elapsed, index as f32 / 52.0);
            let color = Color32::from_rgba_unmultiplied(
                color.r(),
                color.g(),
                color.b(),
                (60.0 + pulse * 190.0) as u8,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - radius, center.y),
                    Pos2::new(center.x + radius, center.y),
                ],
                Stroke::new(1.5, color),
            );
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - radius),
                    Pos2::new(center.x, center.y + radius),
                ],
                Stroke::new(1.5, color),
            );
        }

        paint_disco_ball(&painter, rect, elapsed, activation_elapsed);
    }
}
