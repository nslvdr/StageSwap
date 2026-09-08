use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const UI_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SetupStep {
    HowItWorks,
    Webcam,
    Screen,
    Reference,
    Ready,
}

impl SetupStep {
    pub(crate) const ALL: [Self; 5] = [
        Self::HowItWorks,
        Self::Webcam,
        Self::Screen,
        Self::Reference,
        Self::Ready,
    ];

    pub(crate) const fn number(self) -> usize {
        match self {
            Self::HowItWorks => 1,
            Self::Webcam => 2,
            Self::Screen => 3,
            Self::Reference => 4,
            Self::Ready => 5,
        }
    }

    pub(crate) const fn from_number(number: usize) -> Option<Self> {
        match number {
            1 => Some(Self::HowItWorks),
            2 => Some(Self::Webcam),
            3 => Some(Self::Screen),
            4 => Some(Self::Reference),
            5 => Some(Self::Ready),
            _ => None,
        }
    }

    pub(crate) const fn previous(self) -> Option<Self> {
        Self::from_number(self.number().saturating_sub(1))
    }

    pub(crate) const fn next(self) -> Option<Self> {
        Self::from_number(self.number() + 1)
    }

    pub(crate) const fn is_last(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::HowItWorks => "JW Library to Zoom",
            Self::Webcam => "Choose your webcam",
            Self::Screen => "Choose the secondary screen",
            Self::Reference => "Capture reference image",
            Self::Ready => "Ready for the meeting",
        }
    }

    pub(crate) const fn explanation(self) -> &'static str {
        match self {
            Self::HowItWorks => {
                "Automatically switch what Zoom sees between the webcam and JW Library presentations."
            }
            Self::Webcam => {
                "Choose the webcam Zoom should see when JW Library is not playing media."
            }
            Self::Screen => "Choose the secondary screen JW Library uses for presentations.",
            Self::Reference => {
                "Set the reference image StageSwap uses to know when to show the JW Library screen in Zoom."
            }
            Self::Ready => "Review the setup status below, then start automatic switching.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SetupReturnView {
    Dashboard,
    Settings,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SetupSession {
    pub(crate) step: SetupStep,
    pub(crate) return_view: SetupReturnView,
    pub(crate) opened_at: Option<Instant>,
    pub(crate) step_changed_at: Option<Instant>,
    pub(crate) pending_step: Option<SetupStep>,
    pub(crate) transition_started_at: Option<Instant>,
}

impl SetupSession {
    pub(crate) fn live(return_view: SetupReturnView) -> Self {
        let now = Instant::now();
        Self {
            step: SetupStep::HowItWorks,
            return_view,
            opened_at: Some(now),
            step_changed_at: Some(now),
            pending_step: None,
            transition_started_at: None,
        }
    }

    #[cfg(any(not(windows), test))]
    pub(crate) const fn preview(step: SetupStep) -> Self {
        Self {
            step,
            return_view: SetupReturnView::Dashboard,
            opened_at: None,
            step_changed_at: None,
            pending_step: None,
            transition_started_at: None,
        }
    }

    pub(crate) fn go_to(&mut self, step: SetupStep) {
        if self.step != step {
            self.step = step;
            self.step_changed_at = Some(Instant::now());
            self.pending_step = None;
            self.transition_started_at = None;
        }
    }

    pub(crate) fn transition_to(&mut self, step: SetupStep, animations_enabled: bool) {
        if self.step == step {
            return;
        }
        if animations_enabled {
            self.pending_step = Some(step);
            self.transition_started_at = Some(Instant::now());
        } else {
            self.go_to(step);
        }
    }

    pub(crate) fn transition_opacity(&mut self, now: Instant, duration: Duration) -> f32 {
        let (Some(target), Some(started_at)) = (self.pending_step, self.transition_started_at)
        else {
            return 1.0;
        };
        let progress =
            now.saturating_duration_since(started_at).as_secs_f32() / duration.as_secs_f32();
        let progress = progress.clamp(0.0, 1.0);
        if progress < 0.5 {
            return 1.0 - progress * 2.0;
        }
        if self.step != target {
            self.step = target;
            self.step_changed_at = Some(now);
        }
        if progress >= 1.0 {
            self.pending_step = None;
            self.transition_started_at = None;
            1.0
        } else {
            (progress - 0.5) * 2.0
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SetupUiState {
    schema_version: u32,
    // Keep the serialized field name for compatibility with released builds.
    #[serde(rename = "tutorial_completed")]
    setup_guide_dismissed: bool,
}

impl Default for SetupUiState {
    fn default() -> Self {
        Self {
            schema_version: UI_STATE_SCHEMA_VERSION,
            setup_guide_dismissed: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SetupStartup {
    pub(crate) show_setup_guide: bool,
    pub(crate) warnings: Vec<String>,
}

impl SetupStartup {
    #[cfg(not(windows))]
    pub(crate) const fn suppressed() -> Self {
        Self {
            show_setup_guide: false,
            warnings: Vec::new(),
        }
    }
}

pub(crate) fn has_existing_user_data(directory: &Path) -> bool {
    fs::read_dir(directory).ok().is_some_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| name != "ui-state.json")
        })
    })
}

#[derive(Clone, Debug)]
pub(crate) struct SetupStateStore {
    path: PathBuf,
}

impl SetupStateStore {
    pub(crate) fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("ui-state.json"),
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn initialize(&self, existing_install: bool) -> SetupStartup {
        if self.path.exists() {
            match self.load() {
                Ok(state) => {
                    return SetupStartup {
                        show_setup_guide: !state.setup_guide_dismissed,
                        warnings: Vec::new(),
                    };
                }
                Err(error) => {
                    let state = SetupUiState {
                        setup_guide_dismissed: existing_install,
                        ..SetupUiState::default()
                    };
                    let mut warnings = vec![format!(
                        "Could not read setup guide progress; using a safe default: {error}"
                    )];
                    if let Err(save_error) = self.save(&state) {
                        warnings.push(format!(
                            "Could not repair setup guide progress: {save_error}"
                        ));
                    }
                    return SetupStartup {
                        show_setup_guide: !state.setup_guide_dismissed,
                        warnings,
                    };
                }
            }
        }

        let state = SetupUiState {
            setup_guide_dismissed: existing_install,
            ..SetupUiState::default()
        };
        let mut warnings = Vec::new();
        if let Err(error) = self.save(&state) {
            warnings.push(format!("Could not save setup guide progress: {error}"));
        }
        SetupStartup {
            show_setup_guide: !state.setup_guide_dismissed,
            warnings,
        }
    }

    pub(crate) fn mark_completed(&self) -> io::Result<()> {
        self.save(&SetupUiState {
            setup_guide_dismissed: true,
            ..SetupUiState::default()
        })
    }

    fn load(&self) -> io::Result<SetupUiState> {
        let json = fs::read_to_string(&self.path)?;
        let state = serde_json::from_str::<SetupUiState>(&json)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if state.schema_version != UI_STATE_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported setup guide UI state schema",
            ));
        }
        Ok(state)
    }

    fn save(&self, state: &SetupUiState) -> io::Result<()> {
        let Some(directory) = self.path.parent() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "setup guide UI state has no parent directory",
            ));
        };
        fs::create_dir_all(directory)?;
        let mut bytes = serde_json::to_vec_pretty(state)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        bytes.push(b'\n');
        fs::write(&self.path, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_setup_steps_have_stable_bidirectional_boundaries() {
        assert_eq!(SetupStep::HowItWorks.previous(), None);
        assert_eq!(SetupStep::Ready.next(), None);
        for (index, step) in SetupStep::ALL.into_iter().enumerate() {
            assert_eq!(step.number(), index + 1);
            assert_eq!(SetupStep::from_number(index + 1), Some(step));
        }
        assert_eq!(SetupStep::from_number(0), None);
        assert_eq!(SetupStep::from_number(6), None);
    }

    #[test]
    fn flow_page_transition_fades_out_switches_and_fades_in() {
        let mut session = SetupSession::preview(SetupStep::HowItWorks);
        session.transition_to(SetupStep::Webcam, true);
        let started_at = session.transition_started_at.unwrap();
        let duration = Duration::from_millis(150);

        assert!(
            (session.transition_opacity(started_at + Duration::from_millis(30), duration) - 0.6)
                .abs()
                < 0.001
        );
        assert_eq!(session.step, SetupStep::HowItWorks);
        assert_eq!(
            session.transition_opacity(started_at + Duration::from_millis(75), duration),
            0.0
        );
        assert_eq!(session.step, SetupStep::Webcam);
        assert_eq!(
            session.transition_opacity(started_at + Duration::from_millis(150), duration),
            1.0
        );
        assert!(session.pending_step.is_none());
    }

    #[test]
    fn flow_reduced_motion_changes_pages_immediately() {
        let mut session = SetupSession::preview(SetupStep::HowItWorks);
        session.transition_to(SetupStep::Webcam, false);
        assert_eq!(session.step, SetupStep::Webcam);
        assert!(session.pending_step.is_none());
    }

    #[test]
    fn flow_legacy_tutorial_completion_field_remains_compatible() {
        let state: SetupUiState =
            serde_json::from_str(r#"{"schema_version":1,"tutorial_completed":true}"#).unwrap();
        assert!(state.setup_guide_dismissed);
        let serialized = serde_json::to_string(&state).unwrap();
        assert!(serialized.contains(r#""tutorial_completed":true"#));
        assert!(!serialized.contains("setup_guide_dismissed"));
    }

    #[test]
    fn flow_setup_state_initialization_and_completion_respect_install_status() {
        let directory = tempfile::tempdir().unwrap();
        let store = SetupStateStore::new(directory.path());

        assert!(store.initialize(false).show_setup_guide);
        store.mark_completed().unwrap();
        assert!(!store.initialize(false).show_setup_guide);
        let config_path = directory.path().join("config.json");
        fs::write(&config_path, b"existing config").unwrap();

        assert!(!store.initialize(config_path.exists()).show_setup_guide);
        assert!(store.path().exists());
        assert_eq!(fs::read(config_path).unwrap(), b"existing config");
    }

    #[test]
    fn flow_any_retained_user_data_counts_as_an_existing_install() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!has_existing_user_data(directory.path()));
        fs::write(directory.path().join("ui-state.json"), "bad").unwrap();
        assert!(!has_existing_user_data(directory.path()));
        fs::create_dir(directory.path().join("logs")).unwrap();
        assert!(has_existing_user_data(directory.path()));
    }

    #[test]
    fn flow_setup_state_failures_use_safe_defaults_and_do_not_block_startup() {
        let existing = tempfile::tempdir().unwrap();
        let existing_store = SetupStateStore::new(existing.path());
        fs::write(existing_store.path(), "bad").unwrap();
        let existing_load = existing_store.initialize(true);
        assert!(!existing_load.show_setup_guide);
        assert!(!existing_load.warnings.is_empty());

        let fresh = tempfile::tempdir().unwrap();
        let fresh_store = SetupStateStore::new(fresh.path());
        fs::write(fresh_store.path(), "bad").unwrap();
        let fresh_load = fresh_store.initialize(false);
        assert!(fresh_load.show_setup_guide);
        assert!(!fresh_load.warnings.is_empty());
        let blocked = fresh.path().join("not-a-directory");
        fs::write(&blocked, "file").unwrap();
        let store = SetupStateStore::new(&blocked);

        let startup = store.initialize(false);
        assert!(startup.show_setup_guide);
        assert_eq!(startup.warnings.len(), 1);
        assert!(store.mark_completed().is_err());
    }
}
