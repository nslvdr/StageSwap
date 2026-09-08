use super::*;

/// Owns the independent latest-offer and version-picker state.
pub(super) struct UpdateUiState {
    pub(super) status: UpdateStatus,
    pub(super) versions: update::VersionCatalog,
    pub(super) selected_version: Option<update::ReleaseVersion>,
    pub(super) picker_open: bool,
    #[cfg(windows)]
    pub(super) notifications: UpdateNotificationState,
    pub(super) check_started: bool,
    #[cfg(windows)]
    pub(super) worker: Option<UpdateWorker>,
}

impl SwitcherApp {
    pub(super) fn request_update_check(&mut self, manual: bool) {
        if self.update_busy() {
            return;
        }
        #[cfg(windows)]
        {
            let Some(worker) = self.updates.worker.as_ref() else {
                if manual {
                    self.updates.status =
                        UpdateStatus::Failed("The update service is unavailable.".into());
                }
                return;
            };
            match worker.request(UpdateRequest::Check {
                channel: self.config.update_channel,
                manual,
            }) {
                Ok(()) => self.updates.status = UpdateStatus::Checking,
                Err(error) if manual => self.updates.status = UpdateStatus::Failed(error),
                Err(error) => {
                    self.log
                        .write("warning", "update", "UPDATE_CHECK_NOT_STARTED", &error)
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = manual;
            self.updates.status = UpdateStatus::UpToDate;
        }
    }

    pub(super) fn update_busy(&self) -> bool {
        matches!(
            self.updates.status,
            UpdateStatus::Checking | UpdateStatus::Downloading(_) | UpdateStatus::Installing
        ) || matches!(self.updates.versions, update::VersionCatalog::Loading)
    }

    /// Fetching or cancelling the picker never changes the latest-update offer.
    pub(super) fn request_update_versions(&mut self) {
        if self.config.update_channel != UpdateChannel::Beta || self.update_busy() {
            return;
        }
        self.updates.selected_version = None;
        self.updates.picker_open = true;
        self.updates.versions = update::VersionCatalog::Loading;
        #[cfg(not(windows))]
        if self.ui_preview.is_some() {
            self.updates.versions = update::VersionCatalog::Ready(update::preview_versions());
            return;
        }
        #[cfg(windows)]
        {
            let result = self
                .updates
                .worker
                .as_ref()
                .ok_or_else(|| "The update service is unavailable.".to_owned())
                .and_then(|worker| worker.request(UpdateRequest::ListVersions));
            if let Err(error) = result {
                self.updates.versions = update::VersionCatalog::Failed(error);
            }
        }
        #[cfg(not(windows))]
        {
            self.updates.versions = update::VersionCatalog::Failed(
                "Version downloads are available on Windows only.".into(),
            );
        }
    }

    /// Only an explicit catalog selection can be installed; the running version is excluded.
    pub(super) fn selected_version_release(&self) -> Option<update::AvailableUpdate> {
        if self.config.update_channel != UpdateChannel::Beta || self.update_busy() {
            return None;
        }
        let update::VersionCatalog::Ready(versions) = &self.updates.versions else {
            return None;
        };
        versions
            .iter()
            .find(|release| {
                Some(release.version) == self.updates.selected_version
                    && release.version.to_string() != env!("CARGO_PKG_VERSION")
            })
            .cloned()
    }

    #[cfg(windows)]
    pub(super) fn poll_update_worker(&mut self) {
        let Some(result) = self.updates.worker.as_ref().and_then(UpdateWorker::poll) else {
            return;
        };
        match result {
            UpdateResult::Checked { manual, result } => match result {
                Ok(Some(release)) => {
                    let should_notify = self.config.notify_updates
                        && self
                            .updates
                            .notifications
                            .should_notify(self.config.update_channel, release.version);
                    if should_notify {
                        if let Err(error) = self.updates.notifications.save(self.store.directory())
                        {
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
                    self.updates.status = UpdateStatus::Available(release);
                }
                Ok(None) => self.updates.status = UpdateStatus::UpToDate,
                Err(error) if manual => self.updates.status = UpdateStatus::Failed(error),
                Err(error) => {
                    self.log
                        .write("warning", "update", "UPDATE_CHECK_FAILED", &error);
                    self.updates.status = UpdateStatus::Idle;
                }
            },
            UpdateResult::Versions(result) => {
                self.updates.versions = match result {
                    Ok(releases) => update::VersionCatalog::Ready(releases),
                    Err(error) => update::VersionCatalog::Failed(error),
                };
            }
            UpdateResult::InstallFailed(error) => {
                self.log
                    .write("error", "update", "UPDATE_INSTALL_FAILED", &error);
                self.updates.status = UpdateStatus::Failed(error);
            }
            UpdateResult::InstallStarted => self.updates.status = UpdateStatus::Installing,
        }
    }

    pub(super) fn install_available_update(&mut self) {
        let UpdateStatus::Available(release) = &self.updates.status else {
            return;
        };
        self.install_release(release.clone());
    }

    /// Both entry points share verification and native replacement.
    pub(super) fn install_release(&mut self, release: update::AvailableUpdate) {
        if self.update_busy() {
            return;
        }
        #[cfg(not(windows))]
        if self.ui_preview.is_some() {
            self.updates.status = UpdateStatus::Downloading(release);
            return;
        }
        #[cfg(windows)]
        {
            let Some(worker) = self.updates.worker.as_ref() else {
                self.updates.status =
                    UpdateStatus::Failed("The update service is unavailable.".into());
                return;
            };
            match worker.request(UpdateRequest::Install(release.clone())) {
                Ok(()) => self.updates.status = UpdateStatus::Downloading(release),
                Err(error) => self.updates.status = UpdateStatus::Failed(error),
            }
        }
        #[cfg(not(windows))]
        {
            self.updates.status = UpdateStatus::Downloading(release);
        }
    }

    pub(super) fn updates_settings(&mut self, ui: &mut egui::Ui) {
        let status = update_status_text(self.locale(), &self.updates.status);
        settings_current_version_card(ui, &status, &self.updates.status);
        ui.add_space(12.0);

        let busy = self.update_busy();
        let update_available = matches!(self.updates.status, UpdateStatus::Available(_));
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
            self.updates.versions = update::VersionCatalog::Idle;
            self.updates.selected_version = None;
            self.updates.picker_open = false;
            self.updates.status = UpdateStatus::Idle;
            self.request_update_check(false);
        }
    }

    /// Uses the same modal surface and action buttons as the other application dialogs.
    pub(super) fn update_version_picker(&mut self, context: &egui::Context) {
        if !self.updates.picker_open {
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
                match &self.updates.versions {
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
                                            &mut self.updates.selected_version,
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
            self.updates.picker_open = false;
            self.updates.selected_version = None;
        } else if install {
            if let Some(release) = self.selected_version_release() {
                self.updates.picker_open = false;
                self.updates.selected_version = None;
                self.install_release(release);
            }
        } else if retry {
            self.request_update_versions();
        }
    }
}
