//! Application startup, shared UI state, and feature coordination.

mod admin;
mod app_icon;
mod dashboard;
mod deployment_payload;
mod dialogs;
mod disco_ui;
mod local_log;
mod notification_ui;
mod notifications;
mod preview_conversion;
mod preview_demo;
mod preview_ui;
mod settings_persistence;
mod settings_ui;
mod setup_guide;
mod setup_ui;
mod ui_icon;
mod ui_theme;
mod update;
mod updates_ui;
mod widgets;

use dialogs::*;
use disco_ui::*;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Vec2,
};
#[cfg(not(windows))]
use preview_demo::*;
use preview_ui::*;
use settings_ui::*;
use setup_ui::*;
use stageswap_app::{CommandDispatch, RuntimeHandle};
use stageswap_core::{
    AdminProfileStatus, AdminProfileStore, AdminRestoreOutcome, AppConfig, AppSnapshot, Command,
    ComponentFailureKind, ConfigLoad, ConfigStore, DetectionState, DeviceState, Frame,
    OutputLayout, OutputMode, RestartTarget, RunState, Source, StillImagePipLayout,
    StillImagePipSize, UpdateChannel, WebcamFailureKind,
};
use stageswap_i18n::{Locale, format_text, text as localized_text};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::thread;
use std::time::{Duration, Instant};
use widgets::*;

const WINDOW_ASPECT_RATIO: f32 = 16.0 / 9.0;
const MIN_WINDOW_HEIGHT: f32 = 600.0;
const WINDOW_TITLE: &str = concat!("StageSwap - v", env!("CARGO_PKG_VERSION"));
const APP_VERSION_LABEL: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const LIVE_RED: Color32 = Color32::from_rgb(235, 90, 90);
const ACTIVE_GREEN: Color32 = Color32::from_rgb(76, 205, 132);
const TRANSITION_AMBER: Color32 = Color32::from_rgb(245, 190, 75);
const PREVIEW_NEUTRAL: Color32 = ui_theme::BORDER_SUBTLE;
const SETTINGS_BLUE: Color32 = ui_theme::BLUE;
const SETTINGS_SWITCH_OFF: Color32 = ui_theme::CONTROL_HOVERED;
const VISIBLE_REFRESH: Duration = Duration::from_nanos(1_000_000_000 / 30);
const HIDDEN_REFRESH: Duration = Duration::from_millis(250);
const DASHBOARD_PREVIEW_TEXTURE_LIMIT: [u32; 2] = [240, 135];
const ENLARGED_PREVIEW_TEXTURE_LIMIT: [u32; 2] = [1280, 720];
const EMBEDDED_PREVIEW_REFRESH: Duration = Duration::from_nanos(1_000_000_000 / 30);
const ENLARGED_PREVIEW_REFRESH: Duration = Duration::from_nanos(1_000_000_000 / 30);
const CINEMA_PEEK_SCALE: f32 = 0.92;
const CINEMA_PEEK_CAPTION_GAP: f32 = 10.0;
const CINEMA_PEEK_CAPTION_HEIGHT: f32 = 16.0;
const SETTINGS_SAVE_DEBOUNCE: Duration = Duration::from_millis(300);
const SETTINGS_ENTRANCE_DURATION: Duration = Duration::from_millis(160);
const SETTINGS_SECTION_DURATION: Duration = Duration::from_millis(120);
const DIALOG_ENTRANCE_DURATION: Duration = Duration::from_millis(150);
const SETUP_GUIDE_ENTRANCE_DURATION: Duration = Duration::from_millis(180);
const SETUP_GUIDE_STEP_DURATION: Duration = Duration::from_millis(150);
const SETUP_GUIDE_CONTENT_WIDTH: f32 = 920.0;
const SETUP_GUIDE_FOOTER_HEIGHT: f32 = 72.0;
const SETUP_DEMO_LOOP_DURATION: f32 = 7.8;
const SETUP_DEMO_HOLD_DURATION: f32 = 3.5;
const SETUP_DEMO_FADE_DURATION: f32 = 0.4;
const SETUP_HARDWARE_PREVIEW_WIDTH: f32 = 420.0;
const SETUP_BOOTH_BLACK: Color32 = ui_theme::SETUP_BLACK;
const SETUP_SIGNAL_DECK: Color32 = ui_theme::SETUP_DECK;
const SETUP_SIGNAL_WHITE: Color32 = ui_theme::SETUP_WHITE;
const REFERENCE_CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
const SETUP_REFERENCE_FLASH_DURATION: Duration = Duration::from_millis(120);
const SETUP_REFERENCE_CARD_HEIGHT: f32 = 262.0;
const SETUP_REFERENCE_COLUMN_GAP: f32 = 16.0;
const REFERENCE_DIALOG_WIDTH: f32 = 720.0;
const REFERENCE_DIALOG_STACK_BREAKPOINT: f32 = 600.0;
const DISCO_GESTURE_WINDOW: Duration = Duration::from_secs(3);
const SETTINGS_SIDEBAR_WIDTH: f32 = 228.0;
const SETTINGS_PREVIEW_WIDTH: f32 = 480.0;
const SETTINGS_CONTENT_WIDTH: f32 = SETTINGS_PREVIEW_WIDTH;
const SETTINGS_PREVIEW_HEIGHT: f32 = 270.0;
const SETTINGS_SIDEBAR_FILL: Color32 = ui_theme::SIDEBAR;
const SETTINGS_SIDEBAR_CORNER_RADIUS: u8 = 12;
const SETTINGS_NAV_HOVERED: Color32 = ui_theme::SIDEBAR_HOVERED;
const SETTINGS_NAV_SELECTED: Color32 = ui_theme::SIDEBAR_SELECTED;
const SETTINGS_NAV_INDICATOR: Color32 = ui_theme::TEXT_MUTED;
const NOTIFICATION_POPOVER_WIDTH: f32 = 356.0;
const NOTIFICATION_TOAST_WIDTH: f32 = 372.0;
const NOTIFICATION_POPOVER_SPACING: f32 = 8.0;
const NOTIFICATION_POPOVER_HEADER_HEIGHT: f32 = 22.0;
#[cfg(not(windows))]
const UI_PREVIEW_NOTIFICATION_INTERVAL: Duration = Duration::from_secs(30);

#[cfg(windows)]
mod tray;
use local_log::LocalLog;
use notifications::{NotificationCenter, NotificationItem, NotificationSource, NotificationTone};
use preview_conversion::{PreviewConverter, PreviewFrameId};
use setup_guide::{
    SetupReturnView, SetupSession, SetupStartup, SetupStateStore, SetupStep, has_existing_user_data,
};
use ui_icon::UiIcon;
use update::UpdateStatus;
#[cfg(windows)]
use update::{ReleaseVersion, UpdateNotificationState, UpdateRequest, UpdateResult, UpdateWorker};

pub(crate) fn run() -> eframe::Result {
    let _embedded_payload = deployment_payload::bytes();
    #[cfg(windows)]
    let startup_locale = stageswap_windows::preferred_interface_locale();
    #[cfg(not(windows))]
    let ui_arguments = std::env::args().collect::<Vec<_>>();
    #[cfg(not(windows))]
    let ui_preview_request = match parse_ui_preview_request(&ui_arguments) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("StageSwap UI preview: {error}");
            eprintln!(
                "Usage: StageSwap --ui-preview [general|webcam|screen|matching|updates|diagnostics|notifications[-empty|-critical|-updates]|setup-1..5|dialog-*] [--ui-language en-US|fr-FR|es] [--ui-setup-reference-state captured|empty|review|missing-screen]"
            );
            return Ok(());
        }
    };
    #[cfg(not(windows))]
    let ui_screenshot_path = match parse_ui_screenshot_path(&ui_arguments) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("StageSwap UI preview: {error}");
            return Ok(());
        }
    };
    #[cfg(not(windows))]
    let setup_demo_preview_state = match parse_setup_demo_preview_state(&ui_arguments) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("StageSwap UI preview: {error}");
            return Ok(());
        }
    };
    #[cfg(not(windows))]
    let setup_reference_preview_state = match parse_setup_reference_preview_state(&ui_arguments) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("StageSwap UI preview: {error}");
            return Ok(());
        }
    };
    #[cfg(windows)]
    let launch_context =
        match stageswap_windows::portable_bootstrap(_embedded_payload, startup_locale) {
            Ok(stageswap_windows::BootstrapResult::Continue(context)) => context,
            Ok(stageswap_windows::BootstrapResult::Exit) => return Ok(()),
            Err(error) => {
                stageswap_windows::show_error_dialog(
                    startup_locale,
                    "StageSwap installation failed",
                    &error,
                );
                return Ok(());
            }
        };
    #[cfg(windows)]
    let deployment_command = matches!(
        std::env::args().nth(1).as_deref(),
        Some("--register-elevated" | "--cleanup-elevated" | "--cleanup")
    );
    #[cfg(windows)]
    if deployment_command {
        match stageswap_windows::deployment_startup(_embedded_payload) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) => deployment_failure(startup_locale, &error),
        }
    }
    #[cfg(windows)]
    let _single_instance = match stageswap_windows::SingleInstance::acquire() {
        Ok(Some(instance)) => instance,
        Ok(None) => {
            if std::env::args().nth(1).as_deref() != Some("--startup")
                && let Err(error) = stageswap_windows::send_instance_command(
                    stageswap_windows::InstanceCommand::Show,
                )
            {
                stageswap_windows::show_error_dialog(
                    startup_locale,
                    "StageSwap is already running",
                    &format!(
                        "Its window could not be opened. Exit the legacy tray instance and try again.\n\n{error}"
                    ),
                );
            }
            return Ok(());
        }
        Err(error) => {
            stageswap_windows::show_error_dialog(
                startup_locale,
                "StageSwap could not start",
                &error,
            );
            return Ok(());
        }
    };
    #[cfg(windows)]
    if !deployment_command {
        match stageswap_windows::deployment_startup(_embedded_payload) {
            Ok(false) => {}
            Ok(true) => return Ok(()),
            Err(error) => deployment_failure(startup_locale, &error),
        }
    }
    #[cfg(windows)]
    let (instance_sender, instance_receiver) = std::sync::mpsc::channel();
    #[cfg(windows)]
    let _instance_control = match stageswap_windows::InstanceControl::start(instance_sender) {
        Ok(control) => control,
        Err(error) => {
            stageswap_windows::show_error_dialog(
                startup_locale,
                "StageSwap could not start its local control service",
                &error,
            );
            return Ok(());
        }
    };
    #[cfg(windows)]
    let instance_readiness = _instance_control.readiness();
    #[cfg(windows)]
    let store = ConfigStore::new(local_data_directory());
    #[cfg(not(windows))]
    let store = ConfigStore::new(if ui_preview_request.is_some() {
        std::env::temp_dir().join(format!("StageSwap-ui-preview-{}", std::process::id()))
    } else {
        local_data_directory()
    });
    let existing_install = has_existing_user_data(store.directory());
    #[cfg(windows)]
    let setup_startup = SetupStateStore::new(store.directory()).initialize(existing_install);
    #[cfg(not(windows))]
    let setup_startup = if ui_preview_request.is_some() {
        SetupStartup::suppressed()
    } else {
        SetupStateStore::new(store.directory()).initialize(existing_install)
    };
    let show_first_run_setup = setup_startup.show_setup_guide;
    #[cfg(windows)]
    let loaded = load_config_with_admin_restore(&store);
    #[cfg(not(windows))]
    let loaded = if ui_preview_request.is_some() {
        ConfigLoad {
            config: ui_preview_config(),
            used_defaults: false,
            ..ConfigLoad::default()
        }
    } else {
        load_config_with_admin_restore(&store)
    };
    #[cfg(windows)]
    let mut loaded = loaded;
    #[cfg(windows)]
    if loaded.used_defaults {
        loaded.config.interface_language = startup_locale.tag().into();
        if let Err(error) = stageswap_windows::save_config_atomic(&store, &loaded.config) {
            loaded.warnings.push(format!(
                "Could not save the initial interface language: {error}"
            ));
        }
    }
    #[cfg(windows)]
    if launch_context.mode == stageswap_windows::PortableMode::Managed
        && let Err(error) = stageswap_windows::configure_startup(loaded.config.start_with_windows)
    {
        loaded.warnings.push(format!(
            "Could not reconcile the Windows startup preference: {error}"
        ));
    }
    #[cfg(windows)]
    let start_visible =
        show_first_run_setup || launch_context.force_visible || !loaded.config.start_minimized;
    #[cfg(not(windows))]
    let start_visible =
        show_first_run_setup || ui_preview_request.is_some() || !loaded.config.start_minimized;
    let app_icon = app_icon::load(None).expect("embedded app icon should decode");
    #[cfg(windows)]
    let renderer = eframe::Renderer::Wgpu;
    #[cfg(not(windows))]
    let renderer = if ui_screenshot_path.is_some() {
        eframe::Renderer::Glow
    } else {
        eframe::Renderer::Wgpu
    };
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([MIN_WINDOW_HEIGHT * WINDOW_ASPECT_RATIO, MIN_WINDOW_HEIGHT])
            .with_visible(start_visible)
            .with_icon(egui::IconData {
                rgba: app_icon.rgba,
                width: app_icon.width,
                height: app_icon.height,
            }),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(move |context| {
            ui_icon::install_fonts(&context.egui_ctx);
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = ui_theme::PANEL;
            visuals.window_fill = ui_theme::WINDOW;
            visuals.selection.bg_fill = ui_theme::SELECTED_BLUE;
            visuals.widgets.inactive.corner_radius = 6.into();
            visuals.widgets.hovered.corner_radius = 6.into();
            visuals.widgets.active.corner_radius = 6.into();
            visuals.widgets.inactive.bg_fill = ui_theme::CONTROL;
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, ui_theme::BORDER);
            visuals.widgets.hovered.bg_fill = ui_theme::CONTROL_HOVERED;
            visuals.widgets.active.bg_fill = ui_theme::CONTROL_ACTIVE;
            context.egui_ctx.set_visuals(visuals);
            context.egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
                style.spacing.item_spacing = egui::vec2(10.0, 10.0);
                style.spacing.button_padding = egui::vec2(14.0, 8.0);
            });
            let app = SwitcherApp::new(loaded.config, loaded.warnings, store)
                .with_setup_startup(setup_startup)
                .with_initial_window_visibility(start_visible);
            #[cfg(not(windows))]
            let app = if let Some(request) = ui_preview_request {
                app.with_ui_preview(request)
            } else {
                app
            };
            #[cfg(not(windows))]
            let app = if let Some(path) = ui_screenshot_path.clone() {
                app.with_ui_screenshot(path)
            } else {
                app
            };
            #[cfg(not(windows))]
            let app = app.with_setup_demo_preview_state(setup_demo_preview_state);
            #[cfg(not(windows))]
            let app = app.with_setup_reference_preview_state(setup_reference_preview_state);
            #[cfg(windows)]
            let mut app =
                app.with_launch_context(launch_context.mode, instance_receiver, instance_readiness);
            #[cfg(windows)]
            if let Some(visible) = initial_visibility_override(start_visible, app.tray.is_some()) {
                // eframe shows the root window after its first render even when the
                // viewport builder starts hidden. A viewport command is applied
                // afterwards, so reassert tray-hidden startup here.
                context
                    .egui_ctx
                    .send_viewport_cmd(egui::ViewportCommand::Visible(visible));
                app.window_visible = visible;
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(windows)]
fn deployment_failure(locale: Locale, error: &str) -> ! {
    stageswap_windows::show_error_dialog(locale, "StageSwap deployment failed", error);
    eprintln!("StageSwap deployment failed: {error}");
    std::process::exit(1);
}

#[cfg(any(windows, test))]
fn initial_visibility_override(start_visible: bool, tray_available: bool) -> Option<bool> {
    (!start_visible).then_some(!tray_available)
}

fn local_data_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
        .join("StageSwap")
}

fn load_config_with_admin_restore(store: &ConfigStore) -> ConfigLoad {
    let mut restore_warnings = Vec::new();
    let admin_store = AdminProfileStore::new(store.directory());
    if let Err(error) = restore_admin_profile(&admin_store) {
        restore_warnings.push(format!(
            "Could not restore the admin configuration: {error}"
        ));
    }
    let mut loaded = store.load();
    restore_warnings.append(&mut loaded.warnings);
    loaded.warnings = restore_warnings;
    loaded
}

#[cfg(windows)]
fn restore_admin_profile(store: &AdminProfileStore) -> std::io::Result<AdminRestoreOutcome> {
    store.restore_on_launch_with_replace(stageswap_windows::replace_file_atomic)
}

#[cfg(not(windows))]
fn restore_admin_profile(store: &AdminProfileStore) -> std::io::Result<AdminRestoreOutcome> {
    store.restore_on_launch()
}

#[cfg(windows)]
fn restore_admin_profile_now(store: &AdminProfileStore) -> std::io::Result<AdminRestoreOutcome> {
    store.restore_now_with_replace(stageswap_windows::replace_file_atomic)
}

#[cfg(not(windows))]
fn restore_admin_profile_now(store: &AdminProfileStore) -> std::io::Result<AdminRestoreOutcome> {
    store.restore_now()
}

fn aspect_locked_window_size(current: Vec2, previous: Option<Vec2>) -> Vec2 {
    let preserve_width = previous
        .is_none_or(|previous| (current.x - previous.x).abs() >= (current.y - previous.y).abs());
    let mut desired = if preserve_width {
        egui::vec2(current.x, current.x / WINDOW_ASPECT_RATIO)
    } else {
        egui::vec2(current.y * WINDOW_ASPECT_RATIO, current.y)
    };
    if desired.y < MIN_WINDOW_HEIGHT {
        desired = egui::vec2(MIN_WINDOW_HEIGHT * WINDOW_ASPECT_RATIO, MIN_WINDOW_HEIGHT);
    }
    desired
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SettingsTab {
    General,
    Webcam,
    Screen,
    Matching,
    Updates,
    Diagnostics,
}

impl SettingsTab {
    #[cfg(all(test, not(windows)))]
    const ALL: [Self; 6] = [
        Self::General,
        Self::Webcam,
        Self::Screen,
        Self::Matching,
        Self::Updates,
        Self::Diagnostics,
    ];

    const PRIMARY: [(Self, UiIcon); 4] = [
        (Self::General, UiIcon::Settings),
        (Self::Webcam, UiIcon::Camera),
        (Self::Screen, UiIcon::Monitor),
        (Self::Matching, UiIcon::Target),
    ];

    const UPDATES: (Self, UiIcon) = (Self::Updates, UiIcon::Download);

    const DIAGNOSTICS: (Self, UiIcon) = (Self::Diagnostics, UiIcon::Layers);

    const fn icon(self) -> UiIcon {
        match self {
            Self::General => UiIcon::Settings,
            Self::Webcam => UiIcon::Camera,
            Self::Screen => UiIcon::Monitor,
            Self::Matching => UiIcon::Target,
            Self::Updates => UiIcon::Download,
            Self::Diagnostics => UiIcon::Layers,
        }
    }

    fn title(self, locale: Locale) -> std::borrow::Cow<'static, str> {
        localized_text(
            locale,
            match self {
                Self::General => "General",
                Self::Webcam => "Webcam",
                Self::Screen => "Secondary screen",
                Self::Matching => "Reference image",
                Self::Updates => "Updates",
                Self::Diagnostics => "Diagnostics",
            },
        )
    }

    fn description(self, locale: Locale) -> std::borrow::Cow<'static, str> {
        localized_text(
            locale,
            match self {
                Self::General => "Choose how StageSwap starts, stays open, and alerts you.",
                Self::Webcam => {
                    "Choose the webcam Zoom sees when JW Library is not playing media. Output is always 16:9."
                }
                Self::Screen => {
                    "Choose the secondary screen JW Library uses for presentations. StageSwap watches it for media."
                }
                Self::Matching => {
                    "Capture the screen JW Library shows when no media is playing. StageSwap compares the live screen with it to detect media."
                }
                Self::Updates => {
                    "Check for new StageSwap versions and choose when to install them."
                }
                Self::Diagnostics => {
                    "Check video connections, troubleshoot problems, and view logs."
                }
            },
        )
    }
}

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UiPreviewRequest {
    target: UiPreviewTarget,
}

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UiPreviewTarget {
    UpdateVersions,
    Settings(SettingsTab),
    Setup(SetupStep),
    Dialog(AppDialogKind),
    Notifications(NotificationPreviewState),
}

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NotificationPreviewState {
    Empty,
    Critical,
    Updates,
    Stacked,
}

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetupDemoPreviewState {
    Matching,
    Changed,
}

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetupReferencePreviewState {
    Captured,
    Empty,
    Review,
    MissingScreen,
}

#[cfg(not(windows))]
impl SettingsTab {
    #[cfg(test)]
    const fn preview_name(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Webcam => "webcam",
            Self::Screen => "screen",
            Self::Matching => "matching",
            Self::Updates => "updates",
            Self::Diagnostics => "diagnostics",
        }
    }

    fn from_preview_name(value: &str) -> Option<Self> {
        match value {
            "general" => Some(Self::General),
            "webcam" => Some(Self::Webcam),
            "screen" => Some(Self::Screen),
            "matching" => Some(Self::Matching),
            "updates" => Some(Self::Updates),
            "diagnostics" => Some(Self::Diagnostics),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AppView {
    #[default]
    Dashboard,
    Settings,
    SetupGuide,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum AppDialogKind {
    Exit,
    ClearLogs,
    ReferenceCapture,
    Admin,
    ReplaceAdminBaseline,
    LoadAdminConfig,
    RemoveAdminBaseline,
}

#[derive(Clone, Copy, Debug)]
struct ActiveDialog {
    kind: AppDialogKind,
    opened_at: Instant,
    focus_safe_action: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DialogAction {
    Dismiss,
    CancelReferenceCapture,
    RetakeReferenceCapture,
    ConfirmReferenceCandidate,
    Exit,
    ClearLogs,
    SaveAdminBaseline,
    ReplaceAdminBaseline,
    LoadAdminConfig,
    RemoveAdminBaseline,
    SetAdminAutoRestore(bool),
}

#[derive(Default)]
struct DiscoDiagnosticsGesture {
    first_click_at: Option<Instant>,
    click_count: u8,
}

impl DiscoDiagnosticsGesture {
    fn register_primary_click(&mut self, now: Instant) -> bool {
        if self
            .first_click_at
            .is_none_or(|started| now.saturating_duration_since(started) > DISCO_GESTURE_WINDOW)
        {
            self.first_click_at = Some(now);
            self.click_count = 1;
            return false;
        }
        self.click_count += 1;
        if self.click_count < 5 {
            return false;
        }
        self.reset();
        true
    }

    fn reset(&mut self) {
        self.first_click_at = None;
        self.click_count = 0;
    }
}

struct PreviewTexture {
    source: PreviewFrameId,
    size: [usize; 2],
    texture: TextureHandle,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PreviewKind {
    Webcam,
    Screen,
    Reference,
    Output,
}

impl PreviewKind {
    const fn key(self) -> &'static str {
        match self {
            Self::Webcam => "webcam",
            Self::Screen => "screen",
            Self::Reference => "reference",
            Self::Output => "output",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Webcam => "WEBCAM",
            Self::Screen => "SECONDARY SCREEN",
            Self::Reference => "REFERENCE IMAGE",
            Self::Output => "ZOOM OUTPUT",
        }
    }

    const fn icon(self) -> UiIcon {
        match self {
            Self::Webcam => UiIcon::Camera,
            Self::Screen => UiIcon::Monitor,
            Self::Reference => UiIcon::Image,
            Self::Output => UiIcon::Broadcast,
        }
    }

    const fn shows_fps(self) -> bool {
        !matches!(self, Self::Reference)
    }

    fn pipeline_fps(self, snapshot: &AppSnapshot) -> Option<u32> {
        match self {
            Self::Webcam => snapshot.webcam_fps,
            Self::Screen => snapshot.screen_fps,
            Self::Reference => None,
            Self::Output => snapshot.output_fps,
        }
    }

    const fn empty_message(self) -> &'static str {
        match self {
            Self::Webcam => "No webcam frame",
            Self::Screen => "No secondary screen frame",
            Self::Reference => "No reference image",
            Self::Output => "No Zoom output frame",
        }
    }

    fn frame(self, snapshot: &AppSnapshot) -> Option<&Arc<Frame>> {
        match self {
            Self::Webcam => snapshot.previews.webcam.as_ref(),
            Self::Screen => snapshot.previews.screen.as_ref(),
            Self::Reference => snapshot.previews.reference.as_ref(),
            Self::Output => snapshot.previews.final_output.as_ref(),
        }
    }
}

#[derive(Clone, Copy)]
struct PreviewOptions {
    show_fps: bool,
    fps: Option<u32>,
    empty_message: &'static str,
    texture_limit: [u32; 2],
    refresh_interval: Duration,
    style: PreviewStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewStyle {
    Card,
    CinemaPeek,
}

#[derive(Clone, Copy, Debug)]
struct SettingsPreviewControls {
    preview: Rect,
    controls: Rect,
}

#[derive(Debug)]
struct SettingsSidebarLayout {
    brand_icon: Rect,
    brand_title: Rect,
    brand_version: Rect,
    brand_separator: Rect,
    back: Rect,
    primary_navigation: Vec<Rect>,
    updates: Rect,
    diagnostics: Rect,
    go_back: bool,
}

#[derive(Debug)]
#[allow(dead_code)]
struct DashboardControlsLayout {
    sections: [Rect; 3],
    section_headings: [Rect; 3],
    health_indicators: [Rect; 5],
    section_dividers: [Rect; 2],
    other_actions: [Rect; 2],
}

#[derive(Debug)]
struct ControlsWorkspaceLayout {
    body: DashboardControlsLayout,
    footer: Rect,
}

#[derive(Clone, Copy, Debug)]
struct PreviewGridLayout {
    grid: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetupAction {
    Previous,
    Next,
    GoTo(SetupStep),
    Close,
}

#[derive(Clone, Copy, Debug)]
enum SetupReferenceCaptureState {
    Idle,
    PreparingCandidate {
        started_at: Instant,
    },
    CapturingCandidate {
        started_at: Instant,
        previous_candidate_sequence: Option<u64>,
    },
    Review {
        captured_at: Instant,
    },
    SavingCandidate {
        started_at: Instant,
        previous_reference_sequence: Option<u64>,
    },
    Confirmed,
    CaptureFailed,
    SaveFailed {
        previous_reference_sequence: Option<u64>,
    },
}

#[derive(Clone)]
struct SetupExampleTextures {
    reference: TextureHandle,
    webcam: TextureHandle,
    screen: TextureHandle,
}

#[derive(Clone, Copy)]
struct SettingsPreview<'a> {
    kind: PreviewKind,
    frame: Option<&'a Arc<Frame>>,
    label: &'a str,
    empty_message: &'static str,
    actual_output: Source,
}

impl PreviewOptions {
    const fn dashboard(kind: PreviewKind, fps: Option<u32>) -> Self {
        Self {
            show_fps: kind.shows_fps(),
            fps,
            empty_message: kind.empty_message(),
            texture_limit: DASHBOARD_PREVIEW_TEXTURE_LIMIT,
            refresh_interval: EMBEDDED_PREVIEW_REFRESH,
            style: PreviewStyle::Card,
        }
    }

    const fn enlarged(kind: PreviewKind, fps: Option<u32>) -> Self {
        Self {
            show_fps: kind.shows_fps(),
            fps,
            empty_message: kind.empty_message(),
            texture_limit: ENLARGED_PREVIEW_TEXTURE_LIMIT,
            refresh_interval: ENLARGED_PREVIEW_REFRESH,
            style: PreviewStyle::CinemaPeek,
        }
    }

    const fn settings(empty_message: &'static str) -> Self {
        Self {
            show_fps: false,
            fps: None,
            empty_message,
            texture_limit: DASHBOARD_PREVIEW_TEXTURE_LIMIT,
            refresh_interval: EMBEDDED_PREVIEW_REFRESH,
            style: PreviewStyle::Card,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewContour {
    Neutral,
    Active,
    Live,
}

const SETTINGS_RECOVERY_TARGETS: [(UiIcon, &str, f32, RestartTarget); 4] = [
    (
        UiIcon::Camera,
        "Restart webcam",
        176.0,
        RestartTarget::Webcam,
    ),
    (
        UiIcon::Monitor,
        "Restart screen capture",
        218.0,
        RestartTarget::ScreenCapture,
    ),
    (
        UiIcon::Broadcast,
        "Restart virtual camera",
        226.0,
        RestartTarget::VirtualCamera,
    ),
    (UiIcon::Layers, "Restart all", 154.0, RestartTarget::All),
];

#[cfg(not(windows))]
struct UiPreviewSession {
    snapshot: AppSnapshot,
    next_notification_at: Option<Instant>,
    notification_count: u32,
}

#[cfg(not(windows))]
struct UiScreenshotCapture {
    path: PathBuf,
    frames_until_request: u8,
    requested: bool,
}

struct SwitcherApp {
    config: AppConfig,
    runtime: RuntimeHandle,
    store: ConfigStore,
    setup_store: SetupStateStore,
    setup_session: Option<SetupSession>,
    setup_reference_capture: SetupReferenceCaptureState,
    load_warnings: Vec<String>,
    view: AppView,
    settings_tab: SettingsTab,
    pending_settings_save: Option<Instant>,
    settings_writer: Option<settings_persistence::SettingsPersistence>,
    settings_save_error: Option<String>,
    admin_profile_status: Option<AdminProfileStatus>,
    active_dialog: Option<ActiveDialog>,
    awaiting_video_device_id: Option<String>,
    awaiting_monitor_label: Option<String>,
    settings_opened_at: Option<Instant>,
    settings_section_changed_at: Option<Instant>,
    app_icon_texture: Option<TextureHandle>,
    setup_example_textures: Option<SetupExampleTextures>,
    previews: preview_ui::PreviewState,
    render_snapshot: Option<Arc<AppSnapshot>>,
    log: LocalLog,
    last_activity_id: u64,
    notifications: NotificationCenter,
    notification_center_open: bool,
    updates: updates_ui::UpdateUiState,
    #[cfg(windows)]
    tray: Option<tray::Tray>,
    #[cfg(windows)]
    portable_mode: stageswap_windows::PortableMode,
    #[cfg(windows)]
    instance_commands: Option<std::sync::mpsc::Receiver<stageswap_windows::InstanceCommand>>,
    #[cfg(windows)]
    instance_readiness: Option<stageswap_windows::InstanceReadiness>,
    #[cfg(not(windows))]
    ui_preview: Option<UiPreviewSession>,
    #[cfg(not(windows))]
    ui_screenshot: Option<UiScreenshotCapture>,
    #[cfg(not(windows))]
    setup_demo_preview_state: Option<SetupDemoPreviewState>,
    setup_animations_enabled: bool,
    window_visible: bool,
    exit_requested: bool,
    last_window_size: Option<Vec2>,
    disco_diagnostics_gesture: DiscoDiagnosticsGesture,
    disco_ui_activated_at: Option<Instant>,
    ui_animation_started_at: Instant,
}

impl SwitcherApp {
    fn new(mut config: AppConfig, mut load_warnings: Vec<String>, store: ConfigStore) -> Self {
        config.interface_language = Locale::from_tag(&config.interface_language)
            .unwrap_or_default()
            .tag()
            .into();
        #[cfg(windows)]
        let locale = Locale::from_tag(&config.interface_language).unwrap_or_default();
        if config.reference_image_path.is_empty() {
            config.reference_image_path = store.reference_path().display().to_string();
        }
        let admin_profile_status = match AdminProfileStore::new(store.directory()).status() {
            Ok(status) => status,
            Err(error) => {
                let warning = format!("Could not read the admin configuration: {error}");
                if !load_warnings
                    .iter()
                    .any(|warning| warning.contains("admin configuration"))
                {
                    load_warnings.push(warning);
                }
                None
            }
        };
        let log = LocalLog::new(store.logs_path(), 14);
        #[cfg(windows)]
        let update_notifications = UpdateNotificationState::load(store.directory());
        #[cfg(windows)]
        let update_worker = UpdateWorker::start(
            store.directory().to_owned(),
            ReleaseVersion::parse(env!("CARGO_PKG_VERSION"))
                .expect("Cargo package version is numeric semantic versioning"),
        )
        .map_err(|error| {
            load_warnings.push(error);
        })
        .ok();
        let settings_store = store.clone();
        let settings_writer = settings_persistence::SettingsPersistence::start(move |config| {
            save_config(&settings_store, config)
                .map_err(|error| format!("Could not save settings: {error}"))
        })
        .map_err(|error| load_warnings.push(error))
        .ok();
        let notification_now = Instant::now();
        let mut notifications = NotificationCenter::default();
        for warning in &load_warnings {
            log.write("warning", "configuration", "LOAD_WARNING", warning);
            notifications.push_critical(
                NotificationSource::Configuration,
                warning.clone(),
                notification_now,
                config.show_notifications,
            );
        }
        Self {
            runtime: RuntimeHandle::spawn(config.clone()),
            config,
            setup_store: SetupStateStore::new(store.directory()),
            store,
            setup_session: None,
            setup_reference_capture: SetupReferenceCaptureState::Idle,
            load_warnings,
            view: AppView::Dashboard,
            settings_tab: SettingsTab::General,
            pending_settings_save: None,
            settings_writer,
            settings_save_error: None,
            admin_profile_status,
            active_dialog: None,
            awaiting_video_device_id: None,
            awaiting_monitor_label: None,
            settings_opened_at: None,
            settings_section_changed_at: None,
            app_icon_texture: None,
            setup_example_textures: None,
            previews: preview_ui::PreviewState::default(),
            render_snapshot: None,
            log,
            last_activity_id: 0,
            notifications,
            notification_center_open: false,
            updates: updates_ui::UpdateUiState {
                status: UpdateStatus::Idle,
                versions: update::VersionCatalog::Idle,
                selected_version: None,
                picker_open: false,
                #[cfg(windows)]
                notifications: update_notifications,
                check_started: false,
                #[cfg(windows)]
                worker: update_worker,
            },
            #[cfg(windows)]
            tray: tray::Tray::new(locale).ok(),
            #[cfg(windows)]
            portable_mode: stageswap_windows::PortableMode::Managed,
            #[cfg(windows)]
            instance_commands: None,
            #[cfg(windows)]
            instance_readiness: None,
            #[cfg(not(windows))]
            ui_preview: None,
            #[cfg(not(windows))]
            ui_screenshot: None,
            #[cfg(not(windows))]
            setup_demo_preview_state: None,
            #[cfg(windows)]
            setup_animations_enabled: stageswap_windows::client_area_animations_enabled(),
            #[cfg(not(windows))]
            setup_animations_enabled: true,
            window_visible: true,
            exit_requested: false,
            last_window_size: None,
            disco_diagnostics_gesture: DiscoDiagnosticsGesture::default(),
            disco_ui_activated_at: None,
            ui_animation_started_at: Instant::now(),
        }
    }

    fn locale(&self) -> Locale {
        Locale::from_tag(&self.config.interface_language).unwrap_or_default()
    }

    fn with_initial_window_visibility(mut self, visible: bool) -> Self {
        self.window_visible = visible;
        self
    }

    #[cfg(windows)]
    fn show_window(&mut self, context: &egui::Context) {
        self.window_visible = true;
        context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        context.send_viewport_cmd(egui::ViewportCommand::Focus);
        context.request_repaint();
    }

    fn hide_window(&mut self, context: &egui::Context) {
        self.window_visible = false;
        self.previews.enlarged = None;
        self.render_snapshot = None;
        for converter in self.previews.converters.values() {
            converter.suspend();
        }
        context.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        context.request_repaint_after(HIDDEN_REFRESH);
    }

    fn push_notification_warning(&mut self, source: NotificationSource, message: String) {
        self.notifications.push_critical(
            source,
            message.clone(),
            Instant::now(),
            self.config.show_notifications,
        );
        self.load_warnings.push(message);
    }

    fn with_setup_startup(mut self, startup: SetupStartup) -> Self {
        for warning in startup.warnings {
            self.log
                .write("warning", "setup", "SETUP_STATE_WARNING", &warning);
            self.push_notification_warning(NotificationSource::Startup, warning);
        }
        if startup.show_setup_guide {
            self.view = AppView::SetupGuide;
            self.setup_session = Some(SetupSession::live(SetupReturnView::Dashboard));
        }
        self
    }

    #[cfg(windows)]
    fn with_launch_context(
        mut self,
        portable_mode: stageswap_windows::PortableMode,
        instance_commands: std::sync::mpsc::Receiver<stageswap_windows::InstanceCommand>,
        instance_readiness: stageswap_windows::InstanceReadiness,
    ) -> Self {
        self.portable_mode = portable_mode;
        self.instance_commands = Some(instance_commands);
        self.instance_readiness = Some(instance_readiness);
        self
    }

    fn snapshot(&self) -> Arc<AppSnapshot> {
        if let Some(snapshot) = &self.render_snapshot {
            return Arc::clone(snapshot);
        }
        #[cfg(not(windows))]
        if let Some(preview) = &self.ui_preview {
            return Arc::new(preview.snapshot.clone());
        }
        Arc::new(self.runtime.snapshot())
    }

    fn send(&mut self, command: Command) -> bool {
        let dispatch = self.runtime.send(command);
        self.handle_command_dispatch(dispatch)
    }

    fn handle_command_dispatch(&mut self, dispatch: CommandDispatch) -> bool {
        match dispatch {
            CommandDispatch::Queued | CommandDispatch::Coalesced => true,
            CommandDispatch::Busy => {
                let message = "StageSwap is busy. Please try again.".to_owned();
                self.log
                    .write("warning", "runtime", "COMMAND_QUEUE_BUSY", &message);
                self.notifications.push_critical(
                    NotificationSource::Command,
                    message,
                    Instant::now(),
                    self.config.show_notifications,
                );
                false
            }
            CommandDispatch::Closed => {
                if !self.exit_requested {
                    let message = "StageSwap runtime is unavailable.".to_owned();
                    self.log
                        .write("warning", "runtime", "COMMAND_QUEUE_CLOSED", &message);
                    self.notifications.push_critical(
                        NotificationSource::Command,
                        message,
                        Instant::now(),
                        self.config.show_notifications,
                    );
                }
                false
            }
        }
    }

    fn send_setup_reference_command(&mut self, command: Command) -> bool {
        #[cfg(not(windows))]
        if let Some(preview) = self.ui_preview.as_mut() {
            match command {
                Command::CaptureReferenceCandidate => {
                    let next_sequence = [
                        preview.snapshot.previews.screen.as_ref(),
                        preview.snapshot.previews.reference.as_ref(),
                        preview.snapshot.previews.reference_candidate.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .map(|frame| frame.sequence)
                    .max()
                    .unwrap_or_default()
                    .wrapping_add(1)
                    .max(1);
                    preview.snapshot.previews.reference_candidate = preview
                        .snapshot
                        .previews
                        .screen
                        .as_ref()
                        .and_then(|frame| clone_ui_preview_frame(frame, next_sequence));
                }
                Command::ConfirmReferenceCandidate => {
                    if let Some(candidate) = preview.snapshot.previews.reference_candidate.take() {
                        preview.snapshot.previews.reference = Some(candidate);
                        preview.snapshot.detection = DetectionState::Unknown;
                    }
                }
                Command::DiscardReferenceCandidate => {
                    preview.snapshot.previews.reference_candidate = None;
                }
                _ => {
                    debug_assert!(false, "unexpected setup reference preview command");
                }
            }
            return true;
        }

        self.send(command)
    }

    fn toggle_disco(&mut self) {
        #[cfg(not(windows))]
        if let Some(preview) = self.ui_preview.as_mut() {
            let enabled = !preview.snapshot.disco_enabled;
            preview.snapshot.disco_enabled = enabled;
            let now = Instant::now();
            self.disco_ui_activated_at = enabled.then_some(now);
            return;
        }

        let enabled = !self.snapshot().disco_enabled;
        if self.runtime.send(Command::ToggleDisco).is_accepted() {
            let now = Instant::now();
            self.disco_ui_activated_at = enabled.then_some(now);
        }
    }

    fn set_mode(&mut self, mode: OutputMode) {
        if self.config.output_mode == mode {
            return;
        }
        self.config.output_mode = mode;
        self.send(Command::SetMode(mode));
        self.queue_settings_save();
    }

    fn root_ui(&mut self, ui: &mut egui::Ui) -> Rect {
        let context = ui.ctx().clone();
        if self.view != AppView::Dashboard {
            self.previews.enlarged = None;
        }
        let render_snapshot = self.snapshot();
        let disco_enabled = render_snapshot.disco_enabled;
        self.render_snapshot = Some(render_snapshot);
        let content_rect = ui
            .scope(|ui| {
                if disco_enabled {
                    let accent = disco_ui_color(
                        Instant::now().saturating_duration_since(self.ui_animation_started_at),
                        0.0,
                    );
                    ui.visuals_mut().selection.bg_fill = accent;
                    ui.visuals_mut().widgets.hovered.bg_fill =
                        mix_color(Color32::from_rgb(48, 53, 63), accent, 0.22);
                    ui.visuals_mut().widgets.active.bg_fill =
                        mix_color(Color32::from_rgb(59, 67, 82), accent, 0.32);
                    ui.visuals_mut().panel_fill =
                        mix_color(Color32::from_rgb(18, 20, 24), accent, 0.08);
                }
                egui::CentralPanel::default()
                    .show(ui, |ui| {
                        let content_rect = ui.max_rect();
                        match self.view {
                            AppView::Dashboard => self.dashboard(ui),
                            AppView::Settings => self.settings_view(&context, ui),
                            AppView::SetupGuide => self.setup_guide_view(&context, ui),
                        }
                        content_rect
                    })
                    .inner
            })
            .inner;
        self.dialog(&context);
        self.update_version_picker(&context);
        self.notification_overlay(&context);
        self.paint_disco_interface(&context, content_rect, disco_enabled);
        self.render_snapshot = None;
        content_rect
    }
}

impl AppDialogKind {
    const fn preferred_width(self) -> f32 {
        match self {
            Self::ReferenceCapture => REFERENCE_DIALOG_WIDTH,
            Self::Admin => 500.0,
            Self::Exit | Self::ClearLogs => 440.0,
            Self::ReplaceAdminBaseline | Self::LoadAdminConfig | Self::RemoveAdminBaseline => 560.0,
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Exit => "Exit StageSwap?",
            Self::ClearLogs => "Clear diagnostic logs?",
            Self::ReferenceCapture => "Confirm reference image",
            Self::Admin => "Admin configuration",
            Self::ReplaceAdminBaseline => "Replace saved configuration?",
            Self::LoadAdminConfig => "Load saved configuration?",
            Self::RemoveAdminBaseline => "Delete saved configuration?",
        }
    }

    const fn icon(self) -> UiIcon {
        match self {
            Self::Exit => UiIcon::SignOut,
            Self::ClearLogs => UiIcon::Trash,
            Self::ReferenceCapture => UiIcon::Capture,
            Self::Admin => UiIcon::Wrench,
            Self::ReplaceAdminBaseline => UiIcon::Save,
            Self::LoadAdminConfig => UiIcon::Load,
            Self::RemoveAdminBaseline => UiIcon::Trash,
        }
    }

    const fn accent(self) -> Color32 {
        match self {
            Self::ReferenceCapture
            | Self::Admin
            | Self::ReplaceAdminBaseline
            | Self::LoadAdminConfig => SETTINGS_BLUE,
            Self::Exit | Self::ClearLogs | Self::RemoveAdminBaseline => {
                Color32::from_rgb(222, 90, 98)
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DialogButtonTone {
    Secondary,
    Primary,
    Danger,
    DangerOutline,
}

impl eframe::App for SwitcherApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(not(windows))]
        if let Some(image) = context.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        }) && let Some(capture) = self.ui_screenshot.take()
        {
            match save_ui_screenshot(&capture.path, &image) {
                Ok(()) => eprintln!(
                    "StageSwap UI screenshot saved to {}",
                    capture.path.display()
                ),
                Err(error) => eprintln!("StageSwap UI screenshot failed: {error}"),
            }
            self.exit_requested = true;
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        #[cfg(windows)]
        if let Some(readiness) = self.instance_readiness.take() {
            readiness.mark_ready();
        }
        if !self.updates.check_started {
            self.updates.check_started = true;
            self.request_update_check(false);
        }
        #[cfg(windows)]
        self.poll_update_worker();
        #[cfg(windows)]
        if let Some(commands) = self.instance_commands.as_ref() {
            let commands: Vec<_> = commands.try_iter().collect();
            for command in commands {
                match command {
                    stageswap_windows::InstanceCommand::Show => {
                        self.show_window(context);
                    }
                    stageswap_windows::InstanceCommand::ShutdownForReplacement => {
                        self.persist_settings();
                        self.exit_requested = true;
                        self.dismiss_dialog();
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        self.tick_ui_preview_notifications(context, Instant::now());
        let snapshot = self.snapshot();
        let now = Instant::now();
        self.notifications
            .ingest_runtime_alerts(&snapshot, self.config.show_notifications, now);
        self.notifications.prune(now);
        if self
            .awaiting_video_device_id
            .as_ref()
            .is_some_and(|expected| *expected == snapshot.selected_video_device_id)
        {
            self.awaiting_video_device_id = None;
        } else if self.awaiting_video_device_id.is_none()
            && !snapshot.selected_video_device_id.is_empty()
            && snapshot.selected_video_device_id != self.config.selected_video_device_id
        {
            self.config.selected_video_device_id = snapshot.selected_video_device_id.clone();
            self.persist_settings();
        }
        if self
            .awaiting_monitor_label
            .as_ref()
            .is_some_and(|expected| {
                snapshot
                    .selected_monitor
                    .as_ref()
                    .is_some_and(|monitor| monitor.label == *expected)
            })
        {
            self.awaiting_monitor_label = None;
        } else if self.awaiting_monitor_label.is_none() {
            self.sync_selected_monitor_preference(&snapshot);
        }
        if self.settings_save_due(Instant::now()) {
            self.flush_settings();
        }
        self.poll_settings_save();
        let first_activity_id = snapshot.recent_activity_first_id;
        if self.last_activity_id.saturating_add(1) < first_activity_id {
            self.log.write(
                "warning",
                "runtime",
                "ACTIVITY_GAP",
                "Runtime activity entries were overwritten before the UI observed them",
            );
            self.last_activity_id = first_activity_id.saturating_sub(1);
        }
        for (index, activity) in snapshot.recent_activity.iter().enumerate() {
            let activity_id = first_activity_id.saturating_add(index as u64);
            if activity_id > self.last_activity_id {
                self.log
                    .write("info", "runtime", "ACTIVITY", activity.as_str());
                self.last_activity_id = activity_id;
            }
        }
        #[cfg(windows)]
        if let Some(tray) = self.tray.as_ref() {
            tray.sync(&snapshot, self.locale());
        }
        #[cfg(windows)]
        if let Some(action) = self.tray.as_ref().and_then(tray::Tray::poll) {
            match action {
                tray::TrayAction::Show => {
                    self.show_window(context);
                }
                tray::TrayAction::OpenSettings => {
                    self.open_settings();
                    self.show_window(context);
                }
                tray::TrayAction::ToggleAutomation => {
                    if matches!(snapshot.run_state, RunState::Running | RunState::Starting) {
                        self.send(Command::Stop);
                    } else {
                        self.send(Command::Start);
                    }
                }
                tray::TrayAction::SetMode(mode) => self.set_mode(mode),
                tray::TrayAction::RescanDisplays => {
                    self.send(Command::Rescan);
                }
                tray::TrayAction::RestartScreenCapture => {
                    self.send(Command::Restart(RestartTarget::ScreenCapture));
                }
                tray::TrayAction::RestartVirtualCamera => {
                    self.send(Command::Restart(RestartTarget::VirtualCamera));
                }
                tray::TrayAction::RestartAll => {
                    self.send(Command::Restart(RestartTarget::All));
                }
                tray::TrayAction::OpenReferenceCapture => {
                    self.open_settings();
                    self.settings_tab = SettingsTab::Matching;
                    self.settings_section_changed_at = Some(Instant::now());
                    self.begin_reference_capture();
                    self.show_window(context);
                }
                tray::TrayAction::Exit => {
                    if self.config.confirm_exit {
                        self.open_dialog(AppDialogKind::Exit);
                        self.show_window(context);
                    } else {
                        self.exit_requested = true;
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        let (window_size, maximized, fullscreen) = context.input(|input| {
            let viewport = input.viewport();
            (
                viewport.inner_rect.map(|rect| rect.size()),
                viewport.maximized.unwrap_or(false),
                viewport.fullscreen.unwrap_or(false),
            )
        });
        if let Some(window_size) = window_size {
            if !maximized && !fullscreen {
                let desired = aspect_locked_window_size(window_size, self.last_window_size);
                if (desired.x - window_size.x).abs() > 1.0
                    || (desired.y - window_size.y).abs() > 1.0
                {
                    context.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired));
                    self.last_window_size = Some(desired);
                } else {
                    self.last_window_size = Some(window_size);
                }
            } else {
                self.last_window_size = Some(window_size);
            }
        }
        let close_requested = context.input(|input| input.viewport().close_requested());
        if close_requested && self.config.close_to_tray && !self.exit_requested {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(context);
        } else if close_requested && self.config.confirm_exit && !self.exit_requested {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.dialog_is(AppDialogKind::Exit) {
                self.open_dialog(AppDialogKind::Exit);
            }
        }
        context.request_repaint_after(repaint_interval(false));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        set_ui_locale(ui.ctx(), self.locale());
        if !self.window_visible {
            self.render_snapshot = None;
            ui.ctx().request_repaint_after(HIDDEN_REFRESH);
            return;
        }
        self.root_ui(ui);
        #[cfg(not(windows))]
        if let Some(capture) = self.ui_screenshot.as_mut()
            && !capture.requested
        {
            if capture.frames_until_request > 0 {
                capture.frames_until_request -= 1;
                ui.ctx().request_repaint();
            } else {
                eprintln!("StageSwap UI screenshot requested");
                ui.ctx()
                    .send_viewport_cmd(
                        egui::ViewportCommand::Screenshot(egui::UserData::default()),
                    );
                capture.requested = true;
                ui.ctx().request_repaint();
            }
        }
        ui.ctx().request_repaint_after(repaint_interval(true));
    }
}

const UI_LOCALE_ID: &str = "stageswap-ui-locale";

fn set_ui_locale(context: &egui::Context, locale: Locale) {
    context.data_mut(|data| data.insert_temp(egui::Id::new(UI_LOCALE_ID), locale));
}

fn ui_locale(ui: &egui::Ui) -> Locale {
    ui.data(|data| {
        data.get_temp::<Locale>(egui::Id::new(UI_LOCALE_ID))
            .unwrap_or_default()
    })
}

fn tr<'a>(ui: &egui::Ui, source: &'a str) -> std::borrow::Cow<'a, str> {
    localized_text(ui_locale(ui), source)
}

fn repaint_interval(ui_rendered: bool) -> Duration {
    if ui_rendered {
        VISIBLE_REFRESH
    } else {
        HIDDEN_REFRESH
    }
}

impl Drop for SwitcherApp {
    fn drop(&mut self) {
        #[cfg(not(windows))]
        if self.ui_preview.is_some() {
            return;
        }
        self.persist_settings();
        // Drop joins the single writer after its final pending value is saved.
        self.settings_writer.take();
    }
}

#[cfg(windows)]
fn save_config(store: &ConfigStore, config: &AppConfig) -> std::io::Result<()> {
    stageswap_windows::save_config_atomic(store, config)
}

#[cfg(not(windows))]
fn save_config(store: &ConfigStore, config: &AppConfig) -> std::io::Result<()> {
    store.save(config)
}

#[cfg(windows)]
fn save_admin_profile(
    store: &AdminProfileStore,
    config: &AppConfig,
) -> std::io::Result<AdminProfileStatus> {
    store.save_with_replace(config, stageswap_windows::replace_file_atomic)
}

#[cfg(not(windows))]
fn save_admin_profile(
    store: &AdminProfileStore,
    config: &AppConfig,
) -> std::io::Result<AdminProfileStatus> {
    store.save(config)
}

#[cfg(windows)]
fn set_admin_auto_restore(
    store: &AdminProfileStore,
    enabled: bool,
) -> std::io::Result<AdminProfileStatus> {
    store.set_auto_restore_with_replace(enabled, stageswap_windows::replace_file_atomic)
}

#[cfg(not(windows))]
fn set_admin_auto_restore(
    store: &AdminProfileStore,
    enabled: bool,
) -> std::io::Result<AdminProfileStatus> {
    store.set_auto_restore_on_launch(enabled)
}

fn admin_logo_activated(response: &egui::Response) -> bool {
    response.double_clicked_by(egui::PointerButton::Secondary)
}

fn animation_progress(started_at: Option<Instant>, duration: Duration) -> f32 {
    started_at.map_or(1.0, |started_at| {
        (Instant::now()
            .saturating_duration_since(started_at)
            .as_secs_f32()
            / duration.as_secs_f32())
        .clamp(0.0, 1.0)
    })
}

const fn setup_reference_requires_decision(state: SetupReferenceCaptureState) -> bool {
    matches!(
        state,
        SetupReferenceCaptureState::PreparingCandidate { .. }
            | SetupReferenceCaptureState::CapturingCandidate { .. }
            | SetupReferenceCaptureState::Review { .. }
            | SetupReferenceCaptureState::SavingCandidate { .. }
            | SetupReferenceCaptureState::SaveFailed { .. }
    )
}

const fn setup_reference_capture_label(reference_available: bool) -> &'static str {
    if reference_available {
        "Capture again"
    } else {
        "Capture reference image"
    }
}

const fn setup_step_icon(step: SetupStep) -> UiIcon {
    match step {
        SetupStep::HowItWorks => UiIcon::Route,
        SetupStep::Webcam => UiIcon::Camera,
        SetupStep::Screen => UiIcon::Monitor,
        SetupStep::Reference => UiIcon::Capture,
        SetupStep::Ready => UiIcon::CheckCircle,
    }
}

#[derive(Clone, Copy)]
enum SetupFooterButtonStyle {
    Secondary,
    IconOnly,
    Primary { icon_after: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MatchStrictnessExplanation {
    level: &'static str,
    effect: &'static str,
}

const fn component_health_guidance(
    webcam: DeviceState,
    screen: DeviceState,
    virtual_camera: DeviceState,
    detection: DetectionState,
) -> &'static str {
    if matches!(webcam, DeviceState::Failed | DeviceState::Unavailable) {
        "The webcam needs attention. Choose or refresh the camera in Webcam, then restart it here if needed."
    } else if matches!(screen, DeviceState::Failed | DeviceState::Unavailable) {
        "Screen capture needs attention. Choose a display in Screen, then restart capture here if needed."
    } else if matches!(
        virtual_camera,
        DeviceState::Failed | DeviceState::Unavailable
    ) {
        "The virtual camera needs attention. Restart it here, then reselect Stageswap Camera in Zoom if necessary."
    } else if matches!(webcam, DeviceState::Initializing)
        || matches!(screen, DeviceState::Initializing)
        || matches!(virtual_camera, DeviceState::Initializing)
    {
        "One or more components are still starting. Wait briefly before using a tool."
    } else {
        match detection {
            DetectionState::ReferenceMissing => {
                "The video components are ready, but Auto mode needs a captured or imported reference image."
            }
            DetectionState::Unknown => {
                "The components are ready; StageSwap is checking the reference image."
            }
            DetectionState::Matching => "Everything is ready.",
            DetectionState::NotMatching => "Everything is ready.",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SelectorUtilityGeometry {
    selector_width: f32,
    action_width: f32,
}

#[derive(Clone, Copy, Debug)]
struct ReferenceControlGeometry {
    action_width: f32,
    slider_width: f32,
}

const fn settings_switch_colors() -> (Color32, Color32) {
    (SETTINGS_SWITCH_OFF, SETTINGS_BLUE)
}

const HEALTH_STATES: [DeviceState; 4] = [
    DeviceState::Initializing,
    DeviceState::Ready,
    DeviceState::Unavailable,
    DeviceState::Failed,
];

#[derive(Clone, Copy)]
struct IndicatorChoice {
    icon: UiIcon,
    label: &'static str,
    current: bool,
    tone: IndicatorTone,
    span: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IndicatorTone {
    Green,
    Amber,
    Red,
}

#[cfg(test)]
mod tests;
