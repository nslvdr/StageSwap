use eframe::egui::Color32;

// Neutral application surfaces. Keep these values achromatic so the chrome can
// change independently from the semantic status colors and the selected blue.
pub(crate) const BACKGROUND: Color32 = Color32::from_rgb(17, 17, 17);
pub(crate) const PANEL: Color32 = Color32::from_rgb(18, 18, 18);
pub(crate) const WINDOW: Color32 = Color32::from_rgb(23, 23, 23);
pub(crate) const SURFACE: Color32 = Color32::from_rgb(27, 27, 27);
pub(crate) const SURFACE_RAISED: Color32 = Color32::from_rgb(30, 30, 30);
pub(crate) const SURFACE_DEEP: Color32 = Color32::from_rgb(24, 24, 24);
pub(crate) const CONTROL: Color32 = Color32::from_rgb(38, 38, 38);
pub(crate) const CONTROL_HOVERED: Color32 = Color32::from_rgb(48, 48, 48);
pub(crate) const CONTROL_ACTIVE: Color32 = Color32::from_rgb(59, 59, 59);
pub(crate) const BORDER: Color32 = Color32::from_rgb(66, 66, 66);
pub(crate) const BORDER_SUBTLE: Color32 = Color32::from_rgb(57, 57, 57);
pub(crate) const SIDEBAR: Color32 = Color32::from_rgb(20, 20, 20);
pub(crate) const SIDEBAR_HOVERED: Color32 = Color32::from_rgb(32, 32, 32);
pub(crate) const SIDEBAR_SELECTED: Color32 = Color32::from_rgb(45, 45, 45);
pub(crate) const PREVIEW_BACKGROUND: Color32 = Color32::from_rgb(12, 12, 12);

pub(crate) const SETUP_BLACK: Color32 = Color32::from_rgb(16, 16, 16);
pub(crate) const SETUP_DECK: Color32 = Color32::from_rgb(26, 26, 26);
pub(crate) const SETUP_WHITE: Color32 = Color32::from_rgb(245, 245, 245);

pub(crate) const TEXT_PRIMARY: Color32 = Color32::from_rgb(224, 224, 224);
pub(crate) const TEXT_SECONDARY: Color32 = Color32::from_rgb(184, 184, 184);
pub(crate) const TEXT_MUTED: Color32 = Color32::from_rgb(145, 145, 145);
pub(crate) const TEXT_SUBTLE: Color32 = Color32::from_rgb(126, 126, 126);
pub(crate) const TEXT_DIM: Color32 = Color32::from_rgb(112, 112, 112);

// Existing interactive colors stay unchanged.
pub(crate) const BLUE: Color32 = Color32::from_rgb(64, 118, 216);
pub(crate) const SELECTED_BLUE: Color32 = Color32::from_rgb(55, 115, 245);

#[cfg(windows)]
pub(crate) const BLUE_RGBA: [u8; 4] = [64, 118, 216, 255];
#[cfg(windows)]
pub(crate) const START_RGBA: [u8; 4] = [48, 174, 105, 255];
#[cfg(windows)]
pub(crate) const STOP_RGBA: [u8; 4] = [220, 76, 76, 255];
