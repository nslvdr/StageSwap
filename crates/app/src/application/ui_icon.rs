use eframe::egui::{self, Align2, Color32, FontId, Rect};
use egui_phosphor::regular;
#[cfg(any(windows, test))]
use fontdue::{Font, FontSettings};
#[cfg(any(windows, test))]
use image::{Rgba, RgbaImage, imageops::FilterType};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum UiIcon {
    Back,
    ArrowRight,
    Bell,
    Broadcast,
    Camera,
    Capture,
    Check,
    CheckCircle,
    Clock,
    Download,
    Error,
    Folder,
    Image,
    Info,
    Layers,
    Load,
    Loader,
    Monitor,
    Play,
    Question,
    Refresh,
    Robot,
    Route,
    Save,
    Settings,
    SignOut,
    Stop,
    Target,
    Trash,
    Unavailable,
    Warning,
    Window,
    Wrench,
}

impl UiIcon {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 33] = [
        Self::Back,
        Self::ArrowRight,
        Self::Bell,
        Self::Broadcast,
        Self::Camera,
        Self::Capture,
        Self::Check,
        Self::CheckCircle,
        Self::Clock,
        Self::Download,
        Self::Error,
        Self::Folder,
        Self::Image,
        Self::Info,
        Self::Layers,
        Self::Load,
        Self::Loader,
        Self::Monitor,
        Self::Play,
        Self::Question,
        Self::Refresh,
        Self::Robot,
        Self::Route,
        Self::Save,
        Self::Settings,
        Self::SignOut,
        Self::Stop,
        Self::Target,
        Self::Trash,
        Self::Unavailable,
        Self::Warning,
        Self::Window,
        Self::Wrench,
    ];

    pub(crate) const fn glyph(self) -> &'static str {
        match self {
            Self::Back => regular::ARROW_LEFT,
            Self::ArrowRight => regular::ARROW_RIGHT,
            Self::Bell => regular::BELL,
            Self::Broadcast => regular::BROADCAST,
            Self::Camera => regular::WEBCAM,
            Self::Capture => regular::SCAN,
            Self::Check => regular::CHECK,
            Self::CheckCircle => regular::CHECK_CIRCLE,
            Self::Clock => regular::CLOCK,
            Self::Download => regular::DOWNLOAD_SIMPLE,
            Self::Error => regular::X_CIRCLE,
            Self::Folder => regular::FOLDER_OPEN,
            Self::Image => regular::IMAGE,
            Self::Info => regular::INFO,
            Self::Layers => regular::STACK,
            Self::Load => regular::FOLDER_OPEN,
            Self::Loader => regular::SPINNER_GAP,
            Self::Monitor => regular::MONITOR,
            Self::Play => regular::PLAY,
            Self::Question => regular::QUESTION,
            Self::Refresh => regular::ARROWS_CLOCKWISE,
            Self::Robot => regular::ROBOT,
            Self::Route => regular::FLOW_ARROW,
            Self::Save => regular::FLOPPY_DISK,
            Self::Settings => regular::GEAR,
            Self::SignOut => regular::SIGN_OUT,
            Self::Stop => regular::STOP,
            Self::Target => regular::TARGET,
            Self::Trash => regular::TRASH,
            Self::Unavailable => regular::PROHIBIT,
            Self::Warning => regular::WARNING,
            Self::Window => regular::APP_WINDOW,
            Self::Wrench => regular::WRENCH,
        }
    }

    #[cfg(any(windows, test))]
    fn character(self) -> char {
        self.glyph()
            .chars()
            .next()
            .expect("Phosphor icon constants contain one character")
    }
}

pub(crate) fn install_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    context.set_fonts(fonts);
}

pub(crate) fn paint(painter: &egui::Painter, rect: Rect, icon: UiIcon, color: Color32) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.glyph(),
        FontId::proportional(rect.width().min(rect.height())),
        color,
    );
}

#[cfg(any(windows, test))]
pub(crate) fn rasterize(icon: UiIcon, color: [u8; 4], size: u32) -> Result<RgbaImage, String> {
    let source_size = size
        .checked_mul(2)
        .ok_or_else(|| "icon size is too large".to_owned())?;
    let font = Font::from_bytes(
        egui_phosphor::Variant::Regular.font_bytes(),
        FontSettings::default(),
    )
    .map_err(|error| format!("could not load the Phosphor icon font: {error}"))?;
    let (metrics, coverage) = font.rasterize(icon.character(), source_size as f32 * 0.72);
    let mut source = RgbaImage::from_pixel(
        source_size,
        source_size,
        Rgba([color[0], color[1], color[2], 0]),
    );
    let left = (source_size.saturating_sub(metrics.width as u32)) / 2;
    let top = (source_size.saturating_sub(metrics.height as u32)) / 2;

    for y in 0..metrics.height {
        for x in 0..metrics.width {
            let alpha = coverage[y * metrics.width + x];
            let tinted_alpha = ((u16::from(alpha) * u16::from(color[3])) / 255) as u8;
            source.put_pixel(
                left + x as u32,
                top + y as u32,
                Rgba([color[0], color[1], color[2], tinted_alpha]),
            );
        }
    }

    Ok(image::imageops::resize(
        &source,
        size,
        size,
        FilterType::Lanczos3,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_installed_egui_font_contains_and_paints_every_icon() {
        let context = egui::Context::default();
        install_fonts(&context);
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            for icon in UiIcon::ALL {
                ui.fonts_mut(|fonts| {
                    assert!(
                        fonts.has_glyph(&FontId::proportional(16.0), icon.character()),
                        "{icon:?}"
                    );
                });
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                paint(ui.painter(), rect, icon, Color32::WHITE);
            }
        });
        assert_eq!(output.shapes.len(), UiIcon::ALL.len());
    }

    #[test]
    fn contract_tray_rasterization_is_antialiased_tinted_and_padded() {
        let color = [64, 118, 216, 255];
        let image = rasterize(UiIcon::Settings, color, 32).unwrap();
        assert_eq!(image.dimensions(), (32, 32));
        assert!(image.pixels().any(|pixel| pixel.0[3] > 0));
        assert!(image.pixels().any(|pixel| (1..=254).contains(&pixel.0[3])));
        assert!(
            image
                .pixels()
                .filter(|pixel| pixel.0[3] > 0)
                .all(|pixel| pixel.0[..3] == color[..3])
        );
        for corner in [[0, 0], [31, 0], [0, 31], [31, 31]] {
            assert_eq!(image.get_pixel(corner[0], corner[1]).0[3], 0);
        }
    }
}
