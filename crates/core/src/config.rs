use crate::OutputMode;
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const ADMIN_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StillImagePipLayout {
    #[default]
    WebcamMain,
    ScreenMain,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StillImagePipSize {
    Mini,
    #[default]
    Medium,
    Large,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AppConfig {
    pub schema_version: u32,
    pub selected_video_device_id: String,
    pub crop_webcam_to_16_9: bool,
    pub selected_monitor_label: String,
    pub reference_image_path: String,
    pub similarity_threshold: f64,
    pub cursor_visible: bool,
    pub automatic_monitor_rescans: bool,
    pub automatic_screen_capture_recovery: bool,
    pub still_image_pip_enabled: bool,
    pub still_image_pip_delay_seconds: u32,
    pub still_image_pip_layout: StillImagePipLayout,
    pub still_image_pip_size: StillImagePipSize,
    pub start_with_windows: bool,
    pub start_minimized: bool,
    pub start_automatically: bool,
    pub close_to_tray: bool,
    pub show_notifications: bool,
    pub update_channel: UpdateChannel,
    pub notify_updates: bool,
    pub verbose_logging: bool,
    pub interface_language: String,
    pub confirm_exit: bool,
    pub output_mode: OutputMode,
    pub placeholder_color_bgra: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            selected_video_device_id: String::new(),
            crop_webcam_to_16_9: true,
            selected_monitor_label: String::new(),
            reference_image_path: String::new(),
            similarity_threshold: 0.98,
            cursor_visible: false,
            automatic_monitor_rescans: true,
            automatic_screen_capture_recovery: true,
            still_image_pip_enabled: true,
            still_image_pip_delay_seconds: 45,
            still_image_pip_layout: StillImagePipLayout::WebcamMain,
            still_image_pip_size: StillImagePipSize::Medium,
            start_with_windows: false,
            start_minimized: true,
            start_automatically: true,
            close_to_tray: true,
            show_notifications: true,
            update_channel: UpdateChannel::Stable,
            notify_updates: true,
            verbose_logging: false,
            interface_language: "en-US".into(),
            confirm_exit: true,
            output_mode: OutputMode::Automatic,
            placeholder_color_bgra: 0xff17_1719,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AppConfigFile {
    schema_version: u32,
    selected_video_device_id: String,
    crop_webcam_to_16_9: bool,
    selected_monitor_label: String,
    reference_image_path: String,
    similarity_threshold: f64,
    cursor_visible: bool,
    automatic_monitor_rescans: bool,
    automatic_screen_capture_recovery: Option<bool>,
    still_image_pip_enabled: bool,
    still_image_pip_delay_seconds: u32,
    still_image_pip_layout: StillImagePipLayout,
    still_image_pip_size: StillImagePipSize,
    start_with_windows: bool,
    start_minimized: bool,
    start_automatically: bool,
    close_to_tray: bool,
    show_notifications: bool,
    update_channel: UpdateChannel,
    notify_updates: bool,
    verbose_logging: bool,
    interface_language: String,
    confirm_exit: bool,
    output_mode: OutputMode,
    placeholder_color_bgra: u32,
}

impl Default for AppConfigFile {
    fn default() -> Self {
        let config = AppConfig::default();
        Self {
            schema_version: config.schema_version,
            selected_video_device_id: config.selected_video_device_id,
            crop_webcam_to_16_9: config.crop_webcam_to_16_9,
            selected_monitor_label: config.selected_monitor_label,
            reference_image_path: config.reference_image_path,
            similarity_threshold: config.similarity_threshold,
            cursor_visible: config.cursor_visible,
            automatic_monitor_rescans: config.automatic_monitor_rescans,
            automatic_screen_capture_recovery: None,
            still_image_pip_enabled: config.still_image_pip_enabled,
            still_image_pip_delay_seconds: config.still_image_pip_delay_seconds,
            still_image_pip_layout: config.still_image_pip_layout,
            still_image_pip_size: config.still_image_pip_size,
            start_with_windows: config.start_with_windows,
            start_minimized: config.start_minimized,
            start_automatically: config.start_automatically,
            close_to_tray: config.close_to_tray,
            show_notifications: config.show_notifications,
            update_channel: config.update_channel,
            notify_updates: config.notify_updates,
            verbose_logging: config.verbose_logging,
            interface_language: config.interface_language,
            confirm_exit: config.confirm_exit,
            output_mode: config.output_mode,
            placeholder_color_bgra: config.placeholder_color_bgra,
        }
    }
}

impl<'de> Deserialize<'de> for AppConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let file = AppConfigFile::deserialize(deserializer)?;
        Ok(Self {
            schema_version: file.schema_version,
            selected_video_device_id: file.selected_video_device_id,
            crop_webcam_to_16_9: file.crop_webcam_to_16_9,
            selected_monitor_label: file.selected_monitor_label,
            reference_image_path: file.reference_image_path,
            similarity_threshold: file.similarity_threshold,
            cursor_visible: file.cursor_visible,
            automatic_monitor_rescans: file.automatic_monitor_rescans,
            automatic_screen_capture_recovery: file
                .automatic_screen_capture_recovery
                .unwrap_or(file.automatic_monitor_rescans),
            still_image_pip_enabled: file.still_image_pip_enabled,
            still_image_pip_delay_seconds: file.still_image_pip_delay_seconds,
            still_image_pip_layout: file.still_image_pip_layout,
            still_image_pip_size: file.still_image_pip_size,
            start_with_windows: file.start_with_windows,
            start_minimized: file.start_minimized,
            start_automatically: file.start_automatically,
            close_to_tray: file.close_to_tray,
            show_notifications: file.show_notifications,
            update_channel: file.update_channel,
            notify_updates: file.notify_updates,
            verbose_logging: file.verbose_logging,
            interface_language: file.interface_language,
            confirm_exit: file.confirm_exit,
            output_mode: file.output_mode,
            placeholder_color_bgra: file.placeholder_color_bgra,
        })
    }
}

impl AppConfig {
    fn validate(mut self) -> io::Result<Self> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported schema_version",
            ));
        }
        if !self.similarity_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.similarity_threshold)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "similarity_threshold must be between zero and one",
            ));
        }
        if ![30, 45, 60, 120, 300].contains(&self.still_image_pip_delay_seconds) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "still_image_pip_delay_seconds must be an available preset",
            ));
        }
        self.placeholder_color_bgra |= 0xff00_0000;
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigLoad {
    pub config: AppConfig,
    pub used_backup: bool,
    pub used_defaults: bool,
    pub warnings: Vec<String>,
}

impl Default for ConfigLoad {
    fn default() -> Self {
        Self {
            config: AppConfig::default(),
            used_backup: false,
            used_defaults: true,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    directory: PathBuf,
}

impl ConfigStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn config_path(&self) -> PathBuf {
        self.directory.join("config.json")
    }
    pub fn backup_path(&self) -> PathBuf {
        self.directory.join("config.backup.json")
    }
    pub fn reference_path(&self) -> PathBuf {
        self.directory.join("reference.png")
    }
    pub fn logs_path(&self) -> PathBuf {
        self.directory.join("logs")
    }

    pub fn parse(json: &str) -> io::Result<AppConfig> {
        serde_json::from_str::<AppConfig>(json)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            .validate()
    }

    pub fn load(&self) -> ConfigLoad {
        if !self.config_path().exists() {
            return ConfigLoad::default();
        }
        match fs::read_to_string(self.config_path()).and_then(|json| Self::parse(&json)) {
            Ok(config) => ConfigLoad {
                config,
                used_defaults: false,
                ..ConfigLoad::default()
            },
            Err(primary_error) => {
                let mut warnings = vec![format!("Primary configuration invalid: {primary_error}")];
                let invalid = self.directory.join("config.invalid.json");
                if let Err(error) = fs::copy(self.config_path(), invalid) {
                    warnings.push(format!("Could not preserve invalid configuration: {error}"));
                }
                match fs::read_to_string(self.backup_path()).and_then(|json| Self::parse(&json)) {
                    Ok(config) => {
                        warnings.push("Loaded last valid configuration backup".into());
                        ConfigLoad {
                            config,
                            used_backup: true,
                            used_defaults: false,
                            warnings,
                        }
                    }
                    Err(backup_error) => {
                        warnings.push(format!(
                            "Backup configuration invalid: {backup_error}; using defaults"
                        ));
                        ConfigLoad {
                            warnings,
                            ..ConfigLoad::default()
                        }
                    }
                }
            }
        }
    }

    pub fn save(&self, config: &AppConfig) -> io::Result<()> {
        self.save_with_replace(config, atomic_replace)
    }

    pub fn save_with_replace(
        &self,
        config: &AppConfig,
        replace: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<()> {
        config.clone().validate()?;
        fs::create_dir_all(&self.directory)?;
        let temporary = self.directory.join("config.tmp.json");
        let bytes = serde_json::to_vec_pretty(config)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        // A recovered configuration must not back up the corrupt primary over
        // the last usable copy, especially if the following replacement fails.
        if let Ok(primary) = fs::read_to_string(self.config_path())
            && Self::parse(&primary).is_ok()
        {
            fs::copy(self.config_path(), self.backup_path())?;
        }
        if let Err(error) = replace(&temporary, &self.config_path()) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdminProfileStatus {
    pub auto_restore_on_launch: bool,
    pub reference_included: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdminRestoreOutcome {
    Missing,
    Disabled,
    Restored,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminProfileFile {
    schema_version: u32,
    auto_restore_on_launch: bool,
    config: AppConfig,
    reference_file: Option<String>,
}

impl AdminProfileFile {
    fn validate(mut self, directory: &Path) -> io::Result<Self> {
        if self.schema_version != ADMIN_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported admin schema_version",
            ));
        }
        self.config = self.config.validate()?;
        self.config.reference_image_path = directory.join("reference.png").display().to_string();
        if let Some(reference_file) = &self.reference_file {
            validate_admin_reference_name(reference_file)?;
            validate_reference_image(&directory.join(reference_file))?;
        }
        Ok(self)
    }

    fn status(&self) -> AdminProfileStatus {
        AdminProfileStatus {
            auto_restore_on_launch: self.auto_restore_on_launch,
            reference_included: self.reference_file.is_some(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AdminProfileStore {
    directory: PathBuf,
}

impl AdminProfileStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn profile_path(&self) -> PathBuf {
        self.directory.join("admin-profile.json")
    }

    pub fn status(&self) -> io::Result<Option<AdminProfileStatus>> {
        self.load_profile()
            .map(|profile| profile.map(|profile| profile.status()))
    }

    pub fn save(&self, config: &AppConfig) -> io::Result<AdminProfileStatus> {
        self.save_with_replace(config, atomic_replace)
    }

    pub fn save_with_replace(
        &self,
        config: &AppConfig,
        mut replace: impl FnMut(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<AdminProfileStatus> {
        let previous = self.load_profile().ok().flatten();
        let auto_restore_on_launch = previous
            .as_ref()
            .is_some_and(|profile| profile.auto_restore_on_launch);
        let mut snapshot = config.clone().validate()?;
        let source_reference = PathBuf::from(&snapshot.reference_image_path);
        snapshot.reference_image_path = self.directory.join("reference.png").display().to_string();

        fs::create_dir_all(&self.directory)?;
        let reference_file = if source_reference.exists() {
            validate_reference_image(&source_reference)?;
            Some(self.copy_reference_snapshot(&source_reference)?)
        } else {
            None
        };
        let profile = AdminProfileFile {
            schema_version: ADMIN_SCHEMA_VERSION,
            auto_restore_on_launch,
            config: snapshot,
            reference_file,
        };
        let temporary = self.directory.join("admin-profile.tmp.json");
        if let Err(error) = write_json_file(&temporary, &profile) {
            if let Some(reference_file) = &profile.reference_file {
                let _ = fs::remove_file(self.directory.join(reference_file));
            }
            return Err(error);
        }
        if let Err(error) = replace(&temporary, &self.profile_path()) {
            let _ = fs::remove_file(&temporary);
            if let Some(reference_file) = &profile.reference_file {
                let _ = fs::remove_file(self.directory.join(reference_file));
            }
            return Err(error);
        }

        if let Some(previous_reference) = previous.and_then(|profile| profile.reference_file)
            && Some(&previous_reference) != profile.reference_file.as_ref()
        {
            let _ = fs::remove_file(self.directory.join(previous_reference));
        }
        Ok(profile.status())
    }

    pub fn set_auto_restore_on_launch(&self, enabled: bool) -> io::Result<AdminProfileStatus> {
        self.set_auto_restore_with_replace(enabled, atomic_replace)
    }

    pub fn set_auto_restore_with_replace(
        &self,
        enabled: bool,
        mut replace: impl FnMut(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<AdminProfileStatus> {
        let mut profile = self.load_profile()?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "admin baseline does not exist")
        })?;
        profile.auto_restore_on_launch = enabled;
        let temporary = self.directory.join("admin-profile.tmp.json");
        write_json_file(&temporary, &profile)?;
        if let Err(error) = replace(&temporary, &self.profile_path()) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(profile.status())
    }

    pub fn remove(&self) -> io::Result<bool> {
        let reference_file = self
            .load_profile()
            .ok()
            .flatten()
            .and_then(|profile| profile.reference_file);
        match fs::remove_file(self.profile_path()) {
            Ok(()) => {
                if let Some(reference_file) = reference_file {
                    let _ = fs::remove_file(self.directory.join(reference_file));
                }
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn restore_on_launch(&self) -> io::Result<AdminRestoreOutcome> {
        self.restore_on_launch_with_replace(atomic_replace)
    }

    pub fn restore_on_launch_with_replace(
        &self,
        replace: impl FnMut(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<AdminRestoreOutcome> {
        self.restore_with_replace(true, replace)
    }

    pub fn restore_now(&self) -> io::Result<AdminRestoreOutcome> {
        self.restore_now_with_replace(atomic_replace)
    }

    pub fn restore_now_with_replace(
        &self,
        replace: impl FnMut(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<AdminRestoreOutcome> {
        self.restore_with_replace(false, replace)
    }

    fn restore_with_replace(
        &self,
        require_auto_restore: bool,
        mut replace: impl FnMut(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<AdminRestoreOutcome> {
        let Some(profile) = self.load_profile()? else {
            return Ok(AdminRestoreOutcome::Missing);
        };
        if require_auto_restore && !profile.auto_restore_on_launch {
            return Ok(AdminRestoreOutcome::Disabled);
        }

        fs::create_dir_all(&self.directory)?;
        let working_reference = self.directory.join("reference.png");
        let reference_backup = self.directory.join("reference.restore-backup.png");
        let reference_temporary = self.directory.join("reference.restore-tmp.png");
        let had_working_reference = working_reference.exists();
        if had_working_reference {
            copy_file_synced(&working_reference, &reference_backup)?;
        } else {
            let _ = fs::remove_file(&reference_backup);
        }

        let reference_result = if let Some(reference_file) = &profile.reference_file {
            copy_file_synced(&self.directory.join(reference_file), &reference_temporary)
                .and_then(|()| replace(&reference_temporary, &working_reference))
        } else if had_working_reference {
            fs::remove_file(&working_reference)
        } else {
            Ok(())
        };
        if let Err(error) = reference_result {
            let _ = fs::remove_file(&reference_temporary);
            let _ = fs::remove_file(&reference_backup);
            return Err(error);
        }

        let config_store = ConfigStore::new(&self.directory);
        if let Err(error) = config_store
            .save_with_replace(&profile.config, |source, destination| {
                replace(source, destination)
            })
        {
            let rollback = if had_working_reference {
                copy_file_synced(&reference_backup, &reference_temporary)
                    .and_then(|()| replace(&reference_temporary, &working_reference))
            } else if working_reference.exists() {
                fs::remove_file(&working_reference)
            } else {
                Ok(())
            };
            let _ = fs::remove_file(&reference_temporary);
            let _ = fs::remove_file(&reference_backup);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(io::Error::other(format!(
                    "{error}; reference rollback failed: {rollback_error}"
                ))),
            };
        }

        let _ = fs::remove_file(&reference_temporary);
        let _ = fs::remove_file(&reference_backup);
        Ok(AdminRestoreOutcome::Restored)
    }

    fn load_profile(&self) -> io::Result<Option<AdminProfileFile>> {
        let json = match fs::read_to_string(self.profile_path()) {
            Ok(json) => json,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        serde_json::from_str::<AdminProfileFile>(&json)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            .validate(&self.directory)
            .map(Some)
    }

    fn copy_reference_snapshot(&self, source: &Path) -> io::Result<String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for suffix in 0..16 {
            let name = format!(
                "admin-reference-{timestamp}-{}-{suffix}.png",
                std::process::id()
            );
            let destination = self.directory.join(&name);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
            {
                Ok(mut output) => {
                    let copy_result = fs::File::open(source).and_then(|mut input| {
                        io::copy(&mut input, &mut output)?;
                        output.sync_all()
                    });
                    if let Err(error) = copy_result {
                        drop(output);
                        let _ = fs::remove_file(&destination);
                        return Err(error);
                    }
                    if let Err(error) = validate_reference_image(&destination) {
                        let _ = fs::remove_file(&destination);
                        return Err(error);
                    }
                    return Ok(name);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate an admin reference snapshot",
        ))
    }
}

fn validate_admin_reference_name(name: &str) -> io::Result<()> {
    let path = Path::new(name);
    let is_single_component = path.file_name().and_then(|name| name.to_str()) == Some(name);
    if !is_single_component || !name.starts_with("admin-reference-") || !name.ends_with(".png") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid admin reference filename",
        ));
    }
    Ok(())
}

fn validate_reference_image(path: &Path) -> io::Result<()> {
    image::open(path)
        .map(|_| ())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_json_file(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut file = fs::File::create(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn copy_file_synced(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::File::create(destination)?;
    io::copy(&mut input, &mut output)?;
    output.sync_all()
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(source, destination)
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_reference(path: &Path, color: [u8; 4]) {
        image::RgbaImage::from_pixel(2, 2, image::Rgba(color))
            .save(path)
            .unwrap();
    }

    fn reference_color(path: &Path) -> [u8; 4] {
        image::open(path).unwrap().to_rgba8().get_pixel(0, 0).0
    }

    #[test]
    fn contract_config_schema_round_trip_and_update_preferences() {
        let config = AppConfig {
            selected_video_device_id: "camera\\id\"one".into(),
            crop_webcam_to_16_9: false,
            selected_monitor_label: "Studio Display".into(),
            automatic_monitor_rescans: false,
            still_image_pip_enabled: true,
            still_image_pip_delay_seconds: 120,
            still_image_pip_layout: StillImagePipLayout::ScreenMain,
            still_image_pip_size: StillImagePipSize::Large,
            verbose_logging: true,
            output_mode: OutputMode::ForceScreen,
            ..AppConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"still_image_pip_size\":\"large\""));
        assert_eq!(ConfigStore::parse(&json).unwrap(), config);
        assert_eq!(ConfigStore::parse("{}").unwrap(), AppConfig::default());
        let invalid_schema = serde_json::json!({ "schema_version": 2 }).to_string();
        assert!(ConfigStore::parse(&invalid_schema).is_err());
        let update_preferences = AppConfig {
            update_channel: UpdateChannel::Beta,
            notify_updates: false,
            ..AppConfig::default()
        };
        let update_json = serde_json::to_string(&update_preferences).unwrap();
        assert_eq!(
            ConfigStore::parse(&update_json).unwrap(),
            update_preferences
        );
        assert!(update_json.contains("\"schema_version\":1"));
        assert!(update_json.contains("\"update_channel\":\"beta\""));
        let forced_pip = AppConfig {
            output_mode: OutputMode::ForcePip,
            ..AppConfig::default()
        };
        let forced_pip_json = serde_json::to_string(&forced_pip).unwrap();
        assert!(forced_pip_json.contains("\"output_mode\":\"force_pip\""));
        assert_eq!(ConfigStore::parse(&forced_pip_json).unwrap(), forced_pip);
    }

    #[test]
    fn contract_config_rejects_unknown_and_invalid_values_without_normalizing_bad_data() {
        assert!(ConfigStore::parse(r#"{"schema_version":1,"unexpected":true}"#).is_err());
        for threshold in [-0.01, 1.01] {
            let json = serde_json::json!({
                "schema_version": 1,
                "similarity_threshold": threshold,
            })
            .to_string();
            assert!(ConfigStore::parse(&json).is_err());
        }
        for delay in [0, 29, 31, 90, 301] {
            let json = serde_json::json!({
                "schema_version": 1,
                "still_image_pip_delay_seconds": delay,
            })
            .to_string();
            assert!(ConfigStore::parse(&json).is_err());
        }
        assert!(
            ConfigStore::parse(r#"{"schema_version":1,"still_image_pip_size":"extra_large"}"#)
                .is_err()
        );

        let config = ConfigStore::parse(
            &serde_json::json!({
                "schema_version": 1,
                "placeholder_color_bgra": 0x0011_2233_u32,
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(config.placeholder_color_bgra, 0xff11_2233);
    }

    #[test]
    fn contract_failed_config_replace_cleans_temporary_file_and_preserves_previous_config() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let original = AppConfig {
            selected_video_device_id: "original".into(),
            ..AppConfig::default()
        };
        store.save(&original).unwrap();

        let replacement = AppConfig {
            selected_video_device_id: "replacement".into(),
            ..original.clone()
        };
        let error = store
            .save_with_replace(&replacement, |_, _| {
                Err(io::Error::other("simulated config replacement failure"))
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("simulated config replacement failure")
        );
        assert!(!directory.path().join("config.tmp.json").exists());
        assert_eq!(store.load().config, original);

        fs::write(store.config_path(), b"corrupt primary").unwrap();
        let recovered = store.load();
        assert!(recovered.used_backup);
        assert!(
            store
                .save_with_replace(&recovered.config, |_, _| {
                    Err(io::Error::other("replacement remains unavailable"))
                })
                .is_err()
        );
        let recovered_again = store.load();
        assert!(recovered_again.used_backup);
        assert_eq!(recovered_again.config, original);
    }

    #[test]
    fn contract_config_legacy_defaults_and_recovery_preference_migration() {
        let config = ConfigStore::parse(r#"{"schema_version":1}"#).unwrap();
        assert!(config.crop_webcam_to_16_9);
        assert!(config.selected_monitor_label.is_empty());
        assert!(config.automatic_monitor_rescans);
        assert!(config.automatic_screen_capture_recovery);
        assert_eq!(config.update_channel, UpdateChannel::Stable);
        assert!(config.notify_updates);
        assert!(!config.verbose_logging);
        assert!(config.still_image_pip_enabled);
        assert_eq!(config.still_image_pip_delay_seconds, 45);
        assert_eq!(
            config.still_image_pip_layout,
            StillImagePipLayout::WebcamMain
        );
        assert_eq!(config.still_image_pip_size, StillImagePipSize::Medium);
        let disabled =
            ConfigStore::parse(r#"{"schema_version":1,"automatic_monitor_rescans":false}"#)
                .unwrap();
        assert!(!disabled.automatic_monitor_rescans);
        assert!(!disabled.automatic_screen_capture_recovery);

        let enabled =
            ConfigStore::parse(r#"{"schema_version":1,"automatic_monitor_rescans":true}"#).unwrap();
        assert!(enabled.automatic_monitor_rescans);
        assert!(enabled.automatic_screen_capture_recovery);
        for (automatic_monitor_rescans, automatic_screen_capture_recovery) in
            [(false, true), (true, false)]
        {
            let config = ConfigStore::parse(
                &serde_json::json!({
                    "schema_version": 1,
                    "automatic_monitor_rescans": automatic_monitor_rescans,
                    "automatic_screen_capture_recovery": automatic_screen_capture_recovery,
                })
                .to_string(),
            )
            .unwrap();
            assert_eq!(config.automatic_monitor_rescans, automatic_monitor_rescans);
            assert_eq!(
                config.automatic_screen_capture_recovery,
                automatic_screen_capture_recovery
            );
        }
    }

    #[test]
    fn contract_config_load_uses_backup_then_defaults_when_sources_are_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path());
        let original = AppConfig {
            selected_video_device_id: "saved".into(),
            ..AppConfig::default()
        };
        store.save(&original).unwrap();
        store
            .save(&AppConfig {
                selected_video_device_id: "new".into(),
                ..original
            })
            .unwrap();
        fs::write(store.config_path(), "bad").unwrap();
        let loaded = store.load();
        assert!(loaded.used_backup);
        assert!(!loaded.used_defaults);
        assert_eq!(loaded.config.selected_video_device_id, "saved");
        fs::write(store.backup_path(), "bad too").unwrap();
        let defaults = store.load();
        assert!(defaults.used_defaults);
        assert_eq!(defaults.config, AppConfig::default());
        let missing = ConfigStore::new(tempfile::tempdir().unwrap().path()).load();
        assert!(missing.used_defaults);
        assert!(!missing.used_backup);
    }

    #[test]
    fn contract_admin_baseline_metadata_and_reference_absence() {
        let directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(directory.path());
        let admin_store = AdminProfileStore::new(directory.path());
        write_reference(&config_store.reference_path(), [255, 0, 0, 255]);
        let original = AppConfig {
            selected_video_device_id: "admin-camera".into(),
            reference_image_path: config_store.reference_path().display().to_string(),
            still_image_pip_size: StillImagePipSize::Large,
            ..AppConfig::default()
        };

        let status = admin_store.save(&original).unwrap();
        assert_eq!(
            status,
            AdminProfileStatus {
                auto_restore_on_launch: false,
                reference_included: true,
            }
        );
        assert_eq!(
            admin_store
                .load_profile()
                .unwrap()
                .unwrap()
                .config
                .still_image_pip_size,
            StillImagePipSize::Large
        );
        admin_store.set_auto_restore_on_launch(true).unwrap();
        write_reference(&config_store.reference_path(), [0, 255, 0, 255]);
        let replacement = AppConfig {
            selected_video_device_id: "replacement-camera".into(),
            reference_image_path: config_store.reference_path().display().to_string(),
            verbose_logging: true,
            ..AppConfig::default()
        };
        let replaced = admin_store.save(&replacement).unwrap();
        assert!(replaced.auto_restore_on_launch);
        assert_eq!(
            admin_store.load_profile().unwrap().unwrap().config,
            replacement
        );
        assert!(admin_store.remove().unwrap());
        assert_eq!(admin_store.status().unwrap(), None);

        let no_reference_directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(no_reference_directory.path());
        let admin_store = AdminProfileStore::new(no_reference_directory.path());
        let admin = AppConfig {
            selected_video_device_id: "admin-camera".into(),
            reference_image_path: config_store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        assert!(!admin_store.save(&admin).unwrap().reference_included);
        admin_store.set_auto_restore_on_launch(true).unwrap();
        write_reference(&config_store.reference_path(), [0, 0, 255, 255]);
        config_store
            .save(&AppConfig {
                selected_video_device_id: "user-camera".into(),
                reference_image_path: config_store.reference_path().display().to_string(),
                ..AppConfig::default()
            })
            .unwrap();
        assert_eq!(
            admin_store.restore_on_launch().unwrap(),
            AdminRestoreOutcome::Restored
        );
        assert_eq!(config_store.load().config, admin);
        assert!(!config_store.reference_path().exists());
    }

    #[test]
    fn contract_admin_restore_on_launch_respects_the_auto_restore_toggle() {
        let enabled_directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(enabled_directory.path());
        let admin_store = AdminProfileStore::new(enabled_directory.path());
        write_reference(&config_store.reference_path(), [255, 0, 0, 255]);
        let admin = AppConfig {
            selected_video_device_id: "admin-camera".into(),
            cursor_visible: true,
            reference_image_path: config_store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        admin_store.save(&admin).unwrap();
        admin_store.set_auto_restore_on_launch(true).unwrap();
        for user_camera in ["first-user-camera", "second-user-camera"] {
            config_store
                .save(&AppConfig {
                    selected_video_device_id: user_camera.into(),
                    reference_image_path: config_store.reference_path().display().to_string(),
                    ..AppConfig::default()
                })
                .unwrap();
            write_reference(&config_store.reference_path(), [0, 0, 255, 255]);
            assert_eq!(
                admin_store.restore_on_launch().unwrap(),
                AdminRestoreOutcome::Restored
            );
            assert_eq!(config_store.load().config, admin);
            assert_eq!(
                reference_color(&config_store.reference_path()),
                [255, 0, 0, 255]
            );
        }

        let disabled_directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(disabled_directory.path());
        let admin_store = AdminProfileStore::new(disabled_directory.path());
        write_reference(&config_store.reference_path(), [255, 0, 0, 255]);
        admin_store
            .save(&AppConfig {
                selected_video_device_id: "admin-camera".into(),
                reference_image_path: config_store.reference_path().display().to_string(),
                ..AppConfig::default()
            })
            .unwrap();
        let user = AppConfig {
            selected_video_device_id: "user-camera".into(),
            reference_image_path: config_store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        config_store.save(&user).unwrap();
        write_reference(&config_store.reference_path(), [0, 0, 255, 255]);
        assert_eq!(
            admin_store.restore_on_launch().unwrap(),
            AdminRestoreOutcome::Disabled
        );
        assert_eq!(config_store.load().config, user);
        assert_eq!(
            reference_color(&config_store.reference_path()),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn contract_manual_admin_restore_restores_or_reports_missing_profiles() {
        let directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(directory.path());
        let admin_store = AdminProfileStore::new(directory.path());
        write_reference(&config_store.reference_path(), [255, 0, 0, 255]);
        let admin = AppConfig {
            selected_video_device_id: "admin-camera".into(),
            cursor_visible: true,
            reference_image_path: config_store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        let status = admin_store.save(&admin).unwrap();
        assert!(!status.auto_restore_on_launch);
        config_store
            .save(&AppConfig {
                selected_video_device_id: "user-camera".into(),
                reference_image_path: config_store.reference_path().display().to_string(),
                ..AppConfig::default()
            })
            .unwrap();
        write_reference(&config_store.reference_path(), [0, 0, 255, 255]);
        assert_eq!(
            admin_store.restore_now().unwrap(),
            AdminRestoreOutcome::Restored
        );
        assert_eq!(config_store.load().config, admin);
        assert_eq!(
            reference_color(&config_store.reference_path()),
            [255, 0, 0, 255]
        );
        assert!(
            !admin_store
                .status()
                .unwrap()
                .unwrap()
                .auto_restore_on_launch
        );

        let missing_directory = tempfile::tempdir().unwrap();
        assert_eq!(
            AdminProfileStore::new(missing_directory.path())
                .restore_now()
                .unwrap(),
            AdminRestoreOutcome::Missing
        );
    }

    #[test]
    fn contract_invalid_admin_profiles_preserve_working_files() {
        {
            let directory = tempfile::tempdir().unwrap();
            let config_store = ConfigStore::new(directory.path());
            let admin_store = AdminProfileStore::new(directory.path());
            let user = AppConfig {
                selected_video_device_id: "user-camera".into(),
                reference_image_path: config_store.reference_path().display().to_string(),
                ..AppConfig::default()
            };
            config_store.save(&user).unwrap();
            write_reference(&config_store.reference_path(), [0, 0, 255, 255]);
            let config_before = fs::read(config_store.config_path()).unwrap();
            let reference_before = fs::read(config_store.reference_path()).unwrap();
            fs::write(admin_store.profile_path(), "not valid json").unwrap();

            assert!(admin_store.restore_on_launch().is_err());
            assert!(admin_store.restore_now().is_err());
            assert_eq!(fs::read(config_store.config_path()).unwrap(), config_before);
            assert_eq!(
                fs::read(config_store.reference_path()).unwrap(),
                reference_before
            );
        }

        {
            let directory = tempfile::tempdir().unwrap();
            let config_store = ConfigStore::new(directory.path());
            let admin_store = AdminProfileStore::new(directory.path());
            write_reference(&config_store.reference_path(), [255, 0, 0, 255]);
            admin_store
                .save(&AppConfig {
                    selected_video_device_id: "admin-camera".into(),
                    reference_image_path: config_store.reference_path().display().to_string(),
                    ..AppConfig::default()
                })
                .unwrap();
            admin_store.set_auto_restore_on_launch(true).unwrap();
            let protected_reference = admin_store
                .load_profile()
                .unwrap()
                .unwrap()
                .reference_file
                .unwrap();
            let user = AppConfig {
                selected_video_device_id: "user-camera".into(),
                reference_image_path: config_store.reference_path().display().to_string(),
                ..AppConfig::default()
            };
            config_store.save(&user).unwrap();
            write_reference(&config_store.reference_path(), [0, 0, 255, 255]);
            fs::write(directory.path().join(protected_reference), "corrupt image").unwrap();

            assert!(admin_store.restore_on_launch().is_err());
            assert!(admin_store.restore_now().is_err());
            assert_eq!(config_store.load().config, user);
            assert_eq!(
                reference_color(&config_store.reference_path()),
                [0, 0, 255, 255]
            );
        }

        let directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(directory.path());
        let admin_store = AdminProfileStore::new(directory.path());
        admin_store.save(&AppConfig::default()).unwrap();
        let mut profile = serde_json::from_slice::<serde_json::Value>(
            &fs::read(admin_store.profile_path()).unwrap(),
        )
        .unwrap();
        profile["schema_version"] = serde_json::json!(ADMIN_SCHEMA_VERSION + 1);
        fs::write(
            admin_store.profile_path(),
            serde_json::to_vec_pretty(&profile).unwrap(),
        )
        .unwrap();
        assert!(admin_store.status().is_err());
        assert!(admin_store.restore_on_launch().is_err());
        assert!(!config_store.config_path().exists());
    }

    #[test]
    fn contract_failed_admin_replacement_keeps_the_previous_baseline() {
        let directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(directory.path());
        let admin_store = AdminProfileStore::new(directory.path());
        write_reference(&config_store.reference_path(), [255, 0, 0, 255]);
        let original = AppConfig {
            selected_video_device_id: "original-admin-camera".into(),
            reference_image_path: config_store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        admin_store.save(&original).unwrap();

        write_reference(&config_store.reference_path(), [0, 255, 0, 255]);
        let replacement = AppConfig {
            selected_video_device_id: "replacement-admin-camera".into(),
            reference_image_path: config_store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        let error = admin_store
            .save_with_replace(&replacement, |_, _| {
                Err(io::Error::other("simulated replacement failure"))
            })
            .unwrap_err();
        assert!(error.to_string().contains("simulated replacement failure"));
        assert_eq!(
            admin_store.load_profile().unwrap().unwrap().config,
            original
        );
    }

    #[test]
    fn contract_failed_config_restore_rolls_back_the_working_reference() {
        let directory = tempfile::tempdir().unwrap();
        let config_store = ConfigStore::new(directory.path());
        let admin_store = AdminProfileStore::new(directory.path());
        write_reference(&config_store.reference_path(), [255, 0, 0, 255]);
        admin_store
            .save(&AppConfig {
                selected_video_device_id: "admin-camera".into(),
                reference_image_path: config_store.reference_path().display().to_string(),
                ..AppConfig::default()
            })
            .unwrap();
        admin_store.set_auto_restore_on_launch(true).unwrap();

        let user = AppConfig {
            selected_video_device_id: "user-camera".into(),
            reference_image_path: config_store.reference_path().display().to_string(),
            ..AppConfig::default()
        };
        config_store.save(&user).unwrap();
        write_reference(&config_store.reference_path(), [0, 0, 255, 255]);

        let result = admin_store.restore_on_launch_with_replace(|source, destination| {
            if destination == config_store.config_path() {
                Err(io::Error::other("simulated config restore failure"))
            } else {
                atomic_replace(source, destination)
            }
        });
        assert!(result.is_err());
        assert_eq!(config_store.load().config, user);
        assert_eq!(
            reference_color(&config_store.reference_path()),
            [0, 0, 255, 255]
        );
    }
}
