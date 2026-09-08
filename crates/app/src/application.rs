use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Vec2,
};
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

#[path = "app_icon.rs"]
mod app_icon;
#[path = "deployment_payload.rs"]
mod deployment_payload;
#[path = "local_log.rs"]
mod local_log;
#[path = "notifications.rs"]
mod notifications;
#[path = "preview_conversion.rs"]
mod preview_conversion;
#[path = "settings_persistence.rs"]
mod settings_persistence;
#[path = "setup_guide.rs"]
mod setup_guide;
#[cfg(windows)]
#[path = "tray.rs"]
mod tray;
#[path = "ui_icon.rs"]
mod ui_icon;
#[path = "ui_theme.rs"]
mod ui_theme;
#[path = "update.rs"]
mod update;
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

#[cfg(not(windows))]
fn parse_ui_preview_request(args: &[String]) -> Result<Option<UiPreviewRequest>, String> {
    let Some(preview_index) = args.iter().position(|argument| argument == "--ui-preview") else {
        return Ok(None);
    };
    ui_preview_locale(args)?;
    let target = match args.get(preview_index + 1).map(String::as_str) {
        Some("updates-versions") => UiPreviewTarget::UpdateVersions,
        Some("dialog-exit") => UiPreviewTarget::Dialog(AppDialogKind::Exit),
        Some("dialog-clear-logs") => UiPreviewTarget::Dialog(AppDialogKind::ClearLogs),
        Some("dialog-admin") => UiPreviewTarget::Dialog(AppDialogKind::Admin),
        Some("dialog-replace-baseline") => {
            UiPreviewTarget::Dialog(AppDialogKind::ReplaceAdminBaseline)
        }
        Some("dialog-load-admin-config") => UiPreviewTarget::Dialog(AppDialogKind::LoadAdminConfig),
        Some("dialog-remove-baseline") => {
            UiPreviewTarget::Dialog(AppDialogKind::RemoveAdminBaseline)
        }
        Some("dialog-reference-capture") => {
            UiPreviewTarget::Dialog(AppDialogKind::ReferenceCapture)
        }
        Some("notifications") => UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
        Some("notifications-empty") => {
            UiPreviewTarget::Notifications(NotificationPreviewState::Empty)
        }
        Some("notifications-critical") => {
            UiPreviewTarget::Notifications(NotificationPreviewState::Critical)
        }
        Some("notifications-updates") => {
            UiPreviewTarget::Notifications(NotificationPreviewState::Updates)
        }
        Some(value) if value.starts_with("setup-") => {
            let number = value
                .strip_prefix("setup-")
                .and_then(|value| value.parse::<usize>().ok());
            let step = number.and_then(SetupStep::from_number).ok_or_else(|| {
                format!("unknown setup preview '{value}'; expected setup-1 through setup-5")
            })?;
            UiPreviewTarget::Setup(step)
        }
        Some(value) if !value.starts_with("--") => SettingsTab::from_preview_name(value)
            .map(UiPreviewTarget::Settings)
            .ok_or_else(|| {
                format!(
                    "unknown UI preview '{value}'; expected a Settings page, notifications preview, or dialog-* preview"
                )
            })?,
        _ => UiPreviewTarget::Settings(SettingsTab::General),
    };
    Ok(Some(UiPreviewRequest { target }))
}

#[cfg(not(windows))]
fn parse_ui_screenshot_path(args: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(index) = args
        .iter()
        .position(|argument| argument == "--ui-screenshot")
    else {
        return Ok(None);
    };
    let value = args
        .get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| "--ui-screenshot requires an absolute PNG path".to_string())?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("--ui-screenshot requires an absolute PNG path".to_string());
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("png") {
        return Err("--ui-screenshot path must end in .png".to_string());
    }
    Ok(Some(path))
}

#[cfg(not(windows))]
fn parse_setup_demo_preview_state(
    args: &[String],
) -> Result<Option<SetupDemoPreviewState>, String> {
    let Some(index) = args
        .iter()
        .position(|argument| argument == "--ui-setup-demo-state")
    else {
        return Ok(None);
    };
    match args.get(index + 1).map(String::as_str) {
        Some("matching") => Ok(Some(SetupDemoPreviewState::Matching)),
        Some("changed" | "non-matching") => Ok(Some(SetupDemoPreviewState::Changed)),
        _ => Err("--ui-setup-demo-state requires either matching or non-matching".to_string()),
    }
}

#[cfg(not(windows))]
fn parse_setup_reference_preview_state(
    args: &[String],
) -> Result<Option<SetupReferencePreviewState>, String> {
    let Some(index) = args
        .iter()
        .position(|argument| argument == "--ui-setup-reference-state")
    else {
        return Ok(None);
    };
    match args.get(index + 1).map(String::as_str) {
        Some("captured") => Ok(Some(SetupReferencePreviewState::Captured)),
        Some("empty") => Ok(Some(SetupReferencePreviewState::Empty)),
        Some("review") => Ok(Some(SetupReferencePreviewState::Review)),
        Some("missing-screen") => Ok(Some(SetupReferencePreviewState::MissingScreen)),
        _ => Err(
            "--ui-setup-reference-state requires captured, empty, review, or missing-screen"
                .to_string(),
        ),
    }
}

#[cfg(not(windows))]
fn ui_preview_locale(args: &[String]) -> Result<Locale, String> {
    let Some(index) = args.iter().position(|argument| argument == "--ui-language") else {
        return Ok(Locale::English);
    };
    let value = args
        .get(index + 1)
        .ok_or_else(|| "--ui-language requires en-US, fr-FR, or es".to_string())?;
    Locale::from_tag(value)
        .ok_or_else(|| format!("unknown UI language '{value}'; expected en-US, fr-FR, or es"))
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

#[cfg(not(windows))]
fn ui_preview_config() -> AppConfig {
    let locale = ui_preview_locale(&std::env::args().collect::<Vec<_>>()).unwrap_or_default();
    AppConfig {
        selected_video_device_id: "preview-camera".into(),
        selected_monitor_label: "Stage display".into(),
        start_with_windows: true,
        start_minimized: true,
        interface_language: locale.tag().into(),
        start_automatically: true,
        output_mode: OutputMode::Automatic,
        still_image_pip_enabled: true,
        ..AppConfig::default()
    }
}

#[cfg(not(windows))]
fn ui_preview_snapshot() -> AppSnapshot {
    let webcam = ui_preview_frame(
        1,
        "setup-webcam-example",
        include_bytes!("../assets/setup-webcam-example.png"),
    );
    let screen = ui_preview_frame(
        2,
        "setup-reference-example",
        include_bytes!("../assets/setup-reference-example.png"),
    );
    AppSnapshot {
        run_state: RunState::Running,
        mode: OutputMode::Automatic,
        detection: DetectionState::Matching,
        automatic_target: Source::Camera,
        actual_output: Source::Camera,
        availability: stageswap_core::SourceAvailability {
            camera_ready: true,
            screen_ready: true,
        },
        webcam_state: DeviceState::Ready,
        screen_state: DeviceState::Ready,
        virtual_camera_state: DeviceState::Ready,
        webcam_fps: Some(30),
        screen_fps: Some(30),
        output_fps: Some(30),
        recent_activity_first_id: 1,
        recent_activity: vec![
            "Reference display confirmed".into(),
            "Automatic output selected Camera".into(),
        ]
        .into(),
        previews: stageswap_core::PreviewFrames {
            final_output: Some(Arc::clone(&webcam)),
            webcam: Some(webcam),
            screen: Some(Arc::clone(&screen)),
            reference: Some(screen),
            reference_candidate: None,
        },
        video_devices: vec![
            stageswap_core::VideoDeviceChoice {
                id: "preview-camera".into(),
                name: "Studio Camera".into(),
            },
            stageswap_core::VideoDeviceChoice {
                id: "preview-camera-secondary".into(),
                name: "Wide Camera".into(),
            },
        ]
        .into(),
        selected_video_device_id: "preview-camera".into(),
        monitors: vec![
            stageswap_core::MonitorDescriptor {
                display_name: "preview-display-1".into(),
                label: "Control display".into(),
                width: 1920,
                height: 1080,
                ..stageswap_core::MonitorDescriptor::default()
            },
            stageswap_core::MonitorDescriptor {
                display_name: "preview-display-2".into(),
                label: "Stage display".into(),
                x: 1920,
                width: 1920,
                height: 1080,
                ..stageswap_core::MonitorDescriptor::default()
            },
        ]
        .into(),
        selected_monitor: Some(stageswap_core::MonitorDescriptor {
            display_name: "preview-display-2".into(),
            label: "Stage display".into(),
            x: 1920,
            width: 1920,
            height: 1080,
            ..stageswap_core::MonitorDescriptor::default()
        }),
        ..AppSnapshot::default()
    }
}

#[cfg(not(windows))]
fn ui_preview_frame(sequence: u64, name: &'static str, bytes: &[u8]) -> Arc<Frame> {
    let image = image::load_from_memory(bytes)
        .unwrap_or_else(|error| panic!("{name} should be a valid embedded image: {error}"))
        .to_rgba8();
    let size = stageswap_core::Size::new(image.width(), image.height());
    let mut pixels = image.into_raw();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(
        Frame::new(
            pixels.into(),
            size,
            size.width * 4,
            sequence,
            0,
            Instant::now(),
        )
        .expect("UI preview frame is valid"),
    )
}

#[cfg(not(windows))]
fn clone_ui_preview_frame(frame: &Frame, sequence: u64) -> Option<Arc<Frame>> {
    Frame::new(
        frame.pixels_arc(),
        frame.size,
        frame.stride,
        sequence,
        frame.timestamp_100ns,
        Instant::now(),
    )
    .ok()
    .map(Arc::new)
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
    textures: HashMap<&'static str, PreviewTexture>,
    preview_converters: HashMap<PreviewKind, PreviewConverter>,
    render_snapshot: Option<Arc<AppSnapshot>>,
    log: LocalLog,
    last_activity_id: u64,
    notifications: NotificationCenter,
    notification_center_open: bool,
    update_status: UpdateStatus,
    update_versions: update::VersionCatalog,
    selected_update_version: Option<update::ReleaseVersion>,
    update_version_picker_open: bool,
    #[cfg(windows)]
    update_notifications: UpdateNotificationState,
    update_check_started: bool,
    #[cfg(windows)]
    update_worker: Option<UpdateWorker>,
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
    enlarged_dashboard_preview: Option<PreviewKind>,
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
            textures: HashMap::new(),
            preview_converters: HashMap::new(),
            render_snapshot: None,
            log,
            last_activity_id: 0,
            notifications,
            notification_center_open: false,
            update_status: UpdateStatus::Idle,
            update_versions: update::VersionCatalog::Idle,
            selected_update_version: None,
            update_version_picker_open: false,
            #[cfg(windows)]
            update_notifications,
            update_check_started: false,
            #[cfg(windows)]
            update_worker,
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
            enlarged_dashboard_preview: None,
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
        self.enlarged_dashboard_preview = None;
        self.render_snapshot = None;
        for converter in self.preview_converters.values() {
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

    #[cfg(not(windows))]
    fn with_ui_preview(mut self, request: UiPreviewRequest) -> Self {
        let preview_now = Instant::now();
        let periodic_notifications = !matches!(
            request.target,
            UiPreviewTarget::UpdateVersions
                | UiPreviewTarget::Notifications(
                    NotificationPreviewState::Empty
                        | NotificationPreviewState::Critical
                        | NotificationPreviewState::Updates
                )
        );
        if periodic_notifications {
            self.seed_ui_preview_notifications(preview_now);
        }
        match request.target {
            UiPreviewTarget::UpdateVersions => {
                self.view = AppView::Settings;
                self.settings_tab = SettingsTab::Updates;
                self.config.update_channel = UpdateChannel::Beta;
                self.update_version_picker_open = true;
                self.update_versions = update::VersionCatalog::Ready(update::preview_versions());
            }
            UiPreviewTarget::Settings(tab) => {
                self.view = AppView::Settings;
                self.settings_tab = tab;
            }
            UiPreviewTarget::Setup(step) => {
                self.view = AppView::SetupGuide;
                self.setup_session = Some(SetupSession::preview(step));
            }
            UiPreviewTarget::Dialog(kind) => {
                self.view = AppView::Settings;
                self.settings_tab = match kind {
                    AppDialogKind::ClearLogs => SettingsTab::Diagnostics,
                    AppDialogKind::ReferenceCapture => SettingsTab::Matching,
                    _ => SettingsTab::General,
                };
                if matches!(
                    kind,
                    AppDialogKind::Admin
                        | AppDialogKind::ReplaceAdminBaseline
                        | AppDialogKind::LoadAdminConfig
                        | AppDialogKind::RemoveAdminBaseline
                ) {
                    self.admin_profile_status = Some(AdminProfileStatus {
                        auto_restore_on_launch: true,
                        reference_included: true,
                    });
                }
                self.open_dialog(kind);
            }
            UiPreviewTarget::Notifications(state) => {
                self.view = AppView::Dashboard;
                let now = Instant::now();
                match state {
                    NotificationPreviewState::Empty => {
                        self.notification_center_open = true;
                    }
                    NotificationPreviewState::Critical => {
                        self.notifications.push_critical(
                            NotificationSource::Webcam,
                            "The selected webcam stopped delivering frames.",
                            now,
                            true,
                        );
                        self.notification_center_open = true;
                    }
                    NotificationPreviewState::Updates => {
                        self.notifications.push_update(
                            "2.4.0",
                            "StageSwap 2.4.0 is ready. Open Settings → Updates to install it.",
                            now,
                            true,
                        );
                        self.notification_center_open = true;
                    }
                    NotificationPreviewState::Stacked => {
                        self.notification_center_open = false;
                    }
                }
            }
        }
        self.settings_opened_at = None;
        self.settings_section_changed_at = None;
        self.ui_preview = Some(UiPreviewSession {
            snapshot: ui_preview_snapshot(),
            next_notification_at: periodic_notifications
                .then_some(preview_now + UI_PREVIEW_NOTIFICATION_INTERVAL),
            notification_count: 0,
        });
        self
    }

    #[cfg(not(windows))]
    fn seed_ui_preview_notifications(&mut self, now: Instant) {
        self.notifications.push_critical(
            NotificationSource::Webcam,
            "The selected webcam stopped delivering frames.",
            now,
            true,
        );
        self.notifications.push_critical(
            NotificationSource::Screen,
            "The selected secondary screen is not providing frames.",
            now,
            true,
        );
        self.notifications.push_update(
            "2.4.0",
            "StageSwap 2.4.0 is ready. Open Settings → Updates to install it.",
            now,
            true,
        );
    }

    #[cfg(not(windows))]
    fn tick_ui_preview_notifications(&mut self, context: &egui::Context, now: Instant) {
        let count = {
            let Some(preview) = self.ui_preview.as_mut() else {
                return;
            };
            let Some(next_notification_at) = preview.next_notification_at else {
                return;
            };
            if now < next_notification_at {
                context.request_repaint_after(next_notification_at - now);
                return;
            }

            preview.notification_count = preview.notification_count.saturating_add(1);
            preview.next_notification_at = Some(now + UI_PREVIEW_NOTIFICATION_INTERVAL);
            preview.notification_count.to_string()
        };
        let message = format_text(
            self.locale(),
            "Preview activity notification {0}: 30 seconds have passed.",
            &[&count],
        );
        self.notifications
            .push_information(NotificationSource::Preview, message, now);
        context.request_repaint_after(UI_PREVIEW_NOTIFICATION_INTERVAL);
    }

    #[cfg(not(windows))]
    fn with_ui_screenshot(mut self, path: PathBuf) -> Self {
        self.ui_screenshot = Some(UiScreenshotCapture {
            path,
            frames_until_request: 2,
            requested: false,
        });
        self
    }

    #[cfg(not(windows))]
    fn with_setup_demo_preview_state(mut self, state: Option<SetupDemoPreviewState>) -> Self {
        self.setup_demo_preview_state = state;
        self
    }

    #[cfg(not(windows))]
    fn with_setup_reference_preview_state(
        mut self,
        state: Option<SetupReferencePreviewState>,
    ) -> Self {
        let Some(state) = state else {
            return self;
        };
        let Some(preview) = self.ui_preview.as_mut() else {
            return self;
        };
        match state {
            SetupReferencePreviewState::Captured => {
                preview.snapshot.previews.reference_candidate = None;
                self.setup_reference_capture = SetupReferenceCaptureState::Idle;
            }
            SetupReferencePreviewState::Empty => {
                preview.snapshot.previews.reference = None;
                preview.snapshot.previews.reference_candidate = None;
                self.setup_reference_capture = SetupReferenceCaptureState::Idle;
            }
            SetupReferencePreviewState::Review => {
                preview.snapshot.previews.reference = None;
                preview.snapshot.previews.reference_candidate =
                    preview.snapshot.previews.screen.clone();
                self.setup_reference_capture = SetupReferenceCaptureState::Review {
                    captured_at: Instant::now() - SETUP_REFERENCE_FLASH_DURATION,
                };
            }
            SetupReferencePreviewState::MissingScreen => {
                preview.snapshot.previews.reference = None;
                preview.snapshot.previews.reference_candidate = None;
                preview.snapshot.previews.screen = None;
                preview.snapshot.selected_monitor = None;
                preview.snapshot.monitors = Vec::new().into();
                preview.snapshot.screen_state = DeviceState::Unavailable;
                preview.snapshot.availability.screen_ready = false;
                self.config.selected_monitor_label.clear();
                self.setup_reference_capture = SetupReferenceCaptureState::Idle;
            }
        }
        self
    }

    fn setup_demo_preview_alpha(&self) -> Option<f32> {
        #[cfg(not(windows))]
        {
            self.setup_demo_preview_state.map(|state| match state {
                SetupDemoPreviewState::Matching => 0.0,
                SetupDemoPreviewState::Changed => 1.0,
            })
        }
        #[cfg(windows)]
        {
            None
        }
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

    fn queue_settings_save(&mut self) {
        self.pending_settings_save = Some(Instant::now());
    }

    fn settings_save_due(&self, now: Instant) -> bool {
        self.pending_settings_save.is_some_and(|started_at| {
            now.saturating_duration_since(started_at) >= SETTINGS_SAVE_DEBOUNCE
        })
    }

    fn flush_settings(&mut self) {
        if self.pending_settings_save.take().is_none() {
            return;
        }
        self.send(Command::UpdateSettings(Box::new(self.config.clone())));
        self.persist_settings();
    }

    fn persist_settings(&mut self) {
        if let Some(writer) = &self.settings_writer {
            writer.submit(self.config.clone());
        } else {
            self.record_settings_save_error(
                "Could not save settings: settings writer is unavailable".into(),
            );
        }
    }

    fn poll_settings_save(&mut self) {
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
    fn wait_for_settings_save(&mut self) {
        assert!(
            self.settings_writer
                .as_ref()
                .unwrap()
                .drain(Duration::from_secs(2))
        );
        self.poll_settings_save();
    }

    fn record_settings_save_error(&mut self, message: String) {
        self.log
            .write("error", "configuration", "SETTINGS_SAVE_FAILED", &message);
        self.settings_save_error = Some(message.clone());
        self.push_notification_warning(NotificationSource::Configuration, message);
    }

    fn sync_selected_monitor_preference(&mut self, snapshot: &AppSnapshot) {
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

    fn open_settings(&mut self) {
        self.enlarged_dashboard_preview = None;
        self.view = AppView::Settings;
        self.disco_diagnostics_gesture.reset();
        self.settings_opened_at = Some(Instant::now());
        self.settings_section_changed_at = Some(Instant::now());
        self.send(Command::RefreshVideoDevices);
        if self.config.automatic_monitor_rescans {
            self.send(Command::Rescan);
        }
    }

    fn request_update_check(&mut self, manual: bool) {
        if self.update_busy() {
            return;
        }
        #[cfg(windows)]
        {
            let Some(worker) = self.update_worker.as_ref() else {
                if manual {
                    self.update_status =
                        UpdateStatus::Failed("The update service is unavailable.".into());
                }
                return;
            };
            match worker.request(UpdateRequest::Check {
                channel: self.config.update_channel,
                manual,
            }) {
                Ok(()) => self.update_status = UpdateStatus::Checking,
                Err(error) if manual => self.update_status = UpdateStatus::Failed(error),
                Err(error) => {
                    self.log
                        .write("warning", "update", "UPDATE_CHECK_NOT_STARTED", &error)
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = manual;
            self.update_status = UpdateStatus::UpToDate;
        }
    }

    fn update_busy(&self) -> bool {
        matches!(
            self.update_status,
            UpdateStatus::Checking | UpdateStatus::Downloading(_) | UpdateStatus::Installing
        ) || matches!(self.update_versions, update::VersionCatalog::Loading)
    }

    /// Fetching or cancelling the picker never changes the latest-update offer.
    fn request_update_versions(&mut self) {
        if self.config.update_channel != UpdateChannel::Beta || self.update_busy() {
            return;
        }
        self.selected_update_version = None;
        self.update_version_picker_open = true;
        self.update_versions = update::VersionCatalog::Loading;
        #[cfg(not(windows))]
        if self.ui_preview.is_some() {
            self.update_versions = update::VersionCatalog::Ready(update::preview_versions());
            return;
        }
        #[cfg(windows)]
        {
            let result = self
                .update_worker
                .as_ref()
                .ok_or_else(|| "The update service is unavailable.".to_owned())
                .and_then(|worker| worker.request(UpdateRequest::ListVersions));
            if let Err(error) = result {
                self.update_versions = update::VersionCatalog::Failed(error);
            }
        }
        #[cfg(not(windows))]
        {
            self.update_versions = update::VersionCatalog::Failed(
                "Version downloads are available on Windows only.".into(),
            );
        }
    }

    /// Only an explicit catalog selection can be installed; the running version is excluded.
    fn selected_version_release(&self) -> Option<update::AvailableUpdate> {
        if self.config.update_channel != UpdateChannel::Beta || self.update_busy() {
            return None;
        }
        let update::VersionCatalog::Ready(versions) = &self.update_versions else {
            return None;
        };
        versions
            .iter()
            .find(|release| {
                Some(release.version) == self.selected_update_version
                    && release.version.to_string() != env!("CARGO_PKG_VERSION")
            })
            .cloned()
    }

    #[cfg(windows)]
    fn poll_update_worker(&mut self) {
        let Some(result) = self.update_worker.as_ref().and_then(UpdateWorker::poll) else {
            return;
        };
        match result {
            UpdateResult::Checked { manual, result } => match result {
                Ok(Some(release)) => {
                    let should_notify = self.config.notify_updates
                        && self
                            .update_notifications
                            .should_notify(self.config.update_channel, release.version);
                    if should_notify {
                        if let Err(error) = self.update_notifications.save(self.store.directory()) {
                            let message =
                                format!("Could not save update notification state: {error}");
                            self.log.write(
                                "warning",
                                "update",
                                "UPDATE_NOTIFICATION_STATE_FAILED",
                                &message,
                            );
                            self.notifications.push_critical_with_detail(
                                NotificationSource::Configuration,
                                format_text(
                                    self.locale(),
                                    "Could not save update notification state.",
                                    &[],
                                ),
                                message.clone(),
                                Instant::now(),
                                self.config.show_notifications,
                            );
                            self.load_warnings.push(message);
                        }
                        let version = release.version.to_string();
                        let message = format_text(
                            self.locale(),
                            "StageSwap {0} is ready. Open Settings → Updates to install it.",
                            &[&version],
                        );
                        self.notifications.push_update(
                            &version,
                            message,
                            Instant::now(),
                            self.config.notify_updates,
                        );
                    }
                    self.update_status = UpdateStatus::Available(release);
                }
                Ok(None) => self.update_status = UpdateStatus::UpToDate,
                Err(error) if manual => self.update_status = UpdateStatus::Failed(error),
                Err(error) => {
                    self.log
                        .write("warning", "update", "UPDATE_CHECK_FAILED", &error);
                    self.update_status = UpdateStatus::Idle;
                }
            },
            UpdateResult::Versions(result) => {
                self.update_versions = match result {
                    Ok(releases) => update::VersionCatalog::Ready(releases),
                    Err(error) => update::VersionCatalog::Failed(error),
                };
            }
            UpdateResult::InstallFailed(error) => {
                self.log
                    .write("error", "update", "UPDATE_INSTALL_FAILED", &error);
                self.update_status = UpdateStatus::Failed(error);
            }
            UpdateResult::InstallStarted => self.update_status = UpdateStatus::Installing,
        }
    }

    fn install_available_update(&mut self) {
        let UpdateStatus::Available(release) = &self.update_status else {
            return;
        };
        self.install_release(release.clone());
    }

    /// Both entry points share verification and native replacement.
    fn install_release(&mut self, release: update::AvailableUpdate) {
        if self.update_busy() {
            return;
        }
        #[cfg(not(windows))]
        if self.ui_preview.is_some() {
            self.update_status = UpdateStatus::Downloading(release);
            return;
        }
        #[cfg(windows)]
        {
            let Some(worker) = self.update_worker.as_ref() else {
                self.update_status =
                    UpdateStatus::Failed("The update service is unavailable.".into());
                return;
            };
            match worker.request(UpdateRequest::Install(release.clone())) {
                Ok(()) => self.update_status = UpdateStatus::Downloading(release),
                Err(error) => self.update_status = UpdateStatus::Failed(error),
            }
        }
        #[cfg(not(windows))]
        {
            self.update_status = UpdateStatus::Downloading(release);
        }
    }

    fn start_setup_guide(&mut self, return_view: SetupReturnView) {
        self.flush_settings();
        self.dismiss_dialog();
        self.view = AppView::SetupGuide;
        self.setup_session = Some(SetupSession::live(return_view));
        self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        self.send_setup_reference_command(Command::DiscardReferenceCandidate);
        self.send(Command::RefreshVideoDevices);
        if self.config.automatic_monitor_rescans {
            self.send(Command::Rescan);
        }
    }

    fn apply_setup_action(&mut self, action: SetupAction) {
        let Some(mut session) = self.setup_session else {
            return;
        };
        let reference_requires_decision = session.step == SetupStep::Reference
            && setup_reference_requires_decision(self.setup_reference_capture);
        let reference_available = self.snapshot().previews.reference.is_some()
            || matches!(
                self.setup_reference_capture,
                SetupReferenceCaptureState::Confirmed
            );
        let reference_capture_required =
            session.step == SetupStep::Reference && !reference_available;
        let moves_forward = matches!(action, SetupAction::Next)
            || matches!(
                action,
                SetupAction::GoTo(destination)
                    if destination.number() > SetupStep::Reference.number()
            );
        if (reference_requires_decision || reference_capture_required) && moves_forward {
            return;
        }
        let leaves_reference = session.step == SetupStep::Reference
            && match action {
                SetupAction::Previous | SetupAction::Next | SetupAction::Close => true,
                SetupAction::GoTo(destination) => destination != SetupStep::Reference,
            };
        if leaves_reference && reference_requires_decision {
            if !self.send_setup_reference_command(Command::DiscardReferenceCandidate) {
                return;
            }
            self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        }
        match action {
            SetupAction::Previous => {
                if let Some(step) = session.step.previous() {
                    session.transition_to(step, self.setup_animations_enabled);
                    self.setup_session = Some(session);
                }
            }
            SetupAction::Next => {
                if let Some(step) = session.step.next() {
                    session.transition_to(step, self.setup_animations_enabled);
                    self.setup_session = Some(session);
                } else {
                    self.finish_setup_guide();
                }
            }
            SetupAction::GoTo(step) => {
                session.transition_to(step, self.setup_animations_enabled);
                self.setup_session = Some(session);
            }
            SetupAction::Close => self.dismiss_setup_guide(session.return_view),
        }
    }

    fn mark_setup_guide_completed(&mut self) {
        if let Err(error) = self.setup_store.mark_completed() {
            let warning = format!(
                "Could not remember setup guide completion; you can still reopen it in Settings: {error}"
            );
            self.log
                .write("warning", "setup", "SETUP_COMPLETION_SAVE_FAILED", &warning);
            self.push_notification_warning(NotificationSource::Startup, warning);
        }
    }

    fn finish_setup_guide(&mut self) {
        self.mark_setup_guide_completed();
        self.set_mode(OutputMode::Automatic);
        self.flush_settings();
        self.send(Command::Start);
        self.send_setup_reference_command(Command::DiscardReferenceCandidate);
        self.setup_session = None;
        self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        self.view = AppView::Dashboard;
    }

    fn dismiss_setup_guide(&mut self, return_view: SetupReturnView) {
        self.mark_setup_guide_completed();
        self.send_setup_reference_command(Command::DiscardReferenceCandidate);
        self.setup_session = None;
        self.setup_reference_capture = SetupReferenceCaptureState::Idle;
        match return_view {
            SetupReturnView::Dashboard => {
                self.view = AppView::Dashboard;
            }
            SetupReturnView::Settings => {
                self.view = AppView::Settings;
                self.settings_tab = SettingsTab::General;
                self.settings_opened_at = Some(Instant::now());
                self.settings_section_changed_at = Some(Instant::now());
            }
        }
    }

    fn close_settings(&mut self) {
        self.flush_settings();
        self.view = AppView::Dashboard;
        self.disco_diagnostics_gesture.reset();
        self.active_dialog = None;
        self.settings_opened_at = None;
        self.settings_section_changed_at = None;
    }

    fn open_dialog(&mut self, kind: AppDialogKind) {
        self.active_dialog = Some(ActiveDialog {
            kind,
            opened_at: Instant::now(),
            focus_safe_action: true,
        });
    }

    fn dismiss_dialog(&mut self) {
        self.active_dialog = None;
    }

    fn dialog_is(&self, kind: AppDialogKind) -> bool {
        self.active_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.kind == kind)
    }

    fn begin_reference_capture(&mut self) {
        self.prepare_reference_capture();
        self.open_dialog(AppDialogKind::ReferenceCapture);
    }

    fn prepare_reference_capture(&mut self) {
        if self.send_setup_reference_command(Command::DiscardReferenceCandidate) {
            self.setup_reference_capture = SetupReferenceCaptureState::PreparingCandidate {
                started_at: Instant::now(),
            };
        }
    }

    fn capture_reference_candidate(&mut self) {
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

    fn cancel_reference_capture(&mut self) {
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

    fn confirm_reference_candidate(&mut self) {
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

    fn open_admin_configuration(&mut self) {
        match AdminProfileStore::new(self.store.directory()).status() {
            Ok(status) => self.admin_profile_status = status,
            Err(error) => {
                self.admin_profile_status = None;
                self.record_admin_failure(format!(
                    "Could not read the existing admin config: {error}"
                ));
            }
        }
        self.open_dialog(AppDialogKind::Admin);
    }

    fn save_admin_baseline(&mut self) {
        let admin_store = AdminProfileStore::new(self.store.directory());
        match save_admin_profile(&admin_store, &self.config) {
            Ok(status) => {
                self.admin_profile_status = Some(status);
                self.open_dialog(AppDialogKind::Admin);
                self.log.write(
                    "info",
                    "configuration",
                    "ADMIN_BASELINE_SAVED",
                    "Admin config saved",
                );
            }
            Err(error) => {
                self.record_admin_failure(format!("Could not save the admin config: {error}"));
            }
        }
    }

    fn load_admin_config(&mut self) {
        // A pending older write must finish before the restore replaces config.json.
        if self
            .settings_writer
            .as_ref()
            .is_some_and(|writer| !writer.drain(Duration::from_secs(2)))
        {
            self.record_admin_failure(
                "Could not load the admin config: settings are still being saved".into(),
            );
            return;
        }
        self.poll_settings_save();
        let admin_store = AdminProfileStore::new(self.store.directory());
        match restore_admin_profile_now(&admin_store) {
            Ok(AdminRestoreOutcome::Restored) => {
                let mut loaded = self.store.load();
                for warning in loaded.warnings.drain(..) {
                    self.log
                        .write("warning", "configuration", "LOAD_WARNING", &warning);
                    self.push_notification_warning(NotificationSource::Configuration, warning);
                }

                let selected_monitor = {
                    let snapshot = self.snapshot();
                    snapshot
                        .monitors
                        .iter()
                        .find(|monitor| monitor.label == loaded.config.selected_monitor_label)
                        .or_else(|| snapshot.monitors.get(1))
                        .or_else(|| snapshot.monitors.first())
                        .cloned()
                };
                self.awaiting_video_device_id =
                    Some(loaded.config.selected_video_device_id.clone());
                self.awaiting_monitor_label = selected_monitor
                    .as_ref()
                    .map(|monitor| monitor.label.clone());
                self.config = loaded.config;
                self.pending_settings_save = None;
                self.settings_save_error = None;
                self.send(Command::ReloadSettings(Box::new(self.config.clone())));
                if let Some(monitor) = selected_monitor {
                    self.send(Command::SelectMonitor(monitor));
                }
                if self.config.automatic_monitor_rescans {
                    self.send(Command::Rescan);
                }
                self.open_dialog(AppDialogKind::Admin);
                self.log.write(
                    "info",
                    "configuration",
                    "ADMIN_CONFIG_LOADED",
                    "Admin config loaded",
                );
            }
            Ok(AdminRestoreOutcome::Missing) => {
                self.admin_profile_status = None;
                self.record_admin_failure("No admin config is saved.".into());
            }
            Ok(AdminRestoreOutcome::Disabled) => {
                self.record_admin_failure("Could not load the admin config.".into());
            }
            Err(error) => {
                self.record_admin_failure(format!("Could not load the admin config: {error}"));
            }
        }
    }

    fn set_admin_auto_restore(&mut self, enabled: bool) {
        let admin_store = AdminProfileStore::new(self.store.directory());
        match set_admin_auto_restore(&admin_store, enabled) {
            Ok(status) => {
                self.admin_profile_status = Some(status);
                self.log.write(
                    "info",
                    "configuration",
                    "ADMIN_AUTO_RESTORE_CHANGED",
                    if enabled {
                        "Admin auto-restore enabled"
                    } else {
                        "Admin auto-restore disabled"
                    },
                );
            }
            Err(error) => {
                self.record_admin_failure(format!("Could not update auto-restore: {error}"));
            }
        }
    }

    fn remove_admin_baseline(&mut self) {
        let admin_store = AdminProfileStore::new(self.store.directory());
        match admin_store.remove() {
            Ok(_) => {
                self.admin_profile_status = None;
                self.open_dialog(AppDialogKind::Admin);
                self.log.write(
                    "info",
                    "configuration",
                    "ADMIN_BASELINE_REMOVED",
                    "Admin config deleted",
                );
            }
            Err(error) => {
                self.record_admin_failure(format!("Could not delete the admin config: {error}"));
            }
        }
    }

    fn record_admin_failure(&mut self, message: String) {
        self.log.write(
            "warning",
            "configuration",
            "ADMIN_CONFIGURATION_FAILED",
            &message,
        );
        self.push_notification_warning(NotificationSource::Configuration, message);
    }

    fn import_reference_dialog(&mut self) {
        #[cfg(windows)]
        if let Some(path) = stageswap_windows::pick_reference_image(self.locale()) {
            self.send(Command::ImportReference(path));
        }
        #[cfg(not(windows))]
        self.log.write(
            "info",
            "ui",
            "REFERENCE_FILE_DIALOG_UNAVAILABLE",
            "Reference file dialogs are available in the Windows application",
        );
    }

    fn open_log_directory(&mut self) {
        #[cfg(windows)]
        if let Err(error) = stageswap_windows::open_directory(self.log.directory()) {
            self.push_notification_warning(NotificationSource::Configuration, error);
        }
        #[cfg(not(windows))]
        self.log.write(
            "info",
            "ui",
            "LOG_DIRECTORY_DISPLAY_UNAVAILABLE",
            &format!("Log directory: {}", self.log.directory().display()),
        );
    }

    fn export_logs(&mut self) {
        #[cfg(windows)]
        if let Some(path) = stageswap_windows::pick_log_export_path(self.locale())
            && let Err(error) = self.log.export_to(&path)
        {
            self.push_notification_warning(
                NotificationSource::Configuration,
                format!("Could not export logs: {error}"),
            );
        }
        #[cfg(not(windows))]
        self.log.write(
            "info",
            "ui",
            "LOG_EXPORT_DIALOG_UNAVAILABLE",
            "Log export dialog is available in the Windows application",
        );
    }

    fn clear_logs(&mut self) {
        match self.log.clear() {
            Ok(()) => self
                .log
                .write("info", "logging", "LOGS_CLEARED", "Diagnostic logs cleared"),
            Err(error) => self.push_notification_warning(
                NotificationSource::Configuration,
                format!("Could not clear logs: {error}"),
            ),
        }
    }

    fn request_log_clear(&mut self) {
        self.open_dialog(AppDialogKind::ClearLogs);
    }

    fn confirm_log_clear(&mut self) {
        if self.dialog_is(AppDialogKind::ClearLogs) {
            self.clear_logs();
            self.dismiss_dialog();
        }
    }

    fn root_ui(&mut self, ui: &mut egui::Ui) -> Rect {
        let context = ui.ctx().clone();
        if self.view != AppView::Dashboard {
            self.enlarged_dashboard_preview = None;
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

    fn notification_overlay(&mut self, context: &egui::Context) {
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

    fn notification_popover(&mut self, context: &egui::Context, now: Instant) -> bool {
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

    fn notification_toasts(&self, context: &egui::Context, now: Instant) {
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

    fn paint_disco_interface(
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

    fn setup_guide_view(&mut self, context: &egui::Context, ui: &mut egui::Ui) {
        if self.setup_session.is_none() {
            self.view = AppView::Dashboard;
            return;
        }
        let now = Instant::now();
        let (step, entrance, page_opacity, transition_active) = {
            let session = self.setup_session.as_mut().expect("checked above");
            let entrance = if self.setup_animations_enabled {
                animation_progress(session.opened_at, SETUP_GUIDE_ENTRANCE_DURATION)
            } else {
                1.0
            };
            let page_opacity = if self.setup_animations_enabled {
                session.transition_opacity(now, SETUP_GUIDE_STEP_DURATION)
            } else {
                1.0
            };
            (
                session.step,
                entrance,
                page_opacity,
                session.pending_step.is_some(),
            )
        };
        if entrance < 1.0 || transition_active {
            context.request_repaint();
        }

        let snapshot = self.snapshot();
        self.update_setup_reference_capture(&snapshot);
        let textures = self.setup_example_textures(context);
        let reference_available = snapshot.previews.reference.is_some()
            || matches!(
                self.setup_reference_capture,
                SetupReferenceCaptureState::Confirmed
            );
        let next_enabled = step != SetupStep::Reference
            || (reference_available
                && !setup_reference_requires_decision(self.setup_reference_capture));
        let mut action = if transition_active {
            None
        } else {
            setup_guide_keyboard_action(context, step, next_enabled)
        };

        let full_rect = ui.max_rect();
        let background = ui.visuals().panel_fill;
        ui.painter().rect_filled(full_rect, 0, background);
        let content_width = (full_rect.width() - 64.0).clamp(360.0, SETUP_GUIDE_CONTENT_WIDTH);
        let footer_rect = Rect::from_min_max(
            Pos2::new(
                full_rect.left(),
                full_rect.bottom() - SETUP_GUIDE_FOOTER_HEIGHT,
            ),
            full_rect.max,
        );
        let body_rect = Rect::from_min_max(
            full_rect.min,
            Pos2::new(full_rect.right(), footer_rect.top()),
        );

        egui::Area::new(egui::Id::new(("setup-guide-content", step.number())))
            .fixed_pos(body_rect.center())
            .pivot(Align2::CENTER_CENTER)
            .movable(false)
            .constrain_to(body_rect.shrink(12.0))
            .show(context, |body_ui| {
                body_ui.set_width(content_width);
                body_ui.set_max_height((body_rect.height() - 24.0).max(200.0));
                body_ui.set_opacity(entrance * page_opacity);
                body_ui.with_layout(egui::Layout::top_down(egui::Align::Center), |body_ui| {
                    setup_step_title(body_ui, step);
                    body_ui.add_space(5.0);
                    body_ui.allocate_ui_with_layout(
                        egui::vec2(content_width.min(700.0), 1.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(tr(ui, step.explanation()))
                                        .size(14.0)
                                        .line_height(Some(20.0))
                                        .color(mix_color(background, SETUP_SIGNAL_WHITE, 0.66)),
                                )
                                .wrap()
                                .halign(egui::Align::Center),
                            );
                        },
                    );
                    body_ui.add_space(if body_rect.height() < 520.0 {
                        8.0
                    } else {
                        14.0
                    });
                    if step == SetupStep::HowItWorks {
                        self.setup_how_it_works(
                            body_ui,
                            &textures,
                            session_elapsed(self.setup_session, now),
                            self.setup_animations_enabled,
                            background,
                        );
                    } else {
                        let max_scroll_height = body_ui.available_height().max(160.0);
                        egui::ScrollArea::vertical()
                            .id_salt(("setup-guide-body", step.number()))
                            .auto_shrink([false, true])
                            .max_height(max_scroll_height)
                            .show(body_ui, |ui| {
                                ui.set_width(content_width);
                                if let Some(content_action) =
                                    self.setup_step_content(ui, step, &snapshot, &textures)
                                {
                                    action = Some(content_action);
                                }
                            });
                    }
                });
            });

        ui.painter().line_segment(
            [
                Pos2::new(footer_rect.left(), footer_rect.top()),
                Pos2::new(footer_rect.right(), footer_rect.top()),
            ],
            Stroke::new(1.0, mix_color(background, SETUP_SIGNAL_WHITE, 0.09)),
        );
        let footer_content_rect = Rect::from_center_size(
            footer_rect.center(),
            egui::vec2(content_width, footer_rect.height()),
        );
        let next_width = if step.is_last() { 180.0 } else { 146.0 };
        let back_width = 40.0;
        let actions_width = back_width + 10.0 + next_width;
        let left_rect = Rect::from_center_size(
            Pos2::new(
                footer_content_rect.left() + actions_width / 2.0,
                footer_content_rect.center().y,
            ),
            egui::vec2(actions_width, 40.0),
        );
        let actions_rect = Rect::from_center_size(
            Pos2::new(
                footer_content_rect.right() - actions_width / 2.0,
                footer_content_rect.center().y,
            ),
            egui::vec2(actions_width, 40.0),
        );
        let mut later_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("setup-guide-later")
                .max_rect(left_rect)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::TopDown,
                )),
        );
        later_ui.set_opacity(entrance);
        if setup_footer_button(
            &mut later_ui,
            UiIcon::Clock,
            "Set up later",
            egui::vec2(actions_width, 40.0),
            SetupFooterButtonStyle::Secondary,
            true,
        )
        .clicked()
        {
            action = Some(SetupAction::Close);
        }

        let progress_rect =
            Rect::from_center_size(footer_content_rect.center(), egui::vec2(292.0, 52.0));
        let mut progress_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("setup-guide-progress")
                .max_rect(progress_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        progress_ui.set_opacity(entrance);
        if let Some(destination) = setup_footer_progress(&mut progress_ui, step, next_enabled) {
            action = Some(SetupAction::GoTo(destination));
        }

        let mut actions_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("setup-guide-actions")
                .max_rect(actions_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        actions_ui.set_opacity(entrance);
        if setup_footer_button(
            &mut actions_ui,
            UiIcon::Back,
            "Back",
            egui::vec2(back_width, 40.0),
            SetupFooterButtonStyle::IconOnly,
            step.previous().is_some(),
        )
        .clicked()
        {
            action = Some(SetupAction::Previous);
        }
        actions_ui.add_space(10.0);
        let label = if step.is_last() {
            "Start StageSwap"
        } else {
            "Continue"
        };
        let icon = if step.is_last() {
            UiIcon::Play
        } else {
            UiIcon::ArrowRight
        };
        if setup_footer_button(
            &mut actions_ui,
            icon,
            label,
            egui::vec2(next_width, 40.0),
            SetupFooterButtonStyle::Primary {
                icon_after: !step.is_last(),
            },
            next_enabled,
        )
        .clicked()
        {
            action = Some(SetupAction::Next);
        }

        if let Some(action) = action {
            self.apply_setup_action(action);
        }
    }

    fn setup_step_content(
        &mut self,
        ui: &mut egui::Ui,
        step: SetupStep,
        snapshot: &AppSnapshot,
        textures: &SetupExampleTextures,
    ) -> Option<SetupAction> {
        match step {
            SetupStep::HowItWorks => {
                self.setup_how_it_works(
                    ui,
                    textures,
                    None,
                    self.setup_animations_enabled,
                    ui.visuals().panel_fill,
                );
                None
            }
            SetupStep::Webcam => {
                self.setup_webcam_step(ui, snapshot);
                None
            }
            SetupStep::Screen => {
                self.setup_screen_step(ui, snapshot);
                None
            }
            SetupStep::Reference => self.setup_reference_step(ui, snapshot, textures),
            SetupStep::Ready => {
                self.setup_ready_step(ui, snapshot);
                None
            }
        }
    }

    fn setup_how_it_works(
        &mut self,
        ui: &mut egui::Ui,
        textures: &SetupExampleTextures,
        elapsed: Option<Duration>,
        animations_enabled: bool,
        background: Color32,
    ) {
        if !animations_enabled {
            setup_static_switching_demo(ui, textures, background);
            return;
        }

        let preview_alpha = self.setup_demo_preview_alpha();
        let changed_alpha =
            preview_alpha.unwrap_or_else(|| elapsed.map_or(0.0, setup_demo_changed_alpha));
        if elapsed.is_some() && preview_alpha.is_none() {
            ui.ctx().request_repaint();
        }
        setup_animated_switching_demo(ui, textures, changed_alpha, background);
    }

    fn setup_webcam_step(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let selected_available = snapshot
            .video_devices
            .iter()
            .any(|device| device.id == self.config.selected_video_device_id);
        let empty_message =
            if !self.config.selected_video_device_id.is_empty() && !selected_available {
                "This webcam is unavailable. Choose another one or refresh the list."
            } else {
                "No webcam found. Connect a webcam, then refresh the list."
            };
        let selected_name = snapshot
            .video_devices
            .iter()
            .find(|device| device.id == self.config.selected_video_device_id)
            .map(|device| device.name.clone())
            .unwrap_or_else(|| tr(ui, "No camera selected").into_owned());
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(SETUP_HARDWARE_PREVIEW_WIDTH);
                self.settings_preview_panel(
                    ui,
                    SettingsPreview {
                        kind: PreviewKind::Webcam,
                        frame: snapshot.previews.webcam.as_ref(),
                        label: "Webcam preview",
                        empty_message,
                        actual_output: snapshot.actual_output,
                    },
                    SETUP_HARDWARE_PREVIEW_WIDTH,
                );
                ui.add_space(13.0);
                setup_control_label(ui, UiIcon::Camera, "Webcam", SETTINGS_BLUE);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let geometry = selector_utility_geometry(
                        SETUP_HARDWARE_PREVIEW_WIDTH,
                        ui.spacing().item_spacing.x,
                    );
                    let previous = self.config.selected_video_device_id.clone();
                    egui::ComboBox::from_id_salt("setup-webcam-selector")
                        .width(geometry.selector_width)
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for device in snapshot.video_devices.iter() {
                                ui.selectable_value(
                                    &mut self.config.selected_video_device_id,
                                    device.id.clone(),
                                    &device.name,
                                );
                            }
                        });
                    if self.config.selected_video_device_id != previous {
                        self.awaiting_video_device_id =
                            Some(self.config.selected_video_device_id.clone());
                        self.queue_settings_save();
                    }
                    if icon_only_button(
                        ui,
                        UiIcon::Refresh,
                        "Refresh webcams",
                        egui::vec2(geometry.action_width, 32.0),
                    )
                    .on_hover_text(tr(ui, "Refresh webcams"))
                    .clicked()
                    {
                        self.send(Command::RefreshVideoDevices);
                    }
                });
                if snapshot.previews.webcam.is_some() && !selected_available {
                    ui.add_space(10.0);
                    setup_message(ui, empty_message, LIVE_RED);
                }
            },
        );
    }

    fn setup_screen_step(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let saved_available = snapshot
            .monitors
            .iter()
            .any(|monitor| monitor.label == self.config.selected_monitor_label);
        let empty_message = if !self.config.selected_monitor_label.is_empty() && !saved_available {
            "This secondary screen is unavailable. Choose another one or rescan."
        } else {
            "No screen found. Connect the secondary screen used by JW Library, then rescan."
        };
        let selected_name = snapshot
            .selected_monitor
            .as_ref()
            .map(|monitor| monitor.label.clone())
            .unwrap_or_else(|| tr(ui, "No display selected").into_owned());
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(SETUP_HARDWARE_PREVIEW_WIDTH);
                self.settings_preview_panel(
                    ui,
                    SettingsPreview {
                        kind: PreviewKind::Screen,
                        frame: snapshot.previews.screen.as_ref(),
                        label: "Secondary screen preview",
                        empty_message,
                        actual_output: snapshot.actual_output,
                    },
                    SETUP_HARDWARE_PREVIEW_WIDTH,
                );
                ui.add_space(13.0);
                setup_control_label(ui, UiIcon::Monitor, "Secondary screen", SETTINGS_BLUE);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let geometry = selector_utility_geometry(
                        SETUP_HARDWARE_PREVIEW_WIDTH,
                        ui.spacing().item_spacing.x,
                    );
                    egui::ComboBox::from_id_salt("setup-screen-selector")
                        .width(geometry.selector_width)
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for monitor in snapshot.monitors.iter() {
                                let label = format!(
                                    "{} — {}×{}",
                                    monitor.label, monitor.width, monitor.height
                                );
                                if ui
                                    .selectable_label(
                                        snapshot.selected_monitor.as_ref() == Some(monitor),
                                        label,
                                    )
                                    .clicked()
                                {
                                    self.awaiting_monitor_label = Some(monitor.label.clone());
                                    self.send(Command::SelectMonitor(monitor.clone()));
                                }
                            }
                        });
                    if icon_only_button(
                        ui,
                        UiIcon::Refresh,
                        "Rescan screens",
                        egui::vec2(geometry.action_width, 32.0),
                    )
                    .on_hover_text(tr(ui, "Rescan screens"))
                    .clicked()
                    {
                        self.send(Command::Rescan);
                    }
                });
                if snapshot.previews.screen.is_some()
                    && (!saved_available || snapshot.selected_monitor.is_none())
                {
                    ui.add_space(10.0);
                    setup_message(ui, empty_message, LIVE_RED);
                }
            },
        );
    }

    fn setup_reference_step(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &AppSnapshot,
        textures: &SetupExampleTextures,
    ) -> Option<SetupAction> {
        let mut action = None;
        let reference_available = snapshot.previews.reference.is_some();
        let screen_selected = snapshot.selected_monitor.is_some();
        let screen_ready = screen_selected && snapshot.previews.screen.is_some();
        let screen_link_label = if screen_selected {
            "Change display"
        } else {
            "Choose a display"
        };

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(SETUP_HARDWARE_PREVIEW_WIDTH);
                setup_reference_example_thumbnail(
                    ui,
                    textures.reference.id(),
                    egui::vec2(
                        SETUP_HARDWARE_PREVIEW_WIDTH,
                        SETUP_HARDWARE_PREVIEW_WIDTH / WINDOW_ASPECT_RATIO,
                    ),
                );
                ui.add_space(7.0);
                icon_text(
                    ui,
                    UiIcon::Image,
                    "Example reference image",
                    ui_theme::TEXT_SECONDARY,
                    false,
                );
                ui.add_space(13.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 18.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        setup_control_label(ui, UiIcon::Capture, "Reference image", SETTINGS_BLUE);
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if setup_link(ui, screen_link_label) {
                                    action = Some(SetupAction::GoTo(SetupStep::Screen));
                                }
                            },
                        );
                    },
                );
                ui.add_space(6.0);
                let capture_label = setup_reference_capture_label(reference_available);
                let capture_enabled = screen_ready
                    && !setup_reference_requires_decision(self.setup_reference_capture);
                let response = ui
                    .add_enabled_ui(capture_enabled, |ui| {
                        accent_icon_button(
                            ui,
                            UiIcon::Capture,
                            capture_label,
                            egui::vec2(ui.available_width(), 32.0),
                            SETTINGS_BLUE,
                        )
                    })
                    .inner;
                if response.clicked() {
                    self.begin_reference_capture();
                }
                if matches!(
                    self.setup_reference_capture,
                    SetupReferenceCaptureState::CaptureFailed
                ) {
                    ui.add_space(10.0);
                    setup_message(
                        ui,
                        "StageSwap couldn’t capture the screen. Check the screen preview and try again.",
                        LIVE_RED,
                    );
                }
            },
        );

        action
    }

    fn setup_reference_example_rail(
        &mut self,
        ui: &mut egui::Ui,
        textures: &SetupExampleTextures,
        width: f32,
    ) {
        egui::Frame::new()
            .fill(SETUP_SIGNAL_DECK)
            .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
            .corner_radius(10)
            .inner_margin(14)
            .show(ui, |ui| {
                let content_width = (width - 28.0).max(1.0);
                ui.set_width(content_width);
                ui.set_height(SETUP_REFERENCE_CARD_HEIGHT - 28.0);
                setup_control_label(ui, UiIcon::Image, "Example reference image", SETTINGS_BLUE);
                ui.add_space(10.0);
                let thumbnail_size = egui::vec2(content_width, content_width / WINDOW_ASPECT_RATIO);
                setup_reference_example_thumbnail(ui, textures.reference.id(), thumbnail_size);
                ui.add_space(10.0);
                setup_capture_help(
                    ui,
                    "The screen JW Library shows when no media is playing should look like this example.",
                );
            });
    }

    fn setup_ready_step(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let webcam_ready = snapshot.webcam_state == DeviceState::Ready
            && snapshot.previews.webcam.is_some()
            && !snapshot.selected_video_device_id.is_empty();
        let screen_ready = snapshot.screen_state == DeviceState::Ready
            && snapshot.previews.screen.is_some()
            && snapshot.selected_monitor.is_some();
        let reference_ready = snapshot.previews.reference.is_some();
        let rows = [
            (
                UiIcon::Camera,
                webcam_ready,
                "Webcam ready",
                "Webcam not selected",
            ),
            (
                UiIcon::Monitor,
                screen_ready,
                "Secondary screen ready",
                "Secondary screen not selected",
            ),
            (
                UiIcon::Image,
                reference_ready,
                "Reference image ready",
                "Reference image not captured",
            ),
        ];
        let width = ui.available_width().min(620.0);
        let row_count = rows.len();
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 1.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                egui::Frame::new()
                    .fill(SETUP_SIGNAL_DECK)
                    .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        ui.set_width(width - 32.0);
                        for (index, (icon, ready, ready_text, missing_text)) in
                            rows.into_iter().enumerate()
                        {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 48.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let (icon_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(20.0, 20.0),
                                        Sense::hover(),
                                    );
                                    ui_icon::paint(
                                        ui.painter(),
                                        icon_rect,
                                        if ready { UiIcon::Check } else { icon },
                                        if ready {
                                            ACTIVE_GREEN
                                        } else {
                                            TRANSITION_AMBER
                                        },
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(tr(
                                            ui,
                                            if ready { ready_text } else { missing_text },
                                        ))
                                        .size(13.0)
                                        .color(SETUP_SIGNAL_WHITE),
                                    );
                                },
                            );
                            if index + 1 < row_count {
                                ui.separator();
                            }
                        }
                    });
            },
        );
        ui.add_space(18.0);
        setup_warning_callout(
            ui,
            width,
            "IMPORTANT",
            "In Zoom, select Stageswap Camera as your camera before the meeting.",
        );
        if !(webcam_ready && screen_ready && reference_ready) {
            ui.add_space(18.0);
            setup_message(
                ui,
                "Some setup is missing. StageSwap will start, but Auto mode may not work as expected. You can finish the guided setup later in Settings.",
                TRANSITION_AMBER,
            );
        }
    }

    fn update_setup_reference_capture(&mut self, snapshot: &AppSnapshot) {
        let now = Instant::now();
        match self.setup_reference_capture {
            SetupReferenceCaptureState::PreparingCandidate { started_at } => {
                if snapshot.previews.reference_candidate.is_none() {
                    self.capture_reference_candidate();
                } else if now.saturating_duration_since(started_at) >= REFERENCE_CAPTURE_TIMEOUT {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            SetupReferenceCaptureState::CapturingCandidate {
                started_at,
                previous_candidate_sequence,
            } => {
                let current_sequence = snapshot
                    .previews
                    .reference_candidate
                    .as_ref()
                    .map(|frame| frame.sequence);
                if current_sequence.is_some() && current_sequence != previous_candidate_sequence {
                    self.setup_reference_capture =
                        SetupReferenceCaptureState::Review { captured_at: now };
                } else if now.saturating_duration_since(started_at) >= REFERENCE_CAPTURE_TIMEOUT {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            SetupReferenceCaptureState::Review { .. } => {
                if snapshot.previews.reference_candidate.is_none() {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            SetupReferenceCaptureState::SavingCandidate {
                started_at,
                previous_reference_sequence,
            } => {
                let current_sequence = snapshot
                    .previews
                    .reference
                    .as_ref()
                    .map(|frame| frame.sequence);
                if current_sequence.is_some() && current_sequence != previous_reference_sequence {
                    self.setup_reference_capture = SetupReferenceCaptureState::Confirmed;
                } else if now.saturating_duration_since(started_at) >= REFERENCE_CAPTURE_TIMEOUT {
                    self.setup_reference_capture = SetupReferenceCaptureState::SaveFailed {
                        previous_reference_sequence,
                    };
                }
            }
            SetupReferenceCaptureState::SaveFailed {
                previous_reference_sequence,
            } => {
                let current_sequence = snapshot
                    .previews
                    .reference
                    .as_ref()
                    .map(|frame| frame.sequence);
                if current_sequence.is_some() && current_sequence != previous_reference_sequence {
                    self.setup_reference_capture = SetupReferenceCaptureState::Confirmed;
                } else if snapshot.previews.reference_candidate.is_none() {
                    self.setup_reference_capture = SetupReferenceCaptureState::CaptureFailed;
                }
            }
            _ => {}
        }
    }

    fn setup_example_textures(&mut self, context: &egui::Context) -> SetupExampleTextures {
        self.setup_example_textures
            .get_or_insert_with(|| SetupExampleTextures {
                reference: load_embedded_texture(
                    context,
                    "setup-reference-example",
                    include_bytes!("../assets/setup-reference-example.png"),
                ),
                webcam: load_embedded_texture(
                    context,
                    "setup-webcam-example",
                    include_bytes!("../assets/setup-webcam-example.png"),
                ),
                screen: load_embedded_texture(
                    context,
                    "setup-screen-example",
                    include_bytes!("../assets/setup-screen-example.png"),
                ),
            })
            .clone()
    }

    fn dashboard(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.snapshot();
        if let Some(kind) = self.enlarged_dashboard_preview {
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

    fn render_enlarged_dashboard_preview(
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
            self.enlarged_dashboard_preview = None;
        }
    }

    fn preview_workspace(
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

    fn app_icon_texture(&mut self, context: &egui::Context) -> egui::TextureId {
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

    fn preview_grid(
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

    fn controls_workspace(
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

    fn controls_body(
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

    fn settings_view(&mut self, context: &egui::Context, ui: &mut egui::Ui) {
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

    fn settings_workspace(&mut self, ui: &mut egui::Ui) -> bool {
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

    fn settings_sidebar(&mut self, ui: &mut egui::Ui, height: f32) -> SettingsSidebarLayout {
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

    fn settings_content(&mut self, ui: &mut egui::Ui) {
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

    fn general_settings(&mut self, ui: &mut egui::Ui) {
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

    fn webcam_settings(&mut self, ui: &mut egui::Ui) {
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

    fn screen_settings(&mut self, ui: &mut egui::Ui) {
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

    fn matching_settings(&mut self, ui: &mut egui::Ui) {
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

    fn updates_settings(&mut self, ui: &mut egui::Ui) {
        let status = update_status_text(self.locale(), &self.update_status);
        settings_current_version_card(ui, &status, &self.update_status);
        ui.add_space(12.0);

        let busy = self.update_busy();
        let update_available = matches!(self.update_status, UpdateStatus::Available(_));
        let (action_icon, action_label) = if update_available {
            (UiIcon::Download, "Install update")
        } else {
            (UiIcon::Refresh, "Check for updates")
        };
        let action_width = ui.available_width();
        let action = ui
            .add_enabled_ui(!busy, |ui| {
                icon_button(
                    ui,
                    action_icon,
                    action_label,
                    egui::vec2(action_width, 38.0),
                    false,
                    true,
                )
            })
            .inner;
        if action.clicked() {
            if update_available {
                self.install_available_update();
            } else {
                self.request_update_check(true);
            }
        }

        if self.config.update_channel == UpdateChannel::Beta {
            ui.add_space(8.0);
            let choose = ui
                .add_enabled_ui(!busy, |ui| {
                    icon_button(
                        ui,
                        UiIcon::Download,
                        "Install a specific version",
                        egui::vec2(action_width, 38.0),
                        false,
                        true,
                    )
                })
                .inner;
            if choose.clicked() {
                self.request_update_versions();
            }
        }

        ui.add_space(18.0);

        settings_section_heading(
            ui,
            UiIcon::Settings,
            "Update settings",
            "Choose the update channel and notification preferences.",
        );

        let previous_channel = self.config.update_channel;
        let locale = self.locale();
        settings_fixed_control_row(
            ui,
            "Update channel",
            "Stable releases are recommended. Beta may include unfinished changes.",
            166.0,
            |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    egui::ComboBox::from_id_salt("update-channel")
                        .width(166.0)
                        .selected_text(update_channel_label(self.config.update_channel, locale))
                        .show_ui(ui, |ui| {
                            for channel in [UpdateChannel::Stable, UpdateChannel::Beta] {
                                ui.selectable_value(
                                    &mut self.config.update_channel,
                                    channel,
                                    update_channel_label(channel, locale),
                                );
                            }
                        });
                });
            },
        );
        settings_toggle_row(
            ui,
            &mut self.config.notify_updates,
            "Notify when updates are available",
            "Show one notification for each new version on the selected channel.",
        );
        if self.config.update_channel != previous_channel {
            self.update_versions = update::VersionCatalog::Idle;
            self.selected_update_version = None;
            self.update_version_picker_open = false;
            self.update_status = UpdateStatus::Idle;
            self.request_update_check(false);
        }
    }

    /// Uses the same modal surface and action buttons as the other application dialogs.
    fn update_version_picker(&mut self, context: &egui::Context) {
        if !self.update_version_picker_open {
            return;
        }
        let locale = self.locale();
        let mut retry = false;
        let mut cancel = false;
        let mut install = false;
        let response = egui::Modal::new(egui::Id::new("update-version-picker"))
            .frame(
                egui::Frame::new()
                    .fill(ui_theme::SURFACE_DEEP)
                    .stroke(Stroke::new(1.0, ui_theme::BORDER_SUBTLE))
                    .corner_radius(12)
                    .inner_margin(24)
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 28,
                        spread: 2,
                        color: Color32::from_black_alpha(150),
                    }),
            )
            .backdrop_color(Color32::from_black_alpha(145))
            .show(context, |ui| {
                ui.set_width(480.0_f32.min((context.content_rect().width() - 80.0).max(240.0)));
                ui.label(
                    RichText::new(tr(ui, "Install a specific version"))
                        .size(19.0)
                        .strong(),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format_text(
                        locale,
                        "Current version: {0}",
                        &[env!("CARGO_PKG_VERSION")],
                    ))
                    .size(12.0)
                    .color(ui_theme::TEXT_SUBTLE),
                );
                ui.add_space(8.0);
                ui.label(tr(
                    ui,
                    "Select a release. Installation starts only after you confirm below.",
                ));
                ui.add_space(16.0);
                match &self.update_versions {
                    update::VersionCatalog::Loading | update::VersionCatalog::Idle => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr(ui, "Loading versions…"));
                        });
                    }
                    update::VersionCatalog::Failed(error) => {
                        ui.label(RichText::new(tr(ui, "Could not load versions.")).color(LIVE_RED));
                        ui.collapsing(tr(ui, "Details"), |ui| {
                            ui.label(error);
                        });
                        retry = ui.button(tr(ui, "Retry")).clicked();
                    }
                    update::VersionCatalog::Ready(versions) if versions.is_empty() => {
                        ui.label(tr(ui, "No installable versions found."));
                        retry = ui.button(tr(ui, "Retry")).clicked();
                    }
                    update::VersionCatalog::Ready(versions) => {
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                for release in versions {
                                    let current =
                                        release.version.to_string() == env!("CARGO_PKG_VERSION");
                                    let track = update_channel_label(
                                        if release.prerelease {
                                            UpdateChannel::Beta
                                        } else {
                                            UpdateChannel::Stable
                                        },
                                        locale,
                                    );
                                    let mut label = format!("{}  ·  {track}", release.version);
                                    if current {
                                        label.push_str(&format!("  ·  {}", tr(ui, "Installed")));
                                    }
                                    ui.add_enabled_ui(!current, |ui| {
                                        ui.selectable_value(
                                            &mut self.selected_update_version,
                                            Some(release.version),
                                            label,
                                        );
                                    });
                                }
                            });
                    }
                }
                let selected = self.selected_version_release();
                ui.add_space(16.0);
                if let Some(release) = &selected {
                    ui.label(format_text(
                        locale,
                        "StageSwap {0} will be installed and the app will restart.",
                        &[&release.version.to_string()],
                    ));
                } else {
                    ui.label(
                        RichText::new(tr(ui, "Choose a version to enable installation."))
                            .size(12.0)
                            .color(ui_theme::TEXT_SUBTLE),
                    );
                }
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(12.0);
                let width = dialog_action_width(ui.available_width(), 8.0, 2);
                ui.horizontal(|ui| {
                    cancel = dialog_button(
                        ui,
                        UiIcon::Back,
                        "Cancel",
                        DialogButtonTone::Secondary,
                        width,
                    )
                    .clicked();
                    install = ui
                        .add_enabled_ui(selected.is_some(), |ui| {
                            dialog_button(
                                ui,
                                UiIcon::Download,
                                "Install this version",
                                DialogButtonTone::Primary,
                                width,
                            )
                        })
                        .inner
                        .clicked();
                });
            });
        if cancel || response.should_close() {
            self.update_version_picker_open = false;
            self.selected_update_version = None;
        } else if install {
            if let Some(release) = self.selected_version_release() {
                self.update_version_picker_open = false;
                self.selected_update_version = None;
                self.install_release(release);
            }
        } else if retry {
            self.request_update_versions();
        }
    }

    fn diagnostics_settings(&mut self, ui: &mut egui::Ui) {
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

    fn settings_preview_control_row(
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

    fn settings_single_preview(&mut self, ui: &mut egui::Ui, preview: SettingsPreview<'_>) -> Rect {
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

    fn settings_preview_panel(
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

    fn dialog(&mut self, context: &egui::Context) {
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

    fn reference_capture_dialog_content(
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

    fn reference_capture_candidate_surface(
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

    fn preview_cell(
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
            self.enlarged_dashboard_preview = Some(kind);
        }
        ui.add_space(8.0);
        preview_caption(ui, kind);
        ui.add_space(16.0);
        preview_rect
    }

    fn preview(
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

    fn render_preview_content(
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
                    .preview_converters
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
                if let Some(texture) = self.textures.get_mut(kind.key()) {
                    if texture.source != prepared.id || texture.size != prepared.size {
                        texture.texture.set(prepared.image, TextureOptions::LINEAR);
                        texture.source = prepared.id;
                        texture.size = prepared.size;
                    }
                } else {
                    let texture =
                        ui.ctx()
                            .load_texture(kind.key(), prepared.image, TextureOptions::LINEAR);
                    self.textures.insert(
                        kind.key(),
                        PreviewTexture {
                            source: prepared.id,
                            size: prepared.size,
                            texture,
                        },
                    );
                }
            }
            if let Some(texture) = self.textures.get(kind.key()) {
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

fn allocate_reference_dialog_column(
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

fn allocate_reference_dialog_sized_column(
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

fn reference_dialog_example_top_offset(row_top: f32, preview_rect: Rect) -> f32 {
    (preview_rect.center().y - row_top - SETUP_REFERENCE_CARD_HEIGHT / 2.0).max(0.0)
}

fn compact_reference_example_card(ui: &mut egui::Ui, textures: &SetupExampleTextures, width: f32) {
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

fn reference_dialog_actions(
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

fn dialog_content(
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

fn dialog_header(ui: &mut egui::Ui, kind: AppDialogKind) {
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

fn admin_dialog_content(
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

fn dialog_button(
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

fn dialog_action_width(available_width: f32, gap: f32, action_count: usize) -> f32 {
    ((available_width - gap * (action_count.saturating_sub(1) as f32)) / action_count as f32)
        .max(1.0)
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
        if !self.update_check_started {
            self.update_check_started = true;
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

#[cfg(not(windows))]
fn save_ui_screenshot(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_array());
    }
    let source = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba)
        .ok_or_else(|| "screenshot pixel buffer had an invalid size".to_string())?;
    let output = if source.dimensions() == (1280, 720) {
        source
    } else {
        image::imageops::resize(&source, 1280, 720, image::imageops::FilterType::Lanczos3)
    };
    output
        .save(path)
        .map_err(|error| format!("could not save {}: {error}", path.display()))
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

fn translated_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let label = tr(ui, label);
    ui.button(label.as_ref())
}

fn translated_small_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let label = tr(ui, label);
    ui.small_button(label.as_ref())
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

fn session_elapsed(session: Option<SetupSession>, now: Instant) -> Option<Duration> {
    let session = session?;
    (session.step == SetupStep::HowItWorks)
        .then(|| {
            session
                .step_changed_at
                .map(|start| now.saturating_duration_since(start))
        })
        .flatten()
}

fn setup_demo_changed_alpha(elapsed: Duration) -> f32 {
    let phase = elapsed.as_secs_f32() % SETUP_DEMO_LOOP_DURATION;
    if phase < SETUP_DEMO_HOLD_DURATION {
        0.0
    } else if phase < SETUP_DEMO_HOLD_DURATION + SETUP_DEMO_FADE_DURATION {
        (phase - SETUP_DEMO_HOLD_DURATION) / SETUP_DEMO_FADE_DURATION
    } else if phase < SETUP_DEMO_HOLD_DURATION * 2.0 + SETUP_DEMO_FADE_DURATION {
        1.0
    } else {
        1.0 - (phase - (SETUP_DEMO_HOLD_DURATION * 2.0 + SETUP_DEMO_FADE_DURATION))
            / SETUP_DEMO_FADE_DURATION
    }
    .clamp(0.0, 1.0)
}

fn setup_reference_flash_alpha(elapsed: Duration, animations_enabled: bool) -> f32 {
    if animations_enabled && elapsed < SETUP_REFERENCE_FLASH_DURATION {
        1.0 - elapsed.as_secs_f32() / SETUP_REFERENCE_FLASH_DURATION.as_secs_f32()
    } else {
        0.0
    }
    .clamp(0.0, 1.0)
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

fn setup_step_title(ui: &mut egui::Ui, step: SetupStep) -> Rect {
    let title = tr(ui, step.title()).into_owned();
    let title_width = ui
        .painter()
        .layout_no_wrap(
            title.clone(),
            FontId::proportional(30.0),
            SETUP_SIGNAL_WHITE,
        )
        .size()
        .x;
    let title_group_width = 28.0 + 8.0 + title_width;

    ui.allocate_ui_with_layout(
        egui::vec2(title_group_width, 36.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
            ui_icon::paint(
                ui.painter(),
                icon_rect.shrink(1.0),
                setup_step_icon(step),
                SETTINGS_BLUE,
            );
            ui.label(
                RichText::new(title)
                    .size(30.0)
                    .strong()
                    .color(SETUP_SIGNAL_WHITE),
            );
        },
    )
    .response
    .rect
}

fn setup_guide_keyboard_action(
    context: &egui::Context,
    step: SetupStep,
    next_enabled: bool,
) -> Option<SetupAction> {
    context.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            return Some(SetupAction::Close);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
            return step.previous().map(|_| SetupAction::Previous);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
        {
            return next_enabled.then_some(SetupAction::Next);
        }
        None
    })
}

fn load_embedded_texture(
    context: &egui::Context,
    name: &'static str,
    bytes: &[u8],
) -> TextureHandle {
    let image = image::load_from_memory(bytes)
        .unwrap_or_else(|error| panic!("{name} should be a valid embedded image: {error}"))
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    context.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        TextureOptions::LINEAR,
    )
}

fn setup_footer_progress(
    ui: &mut egui::Ui,
    step: SetupStep,
    forward_enabled: bool,
) -> Option<SetupStep> {
    let background = ui.visuals().panel_fill;
    let step_text = format_text(
        ui_locale(ui),
        "Step {0} of {1}",
        &[
            &step.number().to_string(),
            &SetupStep::ALL.len().to_string(),
        ],
    );
    ui.label(
        RichText::new(step_text)
            .size(12.0)
            .strong()
            .color(mix_color(background, SETUP_SIGNAL_WHITE, 0.78)),
    );
    ui.add_space(3.0);

    let width = 268.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), Sense::hover());
    let first_x = rect.left() + 12.0;
    let last_x = rect.right() - 12.0;
    let center_y = rect.center().y;
    let step_gap = (last_x - first_x) / (SetupStep::ALL.len() - 1) as f32;
    let rail = Rect::from_min_max(
        Pos2::new(first_x, center_y - 1.0),
        Pos2::new(last_x, center_y + 1.0),
    );
    ui.painter()
        .rect_filled(rail, 1, mix_color(background, SETUP_SIGNAL_WHITE, 0.13));
    if step.number() > 1 {
        let active_x = first_x + step_gap * (step.number() - 1) as f32;
        ui.painter().rect_filled(
            Rect::from_min_max(rail.min, Pos2::new(active_x, rail.bottom())),
            1,
            SETTINGS_BLUE,
        );
    }

    let mut destination = None;
    for (index, candidate) in SetupStep::ALL.into_iter().enumerate() {
        let number = index + 1;
        let enabled = forward_enabled || number <= step.number();
        let center = Pos2::new(first_x + step_gap * index as f32, center_y);
        let hit_rect = Rect::from_center_size(center, egui::vec2(34.0, 24.0));
        let response = ui.interact(
            hit_rect,
            egui::Id::new(("setup-progress-step", number)),
            Sense::click(),
        );
        let is_current = candidate == step;
        let is_complete = number < step.number();
        let hovered = enabled && response.hovered() && !is_current;
        let node_fill = if is_current {
            SETTINGS_BLUE
        } else if is_complete {
            mix_color(SETTINGS_BLUE, SETUP_SIGNAL_WHITE, 0.08)
        } else if !enabled {
            mix_color(background, SETUP_SIGNAL_WHITE, 0.07)
        } else if hovered {
            mix_color(background, SETUP_SIGNAL_WHITE, 0.2)
        } else {
            mix_color(background, SETUP_SIGNAL_WHITE, 0.12)
        };
        let radius = if is_current { 9.0 } else { 6.0 };

        if is_current {
            ui.painter()
                .circle_filled(center, 12.0, SETTINGS_BLUE.gamma_multiply(0.18));
        }
        ui.painter().circle_filled(center, radius, node_fill);
        if !is_complete {
            ui.painter().text(
                center,
                Align2::CENTER_CENTER,
                number.to_string(),
                FontId::proportional(if is_current { 10.5 } else { 9.0 }),
                if is_current {
                    SETUP_SIGNAL_WHITE
                } else {
                    mix_color(background, SETUP_SIGNAL_WHITE, 0.62)
                },
            );
        } else {
            ui_icon::paint(
                ui.painter(),
                Rect::from_center_size(center, egui::vec2(8.0, 8.0)),
                UiIcon::Check,
                SETUP_SIGNAL_WHITE,
            );
        }

        let label = tr(ui, candidate.title());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                enabled,
                is_current,
                label.as_ref(),
            )
        });
        let response = response.on_hover_text(label);
        let response = if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        };
        if enabled && response.clicked() && !is_current {
            destination = Some(candidate);
        }
    }
    destination
}

fn setup_rounded_image(
    ui: &egui::Ui,
    rect: Rect,
    texture: egui::TextureId,
    opacity: f32,
    background: Color32,
    corner_radius: u8,
) {
    egui::Image::new((texture, rect.size()))
        .corner_radius(corner_radius)
        .tint(Color32::WHITE.gamma_multiply(opacity))
        .paint_at(ui, rect);
    ui.painter().rect_stroke(
        rect,
        corner_radius,
        Stroke::new(1.0, mix_color(background, SETUP_SIGNAL_WHITE, 0.15)),
        StrokeKind::Inside,
    );
}

fn setup_crossfade_image(
    ui: &egui::Ui,
    rect: Rect,
    first: egui::TextureId,
    second: egui::TextureId,
    blend: f32,
    background: Color32,
) {
    egui::Image::new((first, rect.size()))
        .corner_radius(9)
        .tint(Color32::WHITE.gamma_multiply(1.0 - blend))
        .paint_at(ui, rect);
    if blend > 0.0 {
        egui::Image::new((second, rect.size()))
            .corner_radius(9)
            .tint(Color32::WHITE.gamma_multiply(blend))
            .paint_at(ui, rect);
    }
    ui.painter().rect_stroke(
        rect,
        9,
        Stroke::new(1.0, mix_color(background, SETUP_SIGNAL_WHITE, 0.15)),
        StrokeKind::Inside,
    );
}

fn setup_animated_switching_demo(
    ui: &mut egui::Ui,
    textures: &SetupExampleTextures,
    changed_alpha: f32,
    background: Color32,
) {
    let compact = ui.available_height() < 420.0;
    let input_width = if compact { 120.0 } else { 150.0 };
    let output_width = if compact { 300.0 } else { 360.0 };
    let input_height = input_width / WINDOW_ASPECT_RATIO;
    let output_height = output_width / WINDOW_ASPECT_RATIO;
    let caption_height = 16.0;
    let caption_gap = 4.0;
    let block_height = caption_height + caption_gap + input_height;
    let card_gap = if compact { 6.0 } else { 8.0 };
    let card_padding_x = 14.0;
    let card_padding_y = 8.0;
    let card_width = input_width + card_padding_x * 2.0;
    let card_height = block_height + card_padding_y * 2.0;
    let diagram_height = card_height * 2.0 + card_gap;
    let output_gap = if compact { 46.0 } else { 64.0 };
    let diagram_width = card_width + output_gap + output_width;
    let (diagram, _) =
        ui.allocate_exact_size(egui::vec2(diagram_width, diagram_height), Sense::hover());

    let content_left = diagram.left() + card_padding_x;
    let screen_caption = Rect::from_min_size(
        Pos2::new(content_left, diagram.top() + card_padding_y),
        egui::vec2(input_width, caption_height),
    );
    let screen = Rect::from_min_size(
        Pos2::new(content_left, screen_caption.bottom() + caption_gap),
        egui::vec2(input_width, input_height),
    );
    let screen_card = Rect::from_min_max(
        diagram.min,
        Pos2::new(diagram.left() + card_width, diagram.top() + card_height),
    );
    let webcam_caption = Rect::from_min_size(
        Pos2::new(
            content_left,
            screen_card.bottom() + card_gap + card_padding_y,
        ),
        egui::vec2(input_width, caption_height),
    );
    let webcam = Rect::from_min_size(
        Pos2::new(content_left, webcam_caption.bottom() + caption_gap),
        egui::vec2(input_width, input_height),
    );
    let webcam_card = Rect::from_min_max(
        Pos2::new(diagram.left(), screen_card.bottom() + card_gap),
        Pos2::new(
            diagram.left() + card_width,
            screen_card.bottom() + card_gap + card_height,
        ),
    );
    let output_block_height = caption_height + caption_gap + output_height;
    let output_caption = Rect::from_min_size(
        Pos2::new(
            diagram.right() - output_width,
            diagram.center().y - output_block_height / 2.0,
        ),
        egui::vec2(output_width, caption_height),
    );
    let output = Rect::from_min_size(
        Pos2::new(output_caption.left(), output_caption.bottom() + caption_gap),
        egui::vec2(output_width, output_height),
    );

    paint_setup_group_card(ui, screen_card, changed_alpha, background);
    paint_setup_group_card(ui, webcam_card, 1.0 - changed_alpha, background);

    paint_setup_centered_label(
        ui,
        screen_caption,
        "JW Library",
        mix_color(background, SETUP_SIGNAL_WHITE, 0.62),
    );
    paint_setup_centered_label(
        ui,
        webcam_caption,
        "Webcam",
        mix_color(background, SETUP_SIGNAL_WHITE, 0.62).gamma_multiply(1.0 - changed_alpha * 0.42),
    );
    paint_setup_centered_label(
        ui,
        output_caption,
        "Zoom",
        mix_color(background, SETUP_SIGNAL_WHITE, 0.76),
    );

    setup_crossfade_image(
        ui,
        screen,
        textures.reference.id(),
        textures.screen.id(),
        changed_alpha,
        background,
    );
    setup_rounded_image(
        ui,
        webcam,
        textures.webcam.id(),
        1.0 - changed_alpha * 0.42,
        background,
        8,
    );
    setup_crossfade_image(
        ui,
        output,
        textures.webcam.id(),
        textures.screen.id(),
        changed_alpha,
        background,
    );

    ui.painter().rect_stroke(
        output,
        9,
        Stroke::new(1.5, SETTINGS_BLUE.gamma_multiply(0.72)),
        StrokeKind::Inside,
    );

    ui.add_space(if compact { 4.0 } else { 8.0 });
    setup_demo_rule(ui, diagram_width.min(740.0), changed_alpha, background);
    ui.add_space(if compact { 2.0 } else { 6.0 });
    setup_plain_zoom_reminder(ui, diagram_width.min(620.0), background);
}

fn paint_setup_group_card(ui: &egui::Ui, rect: Rect, active_strength: f32, background: Color32) {
    ui.painter()
        .rect_filled(rect, 10, mix_color(background, SETUP_SIGNAL_WHITE, 0.025));
    let neutral = mix_color(background, SETUP_SIGNAL_WHITE, 0.12);
    ui.painter().rect_stroke(
        rect,
        10,
        Stroke::new(
            1.0 + active_strength * 0.5,
            mix_color(neutral, SETTINGS_BLUE, active_strength * 0.78),
        ),
        StrokeKind::Inside,
    );
}

fn paint_setup_centered_label(ui: &egui::Ui, rect: Rect, text: &str, color: Color32) {
    let text = tr(ui, text);
    let galley = ui
        .painter()
        .layout_no_wrap(text.into_owned(), FontId::proportional(11.5), color);
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

fn paint_setup_active_outline(ui: &egui::Ui, rect: Rect, strength: f32) {
    if strength > 0.0 {
        ui.painter().rect_stroke(
            rect,
            8,
            Stroke::new(1.5, SETTINGS_BLUE.gamma_multiply(strength * 0.72)),
            StrokeKind::Inside,
        );
    }
}

fn paint_setup_crossfade_text(
    ui: &egui::Ui,
    rect: Rect,
    first: &str,
    second: &str,
    blend: f32,
    font: FontId,
    color: Color32,
) {
    let first = tr(ui, first);
    let second = tr(ui, second);
    let first_color = color.gamma_multiply(1.0 - blend);
    let second_color = color.gamma_multiply(blend);
    paint_setup_text(ui, rect, first.as_ref(), font.clone(), first_color);
    if blend > 0.0 {
        paint_setup_text(ui, rect, second.as_ref(), font, second_color);
    }
}

fn paint_setup_text(ui: &egui::Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    if let Some((left, right)) = text.split_once('→') {
        let left = ui
            .painter()
            .layout_no_wrap(left.trim().to_owned(), font.clone(), color);
        let right = ui
            .painter()
            .layout_no_wrap(right.trim().to_owned(), font, color);
        let arrow_width = 24.0;
        let total_width = left.size().x + arrow_width + right.size().x;
        let top = rect.center().y - left.size().y.max(right.size().y) / 2.0;
        let left_pos = Pos2::new(rect.center().x - total_width / 2.0, top);
        ui.painter().galley(left_pos, left.clone(), color);
        let arrow_center = Pos2::new(
            left_pos.x + left.size().x + arrow_width / 2.0,
            rect.center().y,
        );
        let arrow_start = Pos2::new(arrow_center.x - 6.0, arrow_center.y);
        let arrow_end = Pos2::new(arrow_center.x + 6.0, arrow_center.y);
        ui.painter()
            .line_segment([arrow_start, arrow_end], Stroke::new(1.3, color));
        ui.painter().line_segment(
            [arrow_end, arrow_end + egui::vec2(-3.5, -3.5)],
            Stroke::new(1.3, color),
        );
        ui.painter().line_segment(
            [arrow_end, arrow_end + egui::vec2(-3.5, 3.5)],
            Stroke::new(1.3, color),
        );
        ui.painter().galley(
            Pos2::new(left_pos.x + left.size().x + arrow_width, top),
            right,
            color,
        );
    } else {
        let galley = ui
            .painter()
            .layout(text.to_owned(), font, color, rect.width());
        let pos = Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(pos, galley, color);
    }
}

fn setup_demo_rule(ui: &mut egui::Ui, width: f32, blend: f32, background: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), Sense::hover());
    paint_setup_crossfade_text(
        ui,
        rect,
        "No media in JW Library → Zoom sees the webcam",
        "Media detected in JW Library → Zoom sees the secondary screen",
        blend,
        FontId::proportional(13.0),
        mix_color(background, SETUP_SIGNAL_WHITE, 0.82),
    );
}

fn setup_plain_zoom_reminder(ui: &mut egui::Ui, width: f32, background: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::hover());
    let color = mix_color(background, SETUP_SIGNAL_WHITE, 0.76);
    let text = tr(
        ui,
        "StageSwap sends the webcam or JW Library screen to Zoom through one virtual camera.",
    );
    let galley = ui
        .painter()
        .layout_no_wrap(text.into_owned(), FontId::proportional(12.5), color);
    let icon_size = 16.0;
    let gap = 8.0;
    let total_width = icon_size + gap + galley.size().x;
    let icon_rect = Rect::from_min_size(
        Pos2::new(
            rect.center().x - total_width / 2.0,
            rect.center().y - icon_size / 2.0,
        ),
        egui::vec2(icon_size, icon_size),
    );
    ui_icon::paint(ui.painter(), icon_rect, UiIcon::Broadcast, SETTINGS_BLUE);
    ui.painter().galley(
        Pos2::new(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

fn setup_static_switching_demo(
    ui: &mut egui::Ui,
    textures: &SetupExampleTextures,
    background: Color32,
) {
    let width = ui.available_width().min(700.0);
    let row_height = (ui.available_height() / 2.0 - 18.0).clamp(105.0, 132.0);
    setup_static_signal_row(
        ui,
        width,
        row_height,
        "No media in JW Library → Zoom sees the webcam",
        textures,
        false,
        background,
    );
    ui.add_space(7.0);
    setup_static_signal_row(
        ui,
        width,
        row_height,
        "Media detected in JW Library → Zoom sees the secondary screen",
        textures,
        true,
        background,
    );
    ui.add_space(5.0);
    setup_plain_zoom_reminder(ui, width.min(620.0), background);
}

fn setup_static_signal_row(
    ui: &mut egui::Ui,
    width: f32,
    row_height: f32,
    rule: &str,
    textures: &SetupExampleTextures,
    changed: bool,
    background: Color32,
) {
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, row_height), Sense::hover());
    let rule_rect = Rect::from_min_size(row.min, egui::vec2(width, 24.0));
    paint_setup_text(
        ui,
        rule_rect,
        tr(ui, rule).as_ref(),
        FontId::proportional(12.0),
        mix_color(background, SETUP_SIGNAL_WHITE, 0.82),
    );
    let media_width = ((row_height - 47.0) * WINDOW_ASPECT_RATIO).clamp(86.0, 104.0);
    let media_height = media_width / WINDOW_ASPECT_RATIO;
    let zoom_width = media_width * 1.2;
    let stage_width = media_width * 2.0 + zoom_width + 90.0;
    let stage_left = row.center().x - stage_width / 2.0;
    let caption_top = rule_rect.bottom() + 3.0;
    let image_top = caption_top + 17.0;
    let screen = Rect::from_min_size(
        Pos2::new(stage_left, image_top),
        egui::vec2(media_width, media_height),
    );
    let webcam = Rect::from_min_size(
        Pos2::new(screen.right() + 34.0, image_top),
        egui::vec2(media_width, media_height),
    );
    let output = Rect::from_min_size(
        Pos2::new(
            row.right() - (row.width() - stage_width) / 2.0 - zoom_width,
            image_top,
        ),
        egui::vec2(zoom_width, zoom_width / WINDOW_ASPECT_RATIO),
    );
    let screen_card = Rect::from_min_max(
        Pos2::new(screen.left() - 8.0, caption_top - 5.0),
        Pos2::new(screen.right() + 8.0, screen.bottom() + 8.0),
    );
    let webcam_card = Rect::from_min_max(
        Pos2::new(webcam.left() - 8.0, caption_top - 5.0),
        Pos2::new(webcam.right() + 8.0, webcam.bottom() + 8.0),
    );
    paint_setup_group_card(ui, screen_card, f32::from(changed), background);
    paint_setup_group_card(ui, webcam_card, f32::from(!changed), background);
    let label_color = mix_color(background, SETUP_SIGNAL_WHITE, 0.62);
    for (rect, label) in [(screen, "JW Library"), (webcam, "Webcam"), (output, "Zoom")] {
        paint_setup_centered_label(
            ui,
            Rect::from_min_size(
                Pos2::new(rect.left(), caption_top),
                egui::vec2(rect.width(), 15.0),
            ),
            label,
            label_color,
        );
    }
    setup_rounded_image(
        ui,
        screen,
        if changed {
            textures.screen.id()
        } else {
            textures.reference.id()
        },
        1.0,
        background,
        7,
    );
    setup_rounded_image(
        ui,
        webcam,
        textures.webcam.id(),
        if changed { 0.58 } else { 1.0 },
        background,
        7,
    );
    setup_rounded_image(
        ui,
        output,
        if changed {
            textures.screen.id()
        } else {
            textures.webcam.id()
        },
        1.0,
        background,
        7,
    );
    paint_setup_active_outline(ui, output, 1.0);
}

#[derive(Clone, Copy)]
enum SetupFooterButtonStyle {
    Secondary,
    IconOnly,
    Primary { icon_after: bool },
}

fn setup_footer_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &str,
    size: Vec2,
    style: SetupFooterButtonStyle,
    enabled: bool,
) -> egui::Response {
    let primary = matches!(style, SetupFooterButtonStyle::Primary { .. });
    let icon_after = matches!(style, SetupFooterButtonStyle::Primary { icon_after: true });
    let icon_only = matches!(style, SetupFooterButtonStyle::IconOnly);
    let label = tr(ui, label);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let amount = if enabled { 1.0 } else { 0.34 };
    let hovered = enabled && response.hovered();
    let fill = if primary {
        if hovered {
            mix_color(SETTINGS_BLUE, SETUP_SIGNAL_WHITE, 0.12)
        } else {
            SETTINGS_BLUE
        }
    } else if hovered {
        SETUP_SIGNAL_DECK
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if primary {
        Stroke::NONE
    } else {
        Stroke::new(
            1.0,
            mix_color(SETUP_BOOTH_BLACK, SETUP_SIGNAL_WHITE, 0.18).gamma_multiply(amount),
        )
    };
    ui.painter()
        .rect_filled(rect, 7, fill.gamma_multiply(amount));
    ui.painter()
        .rect_stroke(rect, 7, stroke, StrokeKind::Inside);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            9,
            Stroke::new(2.0, SETTINGS_BLUE),
            StrokeKind::Outside,
        );
    }
    let color = SETUP_SIGNAL_WHITE.gamma_multiply(amount);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::proportional(13.5), color);
    let icon_size = 15.0;
    let icon_rect = if icon_only {
        Rect::from_center_size(rect.center(), egui::vec2(icon_size, icon_size))
    } else {
        let gap = 7.0;
        let content_width = icon_size + gap + galley.size().x;
        let content_left = rect.center().x - content_width / 2.0;
        let icon_left = if icon_after {
            content_left + galley.size().x + gap
        } else {
            content_left
        };
        Rect::from_min_size(
            Pos2::new(icon_left, rect.center().y - icon_size / 2.0),
            egui::vec2(icon_size, icon_size),
        )
    };
    ui_icon::paint(ui.painter(), icon_rect, icon, color);
    if !icon_only {
        let gap = 7.0;
        let text_left = if icon_after {
            icon_rect.left() - gap - galley.size().x
        } else {
            icon_rect.right() + gap
        };
        ui.painter().galley(
            Pos2::new(text_left, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.as_ref())
    });
    let response = if icon_only {
        response.on_hover_text(label)
    } else {
        response
    };
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn setup_reference_example_thumbnail(ui: &mut egui::Ui, texture: egui::TextureId, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, 7, ui_theme::PREVIEW_BACKGROUND);
    ui.painter().image(
        texture,
        rect.shrink(2.0),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    ui.painter().rect_stroke(
        rect,
        7,
        Stroke::new(1.0, mix_color(SETUP_BOOTH_BLACK, SETUP_SIGNAL_WHITE, 0.16)),
        StrokeKind::Inside,
    );
}

fn setup_reference_state_badge(ui: &mut egui::Ui, text: &str, tone: Color32) {
    let text = tr(ui, text);
    let galley = ui
        .painter()
        .layout_no_wrap(text.into_owned(), FontId::monospace(9.0), tone);
    let size = galley.size() + egui::vec2(12.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 4, tone.gamma_multiply(0.12));
    ui.painter().rect_stroke(
        rect,
        4,
        Stroke::new(1.0, tone.gamma_multiply(0.38)),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, tone);
}

fn setup_capture_help(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(tr(ui, text))
                .size(11.5)
                .line_height(Some(16.0))
                .color(mix_color(ui.visuals().panel_fill, SETUP_SIGNAL_WHITE, 0.66)),
        )
        .wrap(),
    );
}

fn setup_compact_state(ui: &mut egui::Ui, icon: UiIcon, text: &str, color: Color32) {
    let available = ui.available_width().max(1.0);
    let text = tr(ui, text);
    let text_width = (available - 21.0).max(1.0);
    let galley = ui.painter().layout(
        text.into_owned(),
        FontId::proportional(11.0),
        color,
        text_width,
    );
    let height = (galley.size().y + 6.0).max(30.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(available, height), Sense::hover());
    let icon_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - 7.0),
        egui::vec2(14.0, 14.0),
    );
    ui_icon::paint(ui.painter(), icon_rect, icon, color);
    let text_rect = Rect::from_min_max(Pos2::new(icon_rect.right() + 7.0, rect.top()), rect.max);
    ui.painter().galley(
        Pos2::new(
            text_rect.left(),
            text_rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

fn setup_pending_capture_button(ui: &mut egui::Ui, label: &str, size: Vec2) -> Rect {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let fill = mix_color(SETUP_SIGNAL_DECK, SETTINGS_BLUE, 0.2);
    ui.painter().rect_filled(rect, 6, fill);
    ui.painter().rect_stroke(
        rect,
        6,
        Stroke::new(1.0, SETTINGS_BLUE.gamma_multiply(0.58)),
        StrokeKind::Inside,
    );
    let label = tr(ui, label);
    let color = SETUP_SIGNAL_WHITE.gamma_multiply(0.78);
    let galley = ui
        .painter()
        .layout_no_wrap(label.into_owned(), FontId::proportional(12.0), color);
    let spinner_size = 14.0;
    let gap = 7.0;
    let content_width = spinner_size + gap + galley.size().x;
    let spinner_rect = Rect::from_min_size(
        Pos2::new(
            rect.center().x - content_width / 2.0,
            rect.center().y - spinner_size / 2.0,
        ),
        egui::vec2(spinner_size, spinner_size),
    );
    let mut spinner_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("setup-reference-capture-spinner")
            .max_rect(spinner_rect)
            .layout(egui::Layout::centered_and_justified(
                egui::Direction::TopDown,
            )),
    );
    spinner_ui.add(egui::Spinner::new().size(spinner_size).color(SETTINGS_BLUE));
    ui.painter().galley(
        Pos2::new(
            spinner_rect.right() + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    rect
}

fn setup_control_label(ui: &mut egui::Ui, icon: UiIcon, label: &str, accent: Color32) {
    ui.horizontal(|ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), Sense::hover());
        ui_icon::paint(ui.painter(), icon_rect, icon, accent);
        ui.label(
            RichText::new(tr(ui, label))
                .size(12.0)
                .strong()
                .color(SETUP_SIGNAL_WHITE),
        );
    });
}

fn setup_message(ui: &mut egui::Ui, text: &str, color: Color32) {
    let frame = egui::Frame::new()
        .fill(mix_color(SETUP_SIGNAL_DECK, color, 0.08))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.3)))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(tr(ui, text))
                        .size(11.5)
                        .line_height(Some(17.0))
                        .color(mix_color(SETUP_BOOTH_BLACK, SETUP_SIGNAL_WHITE, 0.82)),
                )
                .wrap(),
            );
        });
    let stripe = Rect::from_min_max(
        Pos2::new(frame.response.rect.left(), frame.response.rect.top() + 7.0),
        Pos2::new(
            frame.response.rect.left() + 3.0,
            frame.response.rect.bottom() - 7.0,
        ),
    );
    ui.painter().rect_filled(stripe, 2, color);
}

fn setup_link(ui: &mut egui::Ui, text: &str) -> bool {
    let text = tr(ui, text);
    ui.add(
        egui::Label::new(
            RichText::new(text.as_ref())
                .size(11.0)
                .underline()
                .color(SETTINGS_BLUE),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

fn setup_warning_callout(ui: &mut egui::Ui, width: f32, title: &str, text: &str) {
    let title = tr(ui, title);
    let text = tr(ui, text);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 1.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            egui::Frame::new()
                .fill(mix_color(SETUP_SIGNAL_DECK, TRANSITION_AMBER, 0.14))
                .stroke(Stroke::new(1.5, TRANSITION_AMBER.gamma_multiply(0.72)))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_width((width - 32.0).max(1.0));
                    ui.horizontal_top(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
                        ui_icon::paint(ui.painter(), icon_rect, UiIcon::Warning, TRANSITION_AMBER);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(title.as_ref())
                                    .size(11.0)
                                    .strong()
                                    .color(TRANSITION_AMBER),
                            );
                            ui.add_space(3.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(text.as_ref())
                                        .size(13.0)
                                        .strong()
                                        .line_height(Some(19.0))
                                        .color(SETUP_SIGNAL_WHITE),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
        },
    );
}

fn notification_bell(ui: &mut egui::Ui, unread: usize) -> bool {
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

fn notification_entry_card(ui: &mut egui::Ui, item: &NotificationItem, now: Instant) {
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

fn notification_toast_card(ui: &mut egui::Ui, item: &NotificationItem, now: Instant) {
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

fn notification_icon_and_color(
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

fn notification_title(ui: &egui::Ui, source: NotificationSource) -> std::borrow::Cow<'static, str> {
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

fn notification_age(ui: &egui::Ui, created_at: Instant, now: Instant) -> String {
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

fn controls_section_heading(ui: &mut egui::Ui, icon: UiIcon, title: &str) -> Rect {
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

fn controls_section_divider(ui: &mut egui::Ui) -> Rect {
    ui.add_space(2.0);
    let divider = ui.separator().rect;
    ui.add_space(2.0);
    divider
}

fn settings_back_button(ui: &mut egui::Ui) -> egui::Response {
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

fn settings_nav_button(
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

fn settings_section_heading(
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

fn settings_info_card(ui: &mut egui::Ui, paragraphs: &[&str]) -> Rect {
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

fn settings_current_version_card(
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

fn update_status_indicator(status: &UpdateStatus) -> (UiIcon, Color32) {
    match status {
        UpdateStatus::Idle => (UiIcon::Clock, ui_theme::TEXT_MUTED),
        UpdateStatus::Checking => (UiIcon::Loader, TRANSITION_AMBER),
        UpdateStatus::UpToDate => (UiIcon::CheckCircle, ACTIVE_GREEN),
        UpdateStatus::Available(_) => (UiIcon::Download, SETTINGS_BLUE),
        UpdateStatus::Downloading(_) | UpdateStatus::Installing => (UiIcon::Loader, SETTINGS_BLUE),
        UpdateStatus::Failed(_) => (UiIcon::Error, LIVE_RED),
    }
}

fn language_selector_text(locale: Locale) -> String {
    format!("      {}", locale.native_name())
}

fn language_flag_rect(container: Rect) -> Rect {
    Rect::from_center_size(
        Pos2::new(container.left() + 17.0, container.center().y),
        egui::vec2(18.0, 12.0),
    )
}

fn paint_language_flag(painter: &egui::Painter, rect: Rect, locale: Locale) {
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

fn settings_save_error_callout(ui: &mut egui::Ui, message: &str) {
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

fn settings_section_gap(ui: &mut egui::Ui) {
    ui.add_space(24.0);
}

fn settings_content_width(available: f32) -> f32 {
    (available - 28.0).clamp(1.0, SETTINGS_CONTENT_WIDTH)
}

fn settings_group_label(ui: &mut egui::Ui, label: &str) {
    let label = tr(ui, label);
    ui.label(
        RichText::new(label.as_ref())
            .size(11.5)
            .strong()
            .color(ui_theme::TEXT_SECONDARY),
    );
    ui.add_space(3.0);
}

fn settings_result_text(ui: &mut egui::Ui, text: &str) -> Rect {
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

fn update_channel_label(channel: UpdateChannel, locale: Locale) -> std::borrow::Cow<'static, str> {
    localized_text(
        locale,
        match channel {
            UpdateChannel::Stable => "Stable releases",
            UpdateChannel::Beta => "Beta",
        },
    )
}

fn update_status_text(locale: Locale, status: &UpdateStatus) -> String {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MatchStrictnessExplanation {
    level: &'static str,
    effect: &'static str,
}

fn match_strictness_explanation(value: f64) -> MatchStrictnessExplanation {
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

fn settings_toggle_row(ui: &mut egui::Ui, value: &mut bool, title: &str, description: &str) {
    settings_toggle_row_with_description(ui, value, title, description, true)
}

fn settings_toggle_row_without_separator(
    ui: &mut egui::Ui,
    value: &mut bool,
    title: &str,
    description: &str,
) {
    settings_toggle_row_with_description(ui, value, title, description, false)
}

fn settings_toggle_row_with_description(
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

fn reference_control_geometry(available: f32, gap: f32) -> ReferenceControlGeometry {
    ReferenceControlGeometry {
        action_width: ((available - gap) / 2.0).max(80.0),
        slider_width: available.max(1.0),
    }
}

fn match_strictness_slider(ui: &mut egui::Ui, value: &mut f64, width: f32) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = width.max(1.0);
        ui.add(egui::Slider::new(value, 0.50..=1.0).show_value(false))
    })
    .inner
}

fn selector_utility_geometry(available: f32, gap: f32) -> SelectorUtilityGeometry {
    let action_width = 32.0;
    SelectorUtilityGeometry {
        selector_width: (available - gap - action_width).max(80.0),
        action_width,
    }
}

fn settings_switch_geometry(row: Rect) -> Rect {
    Rect::from_center_size(
        Pos2::new(row.right() - 21.0, row.center().y),
        egui::vec2(42.0, 22.0),
    )
}

const fn settings_switch_colors() -> (Color32, Color32) {
    (SETTINGS_SWITCH_OFF, SETTINGS_BLUE)
}

fn settings_control_row(
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

fn settings_action_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    add_action: impl FnOnce(&mut egui::Ui),
) {
    settings_control_row(ui, title, description, add_action);
}

fn settings_fixed_control_row(
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

fn settings_single_button_row(
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

fn settings_info_row(ui: &mut egui::Ui, title: &str, value: &str) {
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

fn settings_device_status(ui: &mut egui::Ui, icon: UiIcon, label: &str, state: DeviceState) {
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

fn settings_detection_status(ui: &mut egui::Ui, state: DetectionState) {
    let (icon, status, color) = settings_detection_style(state);
    settings_status_item(ui, UiIcon::Target, "Media detection", icon, status, color);
}

fn settings_detection_style(state: DetectionState) -> (UiIcon, &'static str, Color32) {
    match state {
        DetectionState::Unknown => (UiIcon::Question, "Checking", TRANSITION_AMBER),
        DetectionState::Matching => (UiIcon::Check, "No media", ACTIVE_GREEN),
        DetectionState::NotMatching => (UiIcon::Error, "Media detected", TRANSITION_AMBER),
        DetectionState::ReferenceMissing => {
            (UiIcon::Unavailable, "Reference image missing", LIVE_RED)
        }
    }
}

fn settings_status_item(
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

const HEALTH_STATES: [DeviceState; 4] = [
    DeviceState::Initializing,
    DeviceState::Ready,
    DeviceState::Unavailable,
    DeviceState::Failed,
];

fn preview_contour(
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

fn health_state_group(
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

fn detection_state_group(ui: &mut egui::Ui, current: DetectionState) -> Rect {
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

fn detection_indicator_tone(state: DetectionState) -> IndicatorTone {
    match state {
        DetectionState::Unknown | DetectionState::NotMatching => IndicatorTone::Amber,
        DetectionState::Matching => IndicatorTone::Green,
        DetectionState::ReferenceMissing => IndicatorTone::Red,
    }
}

fn screen_mix_group(ui: &mut egui::Ui, screen_mix: f64) -> Rect {
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

fn indicator_group(
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

fn indicator_heading(
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

fn indicator_chip(
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

fn indicator_palette(tone: IndicatorTone, active_amount: f32) -> (Color32, Color32, Color32) {
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

fn device_state_icon(state: DeviceState) -> UiIcon {
    match state {
        DeviceState::Initializing => UiIcon::Loader,
        DeviceState::Ready => UiIcon::Check,
        DeviceState::Unavailable => UiIcon::Unavailable,
        DeviceState::Failed => UiIcon::Error,
    }
}

fn device_state_tone(state: DeviceState) -> IndicatorTone {
    match state {
        DeviceState::Ready => IndicatorTone::Green,
        DeviceState::Initializing => IndicatorTone::Amber,
        DeviceState::Unavailable | DeviceState::Failed => IndicatorTone::Red,
    }
}

fn friendly_device_state(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Initializing => "Initializing",
        DeviceState::Ready => "Ready",
        DeviceState::Unavailable => "Unavailable",
        DeviceState::Failed => "Failed",
    }
}

fn preview_caption(ui: &mut egui::Ui, kind: PreviewKind) {
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

fn paint_fps_overlay(ui: &egui::Ui, preview_rect: Rect, fps: Option<u32>) {
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

fn app_title(ui: &mut egui::Ui, texture: egui::TextureId) {
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

fn icon_text(ui: &mut egui::Ui, icon: UiIcon, text: &str, color: Color32, strong: bool) {
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

fn icon_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    text: &str,
    desired_size: Vec2,
    selected: bool,
    emphasized: bool,
) -> egui::Response {
    icon_button_impl(ui, icon, text, desired_size, selected, emphasized, None)
}

fn icon_only_button(
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

fn accent_icon_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    text: &str,
    desired_size: Vec2,
    accent: Color32,
) -> egui::Response {
    icon_button_impl(ui, icon, text, desired_size, false, true, Some(accent))
}

fn icon_button_impl(
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

fn mix_color(from: Color32, to: Color32, amount: f32) -> Color32 {
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

fn disco_ui_color(elapsed: Duration, offset: f32) -> Color32 {
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

fn disco_flash_envelope(elapsed: Duration) -> f32 {
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

fn disco_ball_drop_progress(elapsed: Duration) -> f32 {
    let progress = (elapsed.as_secs_f32() / 1.35).clamp(0.0, 1.0);
    let shifted = progress - 1.0;
    1.0 + 2.701_58 * shifted.powi(3) + 1.701_58 * shifted.powi(2)
}

fn disco_ball_center(rect: Rect, activation_elapsed: Duration) -> Pos2 {
    let progress = disco_ball_drop_progress(activation_elapsed);
    Pos2::new(
        rect.center().x,
        rect.top() - 72.0 + (rect.top() + 158.0 - (rect.top() - 72.0)) * progress,
    )
}

fn paint_disco_ball(
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

fn paint_disco_interface(
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

fn preview_texture_size(
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

fn frame_image(frame: &Frame, target: [usize; 2]) -> egui::ColorImage {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_settings_switch_renders_both_states_without_state_labels() {
        fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, texts);
                    }
                }
                _ => {}
            }
        }

        for (initial_value, expected_fill) in [
            (false, settings_switch_colors().0),
            (true, settings_switch_colors().1),
        ] {
            let context = egui::Context::default();
            let mut value = initial_value;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 100.0))),
                    ..egui::RawInput::default()
                },
                |ui| {
                    settings_toggle_row_without_separator(
                        ui,
                        &mut value,
                        "Test switch",
                        "Static switch description.",
                    );
                },
            );
            let mut texts = Vec::new();
            for clipped in &output.shapes {
                collect_text(&clipped.shape, &mut texts);
            }

            assert!(texts.iter().any(|text| text == "Test switch"));
            assert!(
                texts
                    .iter()
                    .any(|text| text == "Static switch description.")
            );
            assert!(!texts.iter().any(|text| text == "On" || text == "Off"));
            assert!(output.shapes.iter().any(|clipped| {
                matches!(
                    &clipped.shape,
                    egui::Shape::Rect(rect) if rect.fill == expected_fill
                )
            }));
        }
    }

    #[test]
    fn smoke_default_ui_fonts_are_bundled() {
        assert!(!egui::FontDefinitions::default().font_data.is_empty());
    }

    #[test]
    fn contract_user_data_directory_is_stageswap() {
        assert_eq!(
            local_data_directory()
                .file_name()
                .and_then(|name| name.to_str()),
            Some("StageSwap")
        );
    }

    #[test]
    fn contract_window_title_includes_the_package_version() {
        assert_eq!(
            WINDOW_TITLE,
            format!("StageSwap - v{}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(APP_VERSION_LABEL, format!("v{}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn contract_minimized_start_is_reasserted_after_eframe_initialization() {
        assert_eq!(initial_visibility_override(true, true), None);
        assert_eq!(initial_visibility_override(false, true), Some(false));
        assert_eq!(initial_visibility_override(false, false), Some(true));
    }

    #[test]
    fn contract_visible_ui_and_hidden_logic_use_distinct_repaint_cadences() {
        assert_eq!(repaint_interval(true), VISIBLE_REFRESH);
        assert_eq!(repaint_interval(false), HIDDEN_REFRESH);
        assert_eq!(VISIBLE_REFRESH, Duration::from_nanos(1_000_000_000 / 30));
        assert_eq!(HIDDEN_REFRESH, Duration::from_millis(250));
    }

    #[cfg(not(windows))]
    #[test]
    fn contract_hiding_window_suspends_preview_work_and_releases_source_frames() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        let now = Instant::now();
        let frame = Arc::new(Frame::placeholder(
            stageswap_core::Size::new(1280, 720),
            0xff00_0001,
            1,
            0,
            now,
        ));
        let pixels = frame.pixels_arc();
        let converter = PreviewConverter::new(PreviewKind::Webcam.key());
        converter.submit(
            Arc::clone(&frame),
            [240, 135],
            EMBEDDED_PREVIEW_REFRESH,
            now,
        );
        app.preview_converters
            .insert(PreviewKind::Webcam, converter);
        app.enlarged_dashboard_preview = Some(PreviewKind::Webcam);
        let output_sequence = app
            .runtime
            .snapshot()
            .previews
            .final_output
            .expect("the stopped runtime publishes an output frame")
            .sequence;

        app.hide_window(&egui::Context::default());
        drop(frame);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(&pixels) > 1 && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(!app.window_visible);
        assert_eq!(app.enlarged_dashboard_preview, None);
        assert_eq!(Arc::strong_count(&pixels), 1);
        let runtime_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = app.runtime.snapshot();
            if snapshot
                .previews
                .final_output
                .as_ref()
                .is_some_and(|output| output.sequence > output_sequence)
            {
                break;
            }
            assert!(
                Instant::now() < runtime_deadline,
                "runtime output stopped while the window was hidden"
            );
            thread::yield_now();
        }
        assert!(
            app.preview_converters
                .get(&PreviewKind::Webcam)
                .unwrap()
                .take_ready()
                .is_none()
        );
    }

    #[test]
    fn contract_preview_conversion_preserves_size_and_bgra_channels() {
        let frame = Frame::new(
            vec![3, 2, 1, 255, 30, 20, 10, 255].into(),
            stageswap_core::Size::new(2, 1),
            8,
            1,
            0,
            Instant::now(),
        )
        .unwrap();
        let image = frame_image(&frame, [2, 1]);
        assert_eq!(image.size, [2, 1]);
        assert_eq!(image.as_raw(), &[1, 2, 3, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn contract_preview_texture_is_capped_to_its_display_size() {
        let frame = Frame::placeholder(
            stageswap_core::Size::new(1280, 720),
            0xff03_0201,
            1,
            0,
            Instant::now(),
        );
        let size = preview_texture_size(
            &frame,
            egui::vec2(320.0, 180.0),
            1.0,
            DASHBOARD_PREVIEW_TEXTURE_LIMIT,
        );
        assert_eq!(size, [240, 135]);
        let image = frame_image(&frame, size);
        assert_eq!(image.size, size);
        assert_eq!(image.pixels[0], Color32::from_rgb(3, 2, 1));

        let high_dpi = preview_texture_size(
            &frame,
            egui::vec2(640.0, 360.0),
            2.0,
            DASHBOARD_PREVIEW_TEXTURE_LIMIT,
        );
        assert_eq!(high_dpi, [240, 135]);

        let enlarged = preview_texture_size(
            &frame,
            egui::vec2(1280.0, 720.0),
            1.0,
            ENLARGED_PREVIEW_TEXTURE_LIMIT,
        );
        assert_eq!(enlarged, [1280, 720]);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn flow_four_thirty_fps_embedded_previews_fit_inside_one_cpu_second() {
        let frame = Frame::placeholder(
            stageswap_core::Size::new(1280, 720),
            0xff03_0201,
            1,
            0,
            Instant::now(),
        );
        let started = Instant::now();
        for _ in 0..120 {
            std::hint::black_box(frame_image(&frame, [240, 135]));
        }
        assert!(
            started.elapsed() <= Duration::from_secs(1),
            "120 embedded preview conversions exceeded one CPU second"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_each_dashboard_preview_can_expand_even_without_a_frame() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        let context = egui::Context::default();
        let screen_rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 280.0));

        for kind in [
            PreviewKind::Webcam,
            PreviewKind::Screen,
            PreviewKind::Reference,
            PreviewKind::Output,
        ] {
            app.enlarged_dashboard_preview = None;
            let mut preview_rect = Rect::NOTHING;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen_rect),
                    ..egui::RawInput::default()
                },
                |ui| {
                    preview_rect =
                        app.preview_cell(ui, kind, None, [320.0, 180.0], Source::Camera, Some(30));
                },
            );
            let position = preview_rect.center();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen_rect),
                    events: vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..egui::RawInput::default()
                },
                |ui| {
                    app.preview_cell(ui, kind, None, [320.0, 180.0], Source::Camera, Some(30));
                },
            );

            assert_eq!(app.enlarged_dashboard_preview, Some(kind));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_enlarged_dashboard_preview_closes_on_click_escape_and_navigation() {
        fn dashboard_input(event: egui::Event) -> egui::RawInput {
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 720.0))),
                events: vec![event],
                ..egui::RawInput::default()
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Settings(SettingsTab::General),
        });
        app.view = AppView::Dashboard;
        let context = egui::Context::default();

        app.enlarged_dashboard_preview = Some(PreviewKind::Webcam);
        let position = Pos2::new(20.0, 20.0);
        let _ = context.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 720.0))),
                ..egui::RawInput::default()
            },
            |ui| app.dashboard(ui),
        );
        assert_eq!(app.enlarged_dashboard_preview, None);

        app.enlarged_dashboard_preview = Some(PreviewKind::Screen);
        let _ = context.run_ui(
            dashboard_input(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }),
            |ui| app.dashboard(ui),
        );
        assert_eq!(app.enlarged_dashboard_preview, None);

        app.enlarged_dashboard_preview = Some(PreviewKind::Output);
        app.open_settings();
        assert_eq!(app.view, AppView::Settings);
        assert_eq!(app.enlarged_dashboard_preview, None);
    }

    #[test]
    fn contract_preview_converter_collapses_pending_jobs_to_latest_frame() {
        let converter = PreviewConverter::new(PreviewKind::Output.key());
        let now = Instant::now();
        for sequence in 1..=12 {
            converter.submit(
                Arc::new(Frame::placeholder(
                    stageswap_core::Size::new(1280, 720),
                    0xff00_0000 | sequence,
                    u64::from(sequence),
                    0,
                    now,
                )),
                [240, 135],
                Duration::ZERO,
                now,
            );
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(prepared) = converter.take_ready()
                && prepared.id.sequence == 12
            {
                assert_eq!(prepared.size, [240, 135]);
                assert_eq!(prepared.image.pixels[0], Color32::from_rgb(0, 0, 12));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "latest preview frame was not prepared"
            );
            thread::yield_now();
        }
    }

    #[test]
    fn contract_preview_contours_mark_live_output_and_active_source() {
        assert_eq!(
            preview_contour(PreviewKind::Output, Source::Camera, OutputLayout::Camera),
            PreviewContour::Live
        );
        assert_eq!(
            preview_contour(PreviewKind::Webcam, Source::Camera, OutputLayout::Camera),
            PreviewContour::Active
        );
        assert_eq!(
            preview_contour(PreviewKind::Screen, Source::Camera, OutputLayout::Camera),
            PreviewContour::Neutral
        );
        assert_eq!(
            preview_contour(PreviewKind::Screen, Source::Screen, OutputLayout::Screen),
            PreviewContour::Active
        );
        assert_eq!(
            preview_contour(
                PreviewKind::Webcam,
                Source::Placeholder,
                OutputLayout::Placeholder,
            ),
            PreviewContour::Neutral
        );
        assert_eq!(
            preview_contour(PreviewKind::Reference, Source::Screen, OutputLayout::Screen,),
            PreviewContour::Neutral
        );
        for kind in [PreviewKind::Webcam, PreviewKind::Screen] {
            assert_eq!(
                preview_contour(kind, Source::Camera, OutputLayout::WebcamMainScreenPip,),
                PreviewContour::Active
            );
        }
    }

    #[test]
    fn contract_health_states_have_lifecycle_order_icons_and_semantic_colors() {
        assert_eq!(
            HEALTH_STATES,
            [
                DeviceState::Initializing,
                DeviceState::Ready,
                DeviceState::Unavailable,
                DeviceState::Failed,
            ]
        );
        assert_eq!(device_state_icon(DeviceState::Initializing), UiIcon::Loader);
        assert_eq!(device_state_icon(DeviceState::Ready), UiIcon::Check);
        assert_eq!(
            device_state_icon(DeviceState::Unavailable),
            UiIcon::Unavailable
        );
        assert_eq!(device_state_icon(DeviceState::Failed), UiIcon::Error);
        assert_eq!(
            indicator_palette(device_state_tone(DeviceState::Ready), 1.0).1,
            ACTIVE_GREEN
        );
        assert_eq!(
            indicator_palette(device_state_tone(DeviceState::Initializing), 1.0).1,
            TRANSITION_AMBER
        );
        assert_eq!(
            indicator_palette(device_state_tone(DeviceState::Failed), 1.0).1,
            LIVE_RED
        );
        assert_eq!(
            indicator_palette(device_state_tone(DeviceState::Unavailable), 1.0).1,
            LIVE_RED
        );
        let inactive = indicator_palette(device_state_tone(DeviceState::Ready), 0.0);
        assert_eq!(inactive.1, Color32::from_rgb(91, 97, 108));
        assert_eq!(
            inactive,
            indicator_palette(device_state_tone(DeviceState::Failed), 0.0)
        );
    }

    #[test]
    fn flow_not_matching_is_warning_amber_but_missing_reference_is_error_red() {
        assert_eq!(
            detection_indicator_tone(DetectionState::NotMatching),
            IndicatorTone::Amber
        );
        assert_eq!(
            settings_detection_style(DetectionState::NotMatching).2,
            TRANSITION_AMBER
        );
        assert_eq!(
            detection_indicator_tone(DetectionState::ReferenceMissing),
            IndicatorTone::Red
        );
        assert_eq!(
            settings_detection_style(DetectionState::ReferenceMissing).2,
            LIVE_RED
        );
    }

    #[test]
    fn contract_fps_overlays_use_runtime_metrics_for_all_live_pipelines() {
        assert!(PreviewKind::Webcam.shows_fps());
        assert!(PreviewKind::Screen.shows_fps());
        assert!(PreviewKind::Output.shows_fps());
        assert!(!PreviewKind::Reference.shows_fps());
        assert!(!PreviewOptions::settings("missing").show_fps);
        assert!(PreviewOptions::dashboard(PreviewKind::Webcam, Some(30)).show_fps);
        let output = PreviewOptions::dashboard(PreviewKind::Output, Some(30));
        assert!(output.show_fps);
        assert_eq!(output.fps, Some(30));
        assert_eq!(output.style, PreviewStyle::Card);
        assert_eq!(output.texture_limit, [240, 135]);
        assert_eq!(output.refresh_interval, EMBEDDED_PREVIEW_REFRESH);
        let enlarged = PreviewOptions::enlarged(PreviewKind::Output, Some(30));
        assert_eq!(enlarged.texture_limit, [1280, 720]);
        assert_eq!(enlarged.refresh_interval, ENLARGED_PREVIEW_REFRESH);
        assert_eq!(enlarged.style, PreviewStyle::CinemaPeek);
        assert_eq!(CINEMA_PEEK_SCALE, 0.92);
    }

    #[cfg(not(windows))]
    #[test]
    fn smoke_enlarged_dashboard_preview_keeps_metadata_without_contour() {
        fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, texts);
                    }
                }
                _ => {}
            }
        }

        fn has_stroke(shape: &egui::Shape) -> bool {
            match shape {
                egui::Shape::Rect(rect) => rect.stroke.width > 0.0,
                egui::Shape::Vec(shapes) => shapes.iter().any(has_stroke),
                _ => false,
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Settings(SettingsTab::General),
        });
        app.view = AppView::Dashboard;
        app.enlarged_dashboard_preview = Some(PreviewKind::Output);
        let context = egui::Context::default();
        ui_icon::install_fonts(&context);
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 720.0))),
                ..egui::RawInput::default()
            },
            |ui| app.dashboard(ui),
        );
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut texts);
        }
        assert!(texts.iter().any(|text| text == "ZOOM OUTPUT"));
        assert!(texts.iter().any(|text| text == "LIVE"));
        assert!(texts.iter().any(|text| text == "30 FPS"));
        assert!(
            !output
                .shapes
                .iter()
                .any(|clipped| has_stroke(&clipped.shape))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn smoke_ui_preview_cli_selects_each_settings_page() {
        for tab in SettingsTab::ALL {
            let args = vec![
                "StageSwap".to_owned(),
                "--ui-preview".to_owned(),
                tab.preview_name().to_owned(),
            ];
            assert_eq!(
                parse_ui_preview_request(&args).unwrap(),
                Some(UiPreviewRequest {
                    target: UiPreviewTarget::Settings(tab),
                })
            );
        }
        for step in SetupStep::ALL {
            let args = vec![
                "StageSwap".to_owned(),
                "--ui-preview".to_owned(),
                format!("setup-{}", step.number()),
            ];
            assert_eq!(
                parse_ui_preview_request(&args).unwrap(),
                Some(UiPreviewRequest {
                    target: UiPreviewTarget::Setup(step),
                })
            );
        }
        let default_page = vec!["StageSwap".to_owned(), "--ui-preview".to_owned()];
        assert_eq!(
            parse_ui_preview_request(&default_page).unwrap(),
            Some(UiPreviewRequest {
                target: UiPreviewTarget::Settings(SettingsTab::General),
            })
        );
        let localized = vec![
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            "general".to_owned(),
            "--ui-language".to_owned(),
            "fr-CA".to_owned(),
        ];
        assert_eq!(ui_preview_locale(&localized).unwrap(), Locale::French);
        let unsupported = vec![
            "StageSwap".to_owned(),
            "--ui-preview".to_owned(),
            "--ui-language".to_owned(),
            "de-DE".to_owned(),
        ];
        assert!(parse_ui_preview_request(&unsupported).is_err());
        for (name, kind) in [
            ("dialog-exit", AppDialogKind::Exit),
            ("dialog-clear-logs", AppDialogKind::ClearLogs),
            ("dialog-reference-capture", AppDialogKind::ReferenceCapture),
            ("dialog-admin", AppDialogKind::Admin),
            (
                "dialog-replace-baseline",
                AppDialogKind::ReplaceAdminBaseline,
            ),
            ("dialog-load-admin-config", AppDialogKind::LoadAdminConfig),
            ("dialog-remove-baseline", AppDialogKind::RemoveAdminBaseline),
        ] {
            let args = vec![
                "StageSwap".to_owned(),
                "--ui-preview".to_owned(),
                name.to_owned(),
            ];
            assert_eq!(
                parse_ui_preview_request(&args).unwrap(),
                Some(UiPreviewRequest {
                    target: UiPreviewTarget::Dialog(kind),
                })
            );
        }
        for (name, state) in [
            ("notifications", NotificationPreviewState::Stacked),
            ("notifications-empty", NotificationPreviewState::Empty),
            ("notifications-critical", NotificationPreviewState::Critical),
            ("notifications-updates", NotificationPreviewState::Updates),
        ] {
            let args = vec![
                "StageSwap".to_owned(),
                "--ui-preview".to_owned(),
                name.to_owned(),
            ];
            assert_eq!(
                parse_ui_preview_request(&args).unwrap(),
                Some(UiPreviewRequest {
                    target: UiPreviewTarget::Notifications(state),
                })
            );
        }
        assert!(
            parse_ui_preview_request(&["StageSwap".to_owned()])
                .unwrap()
                .is_none()
        );
        assert!(
            parse_ui_preview_request(&[
                "StageSwap".to_owned(),
                "--ui-preview".to_owned(),
                "unknown".to_owned(),
            ])
            .is_err()
        );
        assert!(
            parse_ui_preview_request(&[
                "StageSwap".to_owned(),
                "--ui-preview".to_owned(),
                "setup-6".to_owned(),
            ])
            .is_err()
        );
        for (value, expected) in [
            ("captured", SetupReferencePreviewState::Captured),
            ("empty", SetupReferencePreviewState::Empty),
            ("review", SetupReferencePreviewState::Review),
            ("missing-screen", SetupReferencePreviewState::MissingScreen),
        ] {
            assert_eq!(
                parse_setup_reference_preview_state(&[
                    "StageSwap".to_owned(),
                    "--ui-setup-reference-state".to_owned(),
                    value.to_owned(),
                ])
                .unwrap(),
                Some(expected)
            );
        }
        assert!(
            parse_setup_reference_preview_state(&[
                "StageSwap".to_owned(),
                "--ui-setup-reference-state".to_owned(),
                "unknown".to_owned(),
            ])
            .is_err()
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn smoke_ui_screenshot_cli_requires_an_absolute_png_path() {
        let path = "/tmp/stageswap-setup.png";
        assert_eq!(
            parse_ui_screenshot_path(&[
                "StageSwap".to_owned(),
                "--ui-screenshot".to_owned(),
                path.to_owned(),
            ])
            .unwrap(),
            Some(PathBuf::from(path))
        );
        assert!(
            parse_ui_screenshot_path(&[
                "StageSwap".to_owned(),
                "--ui-screenshot".to_owned(),
                "setup.png".to_owned(),
            ])
            .is_err()
        );
        assert!(
            parse_ui_screenshot_path(&[
                "StageSwap".to_owned(),
                "--ui-screenshot".to_owned(),
                "/tmp/setup.jpg".to_owned(),
            ])
            .is_err()
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_ui_preview_seeds_alerts_and_repeats_activity_notifications() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
        });

        assert_eq!(app.notifications.entries().count(), 3);
        assert_eq!(app.notifications.toasts().count(), 2);
        assert_eq!(
            app.notifications
                .entries()
                .filter(|entry| entry.tone == NotificationTone::Critical)
                .count(),
            2
        );
        assert!(
            app.notifications
                .entries()
                .any(|entry| entry.source == NotificationSource::Updates)
        );

        let first_due = app
            .ui_preview
            .as_ref()
            .and_then(|preview| preview.next_notification_at)
            .unwrap();
        let context = egui::Context::default();
        app.tick_ui_preview_notifications(&context, first_due);

        assert_eq!(app.notifications.entries().count(), 4);
        assert_eq!(
            app.notifications
                .entries()
                .filter(|entry| entry.source == NotificationSource::Preview)
                .count(),
            1
        );
        assert!(
            app.notifications
                .entries()
                .any(|entry| entry.tone == NotificationTone::Information)
        );
        assert_eq!(app.notifications.toasts().count(), 2);

        let second_due = app
            .ui_preview
            .as_ref()
            .and_then(|preview| preview.next_notification_at)
            .unwrap();
        assert_eq!(second_due, first_due + UI_PREVIEW_NOTIFICATION_INTERVAL);
        app.tick_ui_preview_notifications(&context, second_due);
        assert_eq!(
            app.notifications
                .entries()
                .filter(|entry| entry.source == NotificationSource::Preview)
                .count(),
            2
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_ui_preview_fixed_notification_states_do_not_start_periodic_activity() {
        for state in [
            NotificationPreviewState::Empty,
            NotificationPreviewState::Critical,
            NotificationPreviewState::Updates,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let app = SwitcherApp::new(
                ui_preview_config(),
                Vec::new(),
                ConfigStore::new(directory.path()),
            )
            .with_ui_preview(UiPreviewRequest {
                target: UiPreviewTarget::Notifications(state),
            });

            assert!(
                app.ui_preview
                    .as_ref()
                    .unwrap()
                    .next_notification_at
                    .is_none()
            );
            assert!(
                !app.notifications
                    .entries()
                    .any(|entry| entry.source == NotificationSource::Preview)
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_notification_popover_closes_on_outside_click() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
        });
        let context = egui::Context::default();
        ui_icon::install_fonts(&context);
        let viewport = egui::vec2(1280.0, 720.0);

        let render = |app: &mut SwitcherApp, input: egui::RawInput| {
            let _ = context.run_ui(input, |ui| {
                app.root_ui(ui);
            });
        };

        app.notification_center_open = true;
        render(
            &mut app,
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
                ..egui::RawInput::default()
            },
        );
        assert!(app.notification_center_open);

        render(
            &mut app,
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
                events: vec![
                    egui::Event::PointerMoved(Pos2::new(900.0, 100.0)),
                    egui::Event::PointerButton {
                        pos: Pos2::new(900.0, 100.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: Pos2::new(900.0, 100.0),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..egui::RawInput::default()
            },
        );
        assert!(!app.notification_center_open);
    }

    #[cfg(not(windows))]
    #[test]
    fn smoke_setup_demo_preview_state_is_deterministic() {
        assert_eq!(
            parse_setup_demo_preview_state(&[
                "StageSwap".to_owned(),
                "--ui-setup-demo-state".to_owned(),
                "matching".to_owned(),
            ])
            .unwrap(),
            Some(SetupDemoPreviewState::Matching)
        );
        assert_eq!(
            parse_setup_demo_preview_state(&[
                "StageSwap".to_owned(),
                "--ui-setup-demo-state".to_owned(),
                "non-matching".to_owned(),
            ])
            .unwrap(),
            Some(SetupDemoPreviewState::Changed)
        );
        assert!(
            parse_setup_demo_preview_state(&[
                "StageSwap".to_owned(),
                "--ui-setup-demo-state".to_owned(),
                "unknown".to_owned(),
            ])
            .is_err()
        );
    }

    #[test]
    fn flow_setup_reference_requires_a_decision_only_for_unresolved_candidates() {
        let now = Instant::now();
        assert!(!setup_reference_requires_decision(
            SetupReferenceCaptureState::Idle
        ));
        assert!(setup_reference_requires_decision(
            SetupReferenceCaptureState::PreparingCandidate { started_at: now }
        ));
        assert!(setup_reference_requires_decision(
            SetupReferenceCaptureState::CapturingCandidate {
                started_at: now,
                previous_candidate_sequence: None,
            }
        ));
        assert!(setup_reference_requires_decision(
            SetupReferenceCaptureState::Review { captured_at: now }
        ));
        assert!(setup_reference_requires_decision(
            SetupReferenceCaptureState::SavingCandidate {
                started_at: now,
                previous_reference_sequence: None,
            }
        ));
        assert!(setup_reference_requires_decision(
            SetupReferenceCaptureState::SaveFailed {
                previous_reference_sequence: None,
            }
        ));
        assert!(!setup_reference_requires_decision(
            SetupReferenceCaptureState::Confirmed
        ));
    }

    #[test]
    fn flow_setup_reference_capture_action_distinguishes_first_capture_from_retry() {
        assert_eq!(
            setup_reference_capture_label(false),
            "Capture reference image"
        );
        assert_eq!(setup_reference_capture_label(true), "Capture again");
    }

    #[cfg(not(windows))]
    #[test]
    fn smoke_ui_preview_frame_decodes_embedded_images_as_bgra() {
        let bytes = include_bytes!("../assets/setup-webcam-example.png");
        let source = image::load_from_memory(bytes).unwrap().to_rgba8();
        let frame = ui_preview_frame(7, "setup-webcam-example", bytes);

        assert_eq!(
            frame.size,
            stageswap_core::Size::new(source.width(), source.height())
        );
        assert_eq!(frame.stride, source.width() * 4);
        assert_eq!(frame.sequence, 7);
        let source_pixel = &source.as_raw()[..4];
        assert_eq!(
            &frame.pixels()[..4],
            &[
                source_pixel[2],
                source_pixel[1],
                source_pixel[0],
                source_pixel[3]
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn smoke_ui_preview_snapshot_has_realistic_ready_inputs_and_frames() {
        let snapshot = ui_preview_snapshot();
        assert_eq!(snapshot.run_state, RunState::Running);
        assert_eq!(snapshot.mode, OutputMode::Automatic);
        assert_eq!(snapshot.detection, DetectionState::Matching);
        assert_eq!(snapshot.webcam_state, DeviceState::Ready);
        assert_eq!(snapshot.screen_state, DeviceState::Ready);
        assert_eq!(snapshot.virtual_camera_state, DeviceState::Ready);
        assert!(snapshot.previews.webcam.is_some());
        assert!(snapshot.previews.screen.is_some());
        assert!(snapshot.previews.reference.is_some());
        assert!(snapshot.previews.final_output.is_some());
        assert_eq!(snapshot.video_devices.len(), 2);
        assert_eq!(snapshot.monitors.len(), 2);
        let webcam = snapshot.previews.webcam.as_ref().unwrap();
        let screen = snapshot.previews.screen.as_ref().unwrap();
        let reference = snapshot.previews.reference.as_ref().unwrap();
        let output = snapshot.previews.final_output.as_ref().unwrap();
        assert_eq!(webcam.size, stageswap_core::Size::new(1672, 941));
        assert_eq!(screen.size, stageswap_core::Size::new(1672, 941));
        assert!(Arc::ptr_eq(webcam, output));
        assert!(Arc::ptr_eq(screen, reference));
    }

    #[cfg(not(windows))]
    #[test]
    fn smoke_primary_ui_surfaces_render_contained_without_panic() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Settings(SettingsTab::General),
        });
        let context = egui::Context::default();
        ui_icon::install_fonts(&context);
        let viewport = egui::vec2(MIN_WINDOW_HEIGHT * WINDOW_ASPECT_RATIO, MIN_WINDOW_HEIGHT);

        let render_root = |app: &mut SwitcherApp, viewport: Vec2| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
                ..egui::RawInput::default()
            };
            let output = context.run_ui(input, |ui| {
                let content = app.root_ui(ui);
                assert!(ui.max_rect().contains_rect(content));
            });
            assert!(!output.shapes.is_empty());
        };

        app.active_dialog = None;
        app.view = AppView::Dashboard;
        render_root(&mut app, viewport);

        for state in [
            NotificationPreviewState::Empty,
            NotificationPreviewState::Critical,
            NotificationPreviewState::Updates,
            NotificationPreviewState::Stacked,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut notification_app = SwitcherApp::new(
                ui_preview_config(),
                Vec::new(),
                ConfigStore::new(directory.path()),
            )
            .with_ui_preview(UiPreviewRequest {
                target: UiPreviewTarget::Notifications(state),
            });
            notification_app.active_dialog = None;
            render_root(&mut notification_app, viewport);
        }

        for locale in Locale::ALL {
            let directory = tempfile::tempdir().unwrap();
            let mut config = ui_preview_config();
            config.interface_language = locale.tag().into();
            let mut notification_app =
                SwitcherApp::new(config, Vec::new(), ConfigStore::new(directory.path()))
                    .with_ui_preview(UiPreviewRequest {
                        target: UiPreviewTarget::Notifications(NotificationPreviewState::Stacked),
                    });
            notification_app.active_dialog = None;
            render_root(&mut notification_app, egui::vec2(1066.0, 600.0));
        }

        for tab in SettingsTab::ALL {
            app.active_dialog = None;
            app.view = AppView::Settings;
            app.settings_tab = tab;
            app.settings_opened_at = None;
            app.settings_section_changed_at = None;
            render_root(&mut app, viewport);
        }

        for step in SetupStep::ALL {
            app.active_dialog = None;
            app.view = AppView::SetupGuide;
            app.setup_session = Some(SetupSession::preview(step));
            app.setup_reference_capture = SetupReferenceCaptureState::Idle;
            render_root(&mut app, viewport);
        }

        app.view = AppView::Settings;
        app.admin_profile_status = Some(AdminProfileStatus {
            auto_restore_on_launch: true,
            reference_included: true,
        });
        for kind in [
            AppDialogKind::Exit,
            AppDialogKind::ClearLogs,
            AppDialogKind::ReferenceCapture,
            AppDialogKind::Admin,
        ] {
            app.active_dialog = Some(ActiveDialog {
                kind,
                opened_at: Instant::now() - DIALOG_ENTRANCE_DURATION,
                focus_safe_action: true,
            });
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
                ..egui::RawInput::default()
            };
            let output = context.run_ui(input, |_ui| app.dialog(&context));
            assert!(!output.shapes.is_empty());
            assert!(app.dialog_is(kind));
        }

        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, viewport)),
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..egui::RawInput::default()
        };
        let _ = context.run_ui(input, |_ui| app.dialog(&context));
        assert!(app.active_dialog.is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_ui_preview_reference_commands_exercise_capture_retake_and_confirmation() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Setup(SetupStep::Reference),
        });
        let original = app.snapshot().previews.reference.clone().unwrap();

        app.setup_reference_capture = SetupReferenceCaptureState::CapturingCandidate {
            started_at: Instant::now(),
            previous_candidate_sequence: None,
        };
        app.send_setup_reference_command(Command::CaptureReferenceCandidate);
        let captured = app.snapshot();
        let first_candidate = captured.previews.reference_candidate.as_ref().unwrap();
        assert!(Arc::ptr_eq(
            captured.previews.reference.as_ref().unwrap(),
            &original
        ));
        assert_eq!(
            first_candidate.pixels(),
            captured.previews.screen.as_ref().unwrap().pixels()
        );
        assert_ne!(first_candidate.sequence, original.sequence);
        app.update_setup_reference_capture(&captured);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
        ));

        app.send_setup_reference_command(Command::DiscardReferenceCandidate);
        let discarded = app.snapshot();
        assert!(discarded.previews.reference_candidate.is_none());
        assert!(Arc::ptr_eq(
            discarded.previews.reference.as_ref().unwrap(),
            &original
        ));

        app.send_setup_reference_command(Command::CaptureReferenceCandidate);
        let candidate = app.snapshot().previews.reference_candidate.clone().unwrap();
        app.setup_reference_capture = SetupReferenceCaptureState::SavingCandidate {
            started_at: Instant::now(),
            previous_reference_sequence: Some(original.sequence),
        };
        app.send_setup_reference_command(Command::ConfirmReferenceCandidate);
        let confirmed = app.snapshot();
        assert!(confirmed.previews.reference_candidate.is_none());
        assert!(Arc::ptr_eq(
            confirmed.previews.reference.as_ref().unwrap(),
            &candidate
        ));
        app.update_setup_reference_capture(&confirmed);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Confirmed
        ));
    }

    #[test]
    fn flow_version_picker_keeps_latest_offer_and_requires_an_explicit_installable_selection() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            AppConfig {
                update_channel: UpdateChannel::Beta,
                ..AppConfig::default()
            },
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        let releases = update::preview_versions();
        let latest = releases[0].clone();
        let older = releases[1].clone();
        app.update_status = UpdateStatus::Available(latest.clone());
        app.update_versions = update::VersionCatalog::Ready(releases);
        assert!(app.selected_version_release().is_none());
        app.selected_update_version = Some(older.version);
        assert_eq!(app.selected_version_release(), Some(older.clone()));
        assert_eq!(app.update_status, UpdateStatus::Available(latest.clone()));
        app.update_version_picker_open = true;
        let context = egui::Context::default();
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            app.update_version_picker(ui.ctx())
        });
        let _ = context.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                }],
                ..egui::RawInput::default()
            },
            |ui| app.update_version_picker(ui.ctx()),
        );
        assert!(!app.update_version_picker_open);
        assert!(app.selected_update_version.is_none());
        assert_eq!(app.update_status, UpdateStatus::Available(latest));

        let mut current = older.clone();
        current.version = update::ReleaseVersion::parse(env!("CARGO_PKG_VERSION")).unwrap();
        app.selected_update_version = Some(current.version);
        app.update_versions = update::VersionCatalog::Ready(vec![current]);
        assert!(app.selected_version_release().is_none());
        app.selected_update_version = Some(older.version);
        app.update_versions = update::VersionCatalog::Failed("offline".into());
        assert!(app.selected_version_release().is_none());
        app.update_versions = update::VersionCatalog::Loading;
        assert!(app.update_busy());
        assert!(app.selected_version_release().is_none());
        app.update_versions = update::VersionCatalog::Ready(vec![older]);
        app.config.update_channel = UpdateChannel::Stable;
        assert!(app.selected_version_release().is_none());
    }

    #[test]
    fn flow_busy_command_feedback_preserves_reference_dialog_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            AppConfig::default(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        app.setup_reference_capture = SetupReferenceCaptureState::Review {
            captured_at: Instant::now(),
        };

        assert!(!app.handle_command_dispatch(CommandDispatch::Busy));

        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
        ));
        assert!(
            app.notifications
                .entries()
                .any(|entry| entry.source == NotificationSource::Command)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_shared_reference_modal_preserves_cancels_and_confirms_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Setup(SetupStep::Reference),
        });
        let original = app.snapshot().previews.reference.clone().unwrap();

        app.begin_reference_capture();
        assert!(app.dialog_is(AppDialogKind::ReferenceCapture));
        let preparing = app.snapshot();
        app.update_setup_reference_capture(&preparing);
        let captured = app.snapshot();
        assert!(Arc::ptr_eq(
            captured.previews.reference.as_ref().unwrap(),
            &original
        ));
        assert!(captured.previews.reference_candidate.is_some());
        let first_candidate_sequence = captured
            .previews
            .reference_candidate
            .as_ref()
            .unwrap()
            .sequence;
        app.update_setup_reference_capture(&captured);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
        ));

        app.prepare_reference_capture();
        let cleared = app.snapshot();
        assert!(cleared.previews.reference_candidate.is_none());
        app.update_setup_reference_capture(&cleared);
        let retaken = app.snapshot();
        assert_eq!(
            retaken
                .previews
                .reference_candidate
                .as_ref()
                .unwrap()
                .sequence,
            first_candidate_sequence
        );
        app.update_setup_reference_capture(&retaken);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
        ));

        app.cancel_reference_capture();
        let cancelled = app.snapshot();
        assert!(app.active_dialog.is_none());
        assert!(cancelled.previews.reference_candidate.is_none());
        assert!(Arc::ptr_eq(
            cancelled.previews.reference.as_ref().unwrap(),
            &original
        ));

        app.begin_reference_capture();
        let preparing = app.snapshot();
        app.update_setup_reference_capture(&preparing);
        let captured = app.snapshot();
        let candidate = captured
            .previews
            .reference_candidate
            .as_ref()
            .unwrap()
            .clone();
        app.update_setup_reference_capture(&captured);
        app.confirm_reference_candidate();
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::SavingCandidate { .. }
        ));
        app.cancel_reference_capture();
        assert!(app.dialog_is(AppDialogKind::ReferenceCapture));

        let confirmed = app.snapshot();
        assert!(confirmed.previews.reference_candidate.is_none());
        assert!(Arc::ptr_eq(
            confirmed.previews.reference.as_ref().unwrap(),
            &candidate
        ));
        app.setup_animations_enabled = false;
        let context = egui::Context::default();
        let _ = context.run_ui(egui::RawInput::default(), |_ui| app.dialog(&context));
        assert!(app.active_dialog.is_none());
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Confirmed
        ));
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::Ready)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_reference_confirmation_does_not_advance_from_settings() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            ui_preview_config(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        )
        .with_ui_preview(UiPreviewRequest {
            target: UiPreviewTarget::Settings(SettingsTab::Matching),
        });
        app.setup_reference_capture = SetupReferenceCaptureState::Confirmed;
        app.open_dialog(AppDialogKind::ReferenceCapture);

        let context = egui::Context::default();
        let _ = context.run_ui(egui::RawInput::default(), |_ui| app.dialog(&context));

        assert!(app.active_dialog.is_none());
        assert!(app.setup_session.is_none());
        assert!(matches!(app.view, AppView::Settings));
        assert_eq!(app.settings_tab, SettingsTab::Matching);
    }

    #[test]
    fn flow_clearing_logs_requires_explicit_confirmation() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            AppConfig::default(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        app.log.write("info", "test", "KEEP", "keep this entry");
        let before = directory.path().join("before.jsonl");

        app.confirm_log_clear();
        app.log.export_to(&before).unwrap();
        assert!(std::fs::read_to_string(&before).unwrap().contains("KEEP"));

        app.request_log_clear();
        assert!(app.dialog_is(AppDialogKind::ClearLogs));
        app.dismiss_dialog();
        assert!(app.active_dialog.is_none());
        let after_cancel = directory.path().join("after-cancel.jsonl");
        app.log.export_to(&after_cancel).unwrap();
        assert!(
            std::fs::read_to_string(&after_cancel)
                .unwrap()
                .contains("KEEP")
        );

        app.request_log_clear();
        app.confirm_log_clear();
        assert!(app.active_dialog.is_none());
        let after_clear = directory.path().join("after-clear.jsonl");
        app.log.export_to(&after_clear).unwrap();
        let contents = std::fs::read_to_string(&after_clear).unwrap();
        assert!(!contents.contains("KEEP"));
        assert!(contents.contains("LOGS_CLEARED"));
    }

    #[test]
    fn flow_settings_save_is_debounced_and_back_flushes_pending_changes() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
        let started_at = Instant::now();
        app.config.cursor_visible = true;
        app.config.selected_video_device_id = "new-camera".into();
        app.config.still_image_pip_enabled = true;
        app.config.still_image_pip_delay_seconds = 120;
        app.config.still_image_pip_layout = StillImagePipLayout::ScreenMain;
        app.config.still_image_pip_size = StillImagePipSize::Large;
        app.pending_settings_save = Some(started_at);

        assert!(!app.settings_save_due(started_at + SETTINGS_SAVE_DEBOUNCE / 2));
        assert!(app.settings_save_due(started_at + SETTINGS_SAVE_DEBOUNCE));

        app.view = AppView::Settings;
        app.close_settings();
        app.wait_for_settings_save();
        assert_eq!(app.view, AppView::Dashboard);
        assert!(app.pending_settings_save.is_none());
        let saved = store.load().config;
        assert!(saved.cursor_visible);
        assert!(saved.still_image_pip_enabled);
        assert_eq!(saved.still_image_pip_delay_seconds, 120);
        assert_eq!(
            saved.still_image_pip_layout,
            StillImagePipLayout::ScreenMain
        );
        assert_eq!(saved.still_image_pip_size, StillImagePipSize::Large);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if app.runtime.snapshot().selected_video_device_id == "new-camera" {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "runtime did not receive the flushed settings"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn flow_accepted_monitor_selection_is_persisted_by_label() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
        let snapshot = AppSnapshot {
            selected_monitor: Some(stageswap_core::MonitorDescriptor {
                display_name: r"\\.\DISPLAY2".into(),
                label: "Stage Display".into(),
                ..stageswap_core::MonitorDescriptor::default()
            }),
            ..AppSnapshot::default()
        };

        app.sync_selected_monitor_preference(&snapshot);
        app.wait_for_settings_save();

        assert_eq!(app.config.selected_monitor_label, "Stage Display");
        assert_eq!(store.load().config.selected_monitor_label, "Stage Display");
    }

    #[test]
    fn flow_opening_settings_does_not_rescan_when_automatic_rescans_are_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            AppConfig {
                automatic_monitor_rescans: false,
                ..AppConfig::default()
            },
            Vec::new(),
            ConfigStore::new(directory.path()),
        );

        app.open_settings();

        let deadline = Instant::now() + Duration::from_secs(2);
        let snapshot = loop {
            let snapshot = app.runtime.snapshot();
            if snapshot
                .recent_activity
                .iter()
                .any(|activity| activity == "Video device list refreshed")
            {
                break snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "runtime did not process the settings refresh"
            );
            std::thread::yield_now();
        };
        assert!(
            snapshot
                .recent_activity
                .iter()
                .all(|activity| activity != "Monitor rescan requested")
        );
    }

    #[test]
    fn flow_settings_save_failure_preserves_the_active_value() {
        let directory = tempfile::tempdir().unwrap();
        let blocked_path = directory.path().join("not-a-directory");
        std::fs::write(&blocked_path, "blocking file").unwrap();
        let mut app = SwitcherApp::new(
            AppConfig::default(),
            Vec::new(),
            ConfigStore::new(&blocked_path),
        );
        let initial_notification_count = app.notifications.entries().count();
        app.config.show_notifications = false;
        app.queue_settings_save();
        app.flush_settings();
        app.wait_for_settings_save();

        assert!(!app.config.show_notifications);
        assert!(
            app.settings_save_error
                .as_deref()
                .is_some_and(|message| message.contains("Could not save settings"))
        );
        assert!(
            app.load_warnings
                .iter()
                .any(|warning| warning.contains("Could not save settings"))
        );
        assert_eq!(
            app.notifications.entries().count(),
            initial_notification_count
        );
    }

    #[test]
    fn flow_verbose_logging_setting_uses_the_normal_settings_save_path() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());

        app.config.verbose_logging = true;
        app.queue_settings_save();
        app.flush_settings();
        app.wait_for_settings_save();

        assert!(app.settings_save_error.is_none());
        assert!(store.load().config.verbose_logging);
    }

    #[test]
    fn flow_admin_baseline_captures_pending_settings_and_toggle_is_independent() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
        assert_eq!(app.admin_profile_status, None);

        app.config.selected_video_device_id = "pending-admin-camera".into();
        app.pending_settings_save = Some(Instant::now());
        app.save_admin_baseline();
        assert_eq!(
            app.admin_profile_status,
            Some(AdminProfileStatus {
                auto_restore_on_launch: false,
                reference_included: false,
            })
        );

        app.set_admin_auto_restore(true);
        assert!(app.admin_profile_status.unwrap().auto_restore_on_launch);
        save_config(
            &store,
            &AppConfig {
                selected_video_device_id: "temporary-user-camera".into(),
                reference_image_path: store.reference_path().display().to_string(),
                ..AppConfig::default()
            },
        )
        .unwrap();
        assert_eq!(
            restore_admin_profile(&AdminProfileStore::new(directory.path())).unwrap(),
            AdminRestoreOutcome::Restored
        );
        assert_eq!(
            store.load().config.selected_video_device_id,
            "pending-admin-camera"
        );

        app.set_admin_auto_restore(false);
        app.config.selected_video_device_id = "replacement-admin-camera".into();
        app.save_admin_baseline();
        assert!(!app.admin_profile_status.unwrap().auto_restore_on_launch);
        app.remove_admin_baseline();
        assert_eq!(app.admin_profile_status, None);
        assert_eq!(
            AdminProfileStore::new(directory.path()).status().unwrap(),
            None
        );
    }

    #[test]
    fn flow_manual_admin_load_updates_working_config_runtime_and_reference() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(store.reference_path())
            .unwrap();
        let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store.clone());
        app.view = AppView::Settings;
        let admin = AppConfig {
            selected_video_device_id: "admin-camera".into(),
            selected_monitor_label: "Admin display".into(),
            similarity_threshold: 0.91,
            cursor_visible: true,
            output_mode: OutputMode::ForceScreen,
            reference_image_path: store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        app.config = admin.clone();
        app.save_admin_baseline();
        assert!(!app.admin_profile_status.unwrap().auto_restore_on_launch);

        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]))
            .save(store.reference_path())
            .unwrap();
        let user = AppConfig {
            selected_video_device_id: "user-camera".into(),
            reference_image_path: store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        save_config(&store, &user).unwrap();
        app.config = user;
        app.pending_settings_save = Some(Instant::now());
        app.open_dialog(AppDialogKind::LoadAdminConfig);

        app.load_admin_config();

        assert_eq!(app.view, AppView::Settings);
        assert!(app.dialog_is(AppDialogKind::Admin));
        assert_eq!(app.config, admin);
        assert_eq!(store.load().config, admin);
        assert!(app.pending_settings_save.is_none());
        assert!(app.settings_save_error.is_none());
        assert!(!app.admin_profile_status.unwrap().auto_restore_on_launch);

        let deadline = Instant::now() + Duration::from_secs(2);
        let snapshot = loop {
            let snapshot = app.runtime.snapshot();
            if snapshot.selected_video_device_id == "admin-camera"
                && snapshot.mode == OutputMode::ForceScreen
                && snapshot.previews.reference.is_some()
                && snapshot
                    .recent_activity
                    .iter()
                    .any(|activity| activity == "Monitor rescan requested")
            {
                break snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "runtime did not apply the manually loaded admin config"
            );
            std::thread::yield_now();
        };
        assert_eq!(
            &snapshot.previews.reference.unwrap().pixels()[..4],
            &[0, 0, 255, 255]
        );
    }

    #[test]
    fn flow_startup_admin_restore_failures_preserve_the_working_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let user = AppConfig {
            selected_video_device_id: "user-camera".into(),
            ..AppConfig::default()
        };
        save_config(&store, &user).unwrap();
        std::fs::write(
            AdminProfileStore::new(directory.path()).profile_path(),
            "invalid admin profile",
        )
        .unwrap();

        let loaded = load_config_with_admin_restore(&store);
        assert_eq!(loaded.config, user);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|warning| warning.contains("Could not restore the admin configuration"))
        );
    }

    #[test]
    fn flow_only_a_secondary_double_click_activates_the_admin_logo() {
        fn click_input(time: f64, button: egui::PointerButton) -> egui::RawInput {
            let position = Pos2::new(20.0, 20.0);
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(140.0, 140.0))),
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: position,
                        button,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..egui::RawInput::default()
            }
        }

        fn render_logo(context: &egui::Context, input: egui::RawInput) -> bool {
            let mut activated = false;
            let _ = context.run_ui(input, |ui| {
                let (_, response) = ui.allocate_exact_size(egui::vec2(96.0, 96.0), Sense::click());
                activated = admin_logo_activated(&response);
            });
            activated
        }

        let secondary = egui::Context::default();
        assert!(!render_logo(
            &secondary,
            click_input(0.0, egui::PointerButton::Secondary)
        ));
        assert!(render_logo(
            &secondary,
            click_input(0.1, egui::PointerButton::Secondary)
        ));

        let primary = egui::Context::default();
        assert!(!render_logo(
            &primary,
            click_input(0.0, egui::PointerButton::Primary)
        ));
        assert!(!render_logo(
            &primary,
            click_input(0.1, egui::PointerButton::Primary)
        ));
    }

    #[test]
    fn flow_five_primary_diagnostics_clicks_toggle_disco_without_mixed_sequences() {
        let start = Instant::now();
        let mut gesture = DiscoDiagnosticsGesture::default();
        for click in 0..4 {
            assert!(!gesture.register_primary_click(start + Duration::from_millis(click * 200)));
        }
        assert!(gesture.register_primary_click(start + Duration::from_millis(800)));

        let mut slow_gesture = DiscoDiagnosticsGesture::default();
        for click in 0..5 {
            assert!(!slow_gesture.register_primary_click(
                start + Duration::from_secs(3) + Duration::from_millis(click * 800)
            ));
        }

        for click in 0..4 {
            assert!(!gesture.register_primary_click(
                start + Duration::from_secs(4) + Duration::from_millis(click * 100)
            ));
        }
        // A click on another control resets the hidden Diagnostics sequence.
        gesture.reset();
        assert!(!gesture.register_primary_click(start + Duration::from_secs(5)));
    }

    #[test]
    fn flow_manual_setup_guide_navigation_and_dismissal_return_to_general_settings() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let mut app = SwitcherApp::new(AppConfig::default(), Vec::new(), store);
        app.setup_animations_enabled = false;
        app.view = AppView::Settings;
        app.settings_tab = SettingsTab::General;

        app.start_setup_guide(SetupReturnView::Settings);
        assert_eq!(app.view, AppView::SetupGuide);
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::HowItWorks)
        );

        app.apply_setup_action(SetupAction::Previous);
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::HowItWorks)
        );
        app.apply_setup_action(SetupAction::Next);
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::Webcam)
        );
        app.apply_setup_action(SetupAction::GoTo(SetupStep::Reference));
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::Reference)
        );
        app.setup_reference_capture = SetupReferenceCaptureState::Review {
            captured_at: Instant::now(),
        };
        app.apply_setup_action(SetupAction::Next);
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::Reference)
        );
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
        ));
        app.apply_setup_action(SetupAction::GoTo(SetupStep::Ready));
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::Reference)
        );
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
        ));
        app.apply_setup_action(SetupAction::GoTo(SetupStep::Screen));
        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::Screen)
        );
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Idle
        ));
        app.apply_setup_action(SetupAction::Close);

        assert!(app.setup_session.is_none());
        assert_eq!(app.view, AppView::Settings);
        assert_eq!(app.settings_tab, SettingsTab::General);
        assert!(
            !SetupStateStore::new(directory.path())
                .initialize(false)
                .show_setup_guide
        );
    }

    #[test]
    fn flow_setup_reference_step_requires_reference_before_continue() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            AppConfig::default(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        app.setup_animations_enabled = false;
        app.setup_session = Some(SetupSession::preview(SetupStep::Reference));
        app.setup_reference_capture = SetupReferenceCaptureState::Idle;

        app.apply_setup_action(SetupAction::Next);

        assert_eq!(
            app.setup_session.map(|session| session.step),
            Some(SetupStep::Reference)
        );
    }

    #[test]
    fn flow_setup_guide_keyboard_navigation_matches_the_visible_actions() {
        fn action_for(key: egui::Key, step: SetupStep, next_enabled: bool) -> Option<SetupAction> {
            let context = egui::Context::default();
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..egui::RawInput::default()
            };
            let mut action = None;
            let _ = context.run_ui(input, |_ui| {
                action = setup_guide_keyboard_action(&context, step, next_enabled);
            });
            action
        }

        assert_eq!(
            action_for(egui::Key::Escape, SetupStep::Webcam, true),
            Some(SetupAction::Close)
        );
        assert_eq!(
            action_for(egui::Key::ArrowLeft, SetupStep::Webcam, true),
            Some(SetupAction::Previous)
        );
        assert_eq!(
            action_for(egui::Key::ArrowLeft, SetupStep::HowItWorks, true),
            None
        );
        assert_eq!(
            action_for(egui::Key::ArrowRight, SetupStep::Ready, true),
            Some(SetupAction::Next)
        );
        assert_eq!(
            action_for(egui::Key::Enter, SetupStep::Screen, true),
            Some(SetupAction::Next)
        );
        assert_eq!(
            action_for(egui::Key::Space, SetupStep::Reference, true),
            Some(SetupAction::Next)
        );
        assert_eq!(
            action_for(egui::Key::Enter, SetupStep::Reference, false),
            None
        );
        assert_eq!(
            action_for(egui::Key::ArrowLeft, SetupStep::Reference, false),
            Some(SetupAction::Previous)
        );
    }

    #[test]
    fn flow_finishing_setup_guide_starts_automatic_mode_and_opens_dashboard() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            AppConfig {
                output_mode: OutputMode::ForceScreen,
                start_automatically: false,
                ..AppConfig::default()
            },
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        app.start_setup_guide(SetupReturnView::Settings);
        app.setup_session.as_mut().unwrap().go_to(SetupStep::Ready);
        app.apply_setup_action(SetupAction::Next);

        assert_eq!(app.view, AppView::Dashboard);
        assert!(app.setup_session.is_none());
        assert_eq!(app.config.output_mode, OutputMode::Automatic);
        let deadline = Instant::now() + Duration::from_secs(2);
        while app.snapshot().run_state != RunState::Running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.snapshot().run_state, RunState::Running);
    }

    #[test]
    fn flow_setup_guide_dismissal_is_independent_from_user_and_admin_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let config = AppConfig {
            selected_video_device_id: "setup-independent-camera".into(),
            ..AppConfig::default()
        };
        save_config(&store, &config).unwrap();
        let admin_store = AdminProfileStore::new(directory.path());
        admin_store.save(&config).unwrap();
        let admin_before = std::fs::read(admin_store.profile_path()).unwrap();

        let mut app = SwitcherApp::new(config.clone(), Vec::new(), store.clone());
        app.start_setup_guide(SetupReturnView::Dashboard);
        app.apply_setup_action(SetupAction::Close);

        assert_eq!(store.load().config, config);
        assert_eq!(
            std::fs::read(admin_store.profile_path()).unwrap(),
            admin_before
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn flow_setup_reference_candidate_tracks_capture_review_save_and_timeouts() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = SwitcherApp::new(
            AppConfig::default(),
            Vec::new(),
            ConfigStore::new(directory.path()),
        );
        let mut snapshot = ui_preview_snapshot();
        let reference_sequence = snapshot.previews.reference.as_ref().unwrap().sequence;
        let candidate = snapshot.previews.screen.clone();
        snapshot.previews.reference = None;
        snapshot.previews.reference_candidate = candidate.clone();

        app.setup_reference_capture = SetupReferenceCaptureState::CapturingCandidate {
            started_at: Instant::now(),
            previous_candidate_sequence: candidate
                .as_ref()
                .map(|frame| frame.sequence.wrapping_sub(1)),
        };
        app.update_setup_reference_capture(&snapshot);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Review { .. }
        ));

        snapshot.previews.reference_candidate = None;
        app.setup_reference_capture = SetupReferenceCaptureState::CapturingCandidate {
            started_at: Instant::now() - REFERENCE_CAPTURE_TIMEOUT,
            previous_candidate_sequence: None,
        };
        app.update_setup_reference_capture(&snapshot);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::CaptureFailed
        ));

        snapshot.previews.reference = ui_preview_snapshot().previews.reference;
        app.setup_reference_capture = SetupReferenceCaptureState::SavingCandidate {
            started_at: Instant::now(),
            previous_reference_sequence: Some(reference_sequence.wrapping_sub(1)),
        };
        app.update_setup_reference_capture(&snapshot);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::Confirmed
        ));

        snapshot.previews.reference_candidate = candidate;
        app.setup_reference_capture = SetupReferenceCaptureState::SavingCandidate {
            started_at: Instant::now() - REFERENCE_CAPTURE_TIMEOUT,
            previous_reference_sequence: Some(reference_sequence),
        };
        app.update_setup_reference_capture(&snapshot);
        assert!(matches!(
            app.setup_reference_capture,
            SetupReferenceCaptureState::SaveFailed { .. }
        ));
    }

    #[test]
    fn smoke_embedded_setup_images_have_the_approved_dimensions() {
        for bytes in [
            include_bytes!("../assets/setup-reference-example.png").as_slice(),
            include_bytes!("../assets/setup-webcam-example.png").as_slice(),
            include_bytes!("../assets/setup-screen-example.png").as_slice(),
        ] {
            let image = image::load_from_memory(bytes).unwrap();
            assert_eq!((image.width(), image.height()), (1672, 941));
        }
    }
}
