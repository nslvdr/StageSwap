use stageswap_core::{AppSnapshot, OutputMode, RunState};
use stageswap_i18n::{Locale, text};
use std::cell::Cell;
use tray_icon::menu::{
    CheckMenuItem, Icon as MenuIcon, IconMenuItem, Menu, MenuEvent, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon as TrayImage, TrayIcon, TrayIconBuilder};

use super::{
    app_icon,
    ui_icon::{self, UiIcon},
    ui_theme,
};

pub struct Tray {
    _icon: TrayIcon,
    show: IconMenuItem,
    automation: IconMenuItem,
    start_icon: MenuIcon,
    stop_icon: MenuIcon,
    automatic: CheckMenuItem,
    camera: CheckMenuItem,
    screen: CheckMenuItem,
    pip: CheckMenuItem,
    output_mode: Submenu,
    rescan_displays: IconMenuItem,
    restart_screen: IconMenuItem,
    restart_virtual_camera: IconMenuItem,
    restart_all: IconMenuItem,
    open_reference_capture: IconMenuItem,
    recovery: Submenu,
    settings: IconMenuItem,
    exit: IconMenuItem,
    locale: Cell<Locale>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    Show,
    OpenSettings,
    ToggleAutomation,
    SetMode(OutputMode),
    RescanDisplays,
    RestartScreenCapture,
    RestartVirtualCamera,
    RestartAll,
    OpenReferenceCapture,
    Exit,
}

impl Tray {
    pub fn new(locale: Locale) -> Result<Self, String> {
        let menu = Menu::new();
        let show = IconMenuItem::new(
            text(locale, "Open StageSwap"),
            true,
            Some(app_menu_icon()?),
            None,
        );
        let start_icon = action_menu_icon(UiIcon::Play, ui_theme::START_RGBA)?;
        let stop_icon = action_menu_icon(UiIcon::Stop, ui_theme::STOP_RGBA)?;
        let automation = IconMenuItem::new(
            text(locale, "Start automatic switching"),
            true,
            Some(start_icon.clone()),
            None,
        );
        let automatic = CheckMenuItem::new(text(locale, "Automatic"), true, true, None);
        let camera = CheckMenuItem::new(text(locale, "Webcam only"), true, false, None);
        let screen = CheckMenuItem::new(text(locale, "Screen only"), true, false, None);
        let pip = CheckMenuItem::new(text(locale, "Picture-in-picture"), true, false, None);
        let output_mode = Submenu::with_items(
            text(locale, "Output mode"),
            true,
            &[&automatic, &camera, &screen, &pip],
        )
        .map_err(|error| format!("could not create tray output-mode menu: {error}"))?;
        output_mode.set_icon(Some(action_menu_icon(UiIcon::Route, ui_theme::BLUE_RGBA)?));
        let rescan_displays = IconMenuItem::new(
            text(locale, "Rescan displays"),
            true,
            Some(action_menu_icon(UiIcon::Refresh, ui_theme::BLUE_RGBA)?),
            None,
        );
        let restart_screen = IconMenuItem::new(
            text(locale, "Restart screen capture"),
            true,
            Some(action_menu_icon(UiIcon::Monitor, ui_theme::BLUE_RGBA)?),
            None,
        );
        let restart_virtual_camera = IconMenuItem::new(
            text(locale, "Restart virtual camera"),
            true,
            Some(action_menu_icon(UiIcon::Broadcast, ui_theme::BLUE_RGBA)?),
            None,
        );
        let restart_all = IconMenuItem::new(
            text(locale, "Restart all"),
            true,
            Some(action_menu_icon(UiIcon::Wrench, ui_theme::BLUE_RGBA)?),
            None,
        );
        let open_reference_capture = IconMenuItem::new(
            text(locale, "Open reference capture"),
            true,
            Some(action_menu_icon(UiIcon::Image, ui_theme::BLUE_RGBA)?),
            None,
        );
        let recovery = Submenu::with_items(
            text(locale, "Recovery"),
            true,
            &[
                &rescan_displays,
                &restart_screen,
                &restart_virtual_camera,
                &restart_all,
                &open_reference_capture,
            ],
        )
        .map_err(|error| format!("could not create tray recovery menu: {error}"))?;
        recovery.set_icon(Some(action_menu_icon(UiIcon::Wrench, ui_theme::BLUE_RGBA)?));
        let settings = IconMenuItem::new(
            text(locale, "Settings"),
            true,
            Some(action_menu_icon(UiIcon::Settings, ui_theme::BLUE_RGBA)?),
            None,
        );
        let exit = IconMenuItem::new(
            text(locale, "Exit"),
            true,
            Some(action_menu_icon(UiIcon::SignOut, ui_theme::STOP_RGBA)?),
            None,
        );
        let first_separator = PredefinedMenuItem::separator();
        let second_separator = PredefinedMenuItem::separator();
        let third_separator = PredefinedMenuItem::separator();
        menu.append_items(&[
            &show,
            &first_separator,
            &automation,
            &output_mode,
            &recovery,
            &second_separator,
            &settings,
            &third_separator,
            &exit,
        ])
        .map_err(|error| format!("could not create tray menu: {error}"))?;

        let app_icon = app_icon::load(Some(32))?;
        let icon = TrayImage::from_rgba(app_icon.rgba, app_icon.width, app_icon.height)
            .map_err(|error| format!("could not create tray icon: {error}"))?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("StageSwap")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(true)
            .with_menu_on_right_click(true)
            .build()
            .map_err(|error| format!("could not create tray icon: {error}"))?;
        Ok(Self {
            _icon: icon,
            show,
            automation,
            start_icon,
            stop_icon,
            automatic,
            camera,
            screen,
            pip,
            output_mode,
            rescan_displays,
            restart_screen,
            restart_virtual_camera,
            restart_all,
            open_reference_capture,
            recovery,
            settings,
            exit,
            locale: Cell::new(locale),
        })
    }

    pub fn sync(&self, snapshot: &AppSnapshot, locale: Locale) {
        if self.locale.replace(locale) != locale {
            self.show.set_text(text(locale, "Open StageSwap"));
            self.automatic.set_text(text(locale, "Automatic"));
            self.camera.set_text(text(locale, "Webcam only"));
            self.screen.set_text(text(locale, "Screen only"));
            self.pip.set_text(text(locale, "Picture-in-picture"));
            self.output_mode.set_text(text(locale, "Output mode"));
            self.rescan_displays
                .set_text(text(locale, "Rescan displays"));
            self.restart_screen
                .set_text(text(locale, "Restart screen capture"));
            self.restart_virtual_camera
                .set_text(text(locale, "Restart virtual camera"));
            self.restart_all.set_text(text(locale, "Restart all"));
            self.open_reference_capture
                .set_text(text(locale, "Open reference capture"));
            self.recovery.set_text(text(locale, "Recovery"));
            self.settings.set_text(text(locale, "Settings"));
            self.exit.set_text(text(locale, "Exit"));
        }
        let (automation_text, automation_enabled) =
            automation_menu_state(snapshot.run_state, locale);
        if self.automation.text() != automation_text {
            self.automation.set_text(&automation_text);
            let icon = if matches!(
                snapshot.run_state,
                RunState::Running | RunState::Starting | RunState::Stopping
            ) {
                self.stop_icon.clone()
            } else {
                self.start_icon.clone()
            };
            self.automation.set_icon(Some(icon));
        }
        if self.automation.is_enabled() != automation_enabled {
            self.automation.set_enabled(automation_enabled);
        }
        set_checked(&self.automatic, snapshot.mode == OutputMode::Automatic);
        set_checked(&self.camera, snapshot.mode == OutputMode::ForceCamera);
        set_checked(&self.screen, snapshot.mode == OutputMode::ForceScreen);
        set_checked(&self.pip, snapshot.mode == OutputMode::ForcePip);
        let runtime_accepts_recovery = snapshot.run_state != RunState::Stopping;
        self.rescan_displays.set_enabled(runtime_accepts_recovery);
        self.restart_screen
            .set_enabled(runtime_accepts_recovery && snapshot.selected_monitor.is_some());
        self.restart_virtual_camera
            .set_enabled(runtime_accepts_recovery);
        self.restart_all.set_enabled(runtime_accepts_recovery);
        self.open_reference_capture
            .set_enabled(runtime_accepts_recovery && snapshot.selected_monitor.is_some());
    }

    pub fn poll(&self) -> Option<TrayAction> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id();
            if id == self.show.id() {
                return Some(TrayAction::Show);
            }
            if id == self.automation.id() {
                return Some(TrayAction::ToggleAutomation);
            }
            if id == self.automatic.id() {
                return Some(TrayAction::SetMode(OutputMode::Automatic));
            }
            if id == self.camera.id() {
                return Some(TrayAction::SetMode(OutputMode::ForceCamera));
            }
            if id == self.screen.id() {
                return Some(TrayAction::SetMode(OutputMode::ForceScreen));
            }
            if id == self.pip.id() {
                return Some(TrayAction::SetMode(OutputMode::ForcePip));
            }
            if id == self.settings.id() {
                return Some(TrayAction::OpenSettings);
            }
            if id == self.rescan_displays.id() {
                return Some(TrayAction::RescanDisplays);
            }
            if id == self.restart_screen.id() {
                return Some(TrayAction::RestartScreenCapture);
            }
            if id == self.restart_virtual_camera.id() {
                return Some(TrayAction::RestartVirtualCamera);
            }
            if id == self.restart_all.id() {
                return Some(TrayAction::RestartAll);
            }
            if id == self.open_reference_capture.id() {
                return Some(TrayAction::OpenReferenceCapture);
            }
            if id == self.exit.id() {
                return Some(TrayAction::Exit);
            }
        }
        None
    }
}

fn app_menu_icon() -> Result<MenuIcon, String> {
    let icon = app_icon::load(Some(32))?;
    MenuIcon::from_rgba(icon.rgba, icon.width, icon.height)
        .map_err(|error| format!("could not create app menu icon: {error}"))
}

fn action_menu_icon(icon: UiIcon, color: [u8; 4]) -> Result<MenuIcon, String> {
    const MENU_SIZE: u32 = 32;
    let image = ui_icon::rasterize(icon, color, MENU_SIZE)?;
    MenuIcon::from_rgba(image.into_raw(), MENU_SIZE, MENU_SIZE)
        .map_err(|error| format!("could not create tray menu action icon: {error}"))
}

fn automation_menu_state(run_state: RunState, locale: Locale) -> (String, bool) {
    let (label, enabled) = match run_state {
        RunState::Running | RunState::Starting => ("Stop automatic switching", true),
        RunState::Stopping => ("Stopping automatic switching…", false),
        RunState::Stopped | RunState::Error => ("Start automatic switching", true),
    };
    (text(locale, label).into_owned(), enabled)
}

fn set_checked(item: &CheckMenuItem, checked: bool) {
    if item.is_checked() != checked {
        item.set_checked(checked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_automation_menu_tracks_run_state() {
        assert_eq!(
            automation_menu_state(RunState::Starting, Locale::English),
            ("Stop automatic switching".into(), true)
        );
        assert_eq!(
            automation_menu_state(RunState::Running, Locale::English),
            ("Stop automatic switching".into(), true)
        );
        assert_eq!(
            automation_menu_state(RunState::Stopping, Locale::English),
            ("Stopping automatic switching…".into(), false)
        );
        assert_eq!(
            automation_menu_state(RunState::Stopped, Locale::English),
            ("Start automatic switching".into(), true)
        );
        assert_eq!(
            automation_menu_state(RunState::Error, Locale::English),
            ("Start automatic switching".into(), true)
        );
    }

    #[test]
    #[ignore = "requires an interactive Windows notification area"]
    fn contract_tray_icon_and_all_retained_menu_actions_build() {
        let tray = Tray::new(Locale::English).expect("tray should build");
        assert_ne!(tray.show.id(), tray.automation.id());
        assert_ne!(tray.automatic.id(), tray.camera.id());
        assert_ne!(tray.camera.id(), tray.screen.id());
        assert_ne!(tray.settings.id(), tray.exit.id());
    }
}
