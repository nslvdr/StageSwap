use super::*;

impl SwitcherApp {
    pub(super) fn open_admin_configuration(&mut self) {
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

    pub(super) fn save_admin_baseline(&mut self) {
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

    pub(super) fn load_admin_config(&mut self) {
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

    pub(super) fn set_admin_auto_restore(&mut self, enabled: bool) {
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

    pub(super) fn remove_admin_baseline(&mut self) {
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

    pub(super) fn record_admin_failure(&mut self, message: String) {
        self.log.write(
            "warning",
            "configuration",
            "ADMIN_CONFIGURATION_FAILED",
            &message,
        );
        self.push_notification_warning(NotificationSource::Configuration, message);
    }

    pub(super) fn import_reference_dialog(&mut self) {
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

    pub(super) fn open_log_directory(&mut self) {
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

    pub(super) fn export_logs(&mut self) {
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

    pub(super) fn clear_logs(&mut self) {
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

    pub(super) fn request_log_clear(&mut self) {
        self.open_dialog(AppDialogKind::ClearLogs);
    }

    pub(super) fn confirm_log_clear(&mut self) {
        if self.dialog_is(AppDialogKind::ClearLogs) {
            self.clear_logs();
            self.dismiss_dialog();
        }
    }
}
