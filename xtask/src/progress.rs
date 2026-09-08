//! Terminal rendering and prompts for release progress.
use super::run_test_command;
use anyhow::{Context, Result, bail};
use dialoguer::console::{Key, Term};
use std::cell::RefCell;
use std::env;
use std::process::Command;

const STAGE_COLUMN_WIDTH: usize = 12;
const RELEASE_TITLE: &str = "StageSwap release";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReleaseStage {
    Infos,
    Checks,
    Preparation,
    Build,
    Publish,
}

impl ReleaseStage {
    const ALL: [Self; 5] = [
        Self::Infos,
        Self::Checks,
        Self::Preparation,
        Self::Build,
        Self::Publish,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Infos => "Infos",
            Self::Checks => "Checks",
            Self::Preparation => "Preparation",
            Self::Build => "Build",
            Self::Publish => "Publish",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Infos => "ⓘ",
            Self::Checks => "✓",
            Self::Preparation => "⚙",
            Self::Build => "⚒",
            Self::Publish => "↗",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Infos => 0,
            Self::Checks => 1,
            Self::Preparation => 2,
            Self::Build => 3,
            Self::Publish => 4,
        }
    }

    fn substep_labels(self) -> &'static [&'static str] {
        match self {
            Self::Infos => &[
                "Check release environment",
                "Select release track",
                "Load release context",
                "Select version bump",
                "Check release target",
            ],
            Self::Checks => &[
                "Check formatting",
                "Run host Clippy",
                "Run Windows-target Clippy",
                "Run workspace tests",
                "Run release workspace tests",
            ],
            Self::Preparation => &["Prepare release version", "Prepare verification package"],
            Self::Build => &[
                "Build media-source DLL",
                "Build StageSwap executable",
                "Package verification build",
                "Validate verification package",
            ],
            Self::Publish => &[
                "Commit release version",
                "Package release (reuse verified build)",
                "Validate release package",
                "Publish release",
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StageStatus {
    Pending,
    Active,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubstepStatus {
    Pending,
    Active,
    Complete,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SubstepState {
    label: String,
    status: SubstepStatus,
    progress: Option<(usize, usize)>,
}

struct StageState {
    status: StageStatus,
    substeps: Vec<SubstepState>,
}

struct ReleaseProgressState {
    stages: [StageState; ReleaseStage::ALL.len()],
    current_stage: Option<ReleaseStage>,
    frame_lines: usize,
    prompt_lines: usize,
    prompt_active: bool,
}

impl ReleaseProgressState {
    fn new() -> Self {
        Self {
            stages: std::array::from_fn(|index| StageState {
                status: StageStatus::Pending,
                substeps: ReleaseStage::ALL[index]
                    .substep_labels()
                    .iter()
                    .map(|label| SubstepState {
                        label: (*label).to_owned(),
                        status: SubstepStatus::Pending,
                        progress: None,
                    })
                    .collect(),
            }),
            current_stage: None,
            frame_lines: 0,
            prompt_lines: 0,
            prompt_active: false,
        }
    }
}

pub(super) struct ReleaseProgress {
    interactive: bool,
    color: bool,
    terminal_renderer: bool,
    state: RefCell<ReleaseProgressState>,
}

impl ReleaseProgress {
    pub(super) fn new() -> Self {
        let interactive = Term::stderr().is_term();
        let color = interactive && env::var_os("NO_COLOR").is_none();
        let progress = Self {
            interactive,
            color,
            terminal_renderer: interactive,
            state: RefCell::new(ReleaseProgressState::new()),
        };
        if interactive {
            progress.refresh();
        } else {
            eprintln!("{RELEASE_TITLE}");
        }
        progress
    }

    /// Local checks use plain progress without release prompts or terminal animation.
    pub(super) fn for_checks() -> Self {
        Self {
            interactive: false,
            color: false,
            terminal_renderer: false,
            state: RefCell::new(ReleaseProgressState::new()),
        }
    }

    pub(super) fn step<T>(
        &self,
        stage: ReleaseStage,
        label: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.begin_substep(stage, label, None);
        match action() {
            Ok(value) => {
                self.finish_substep(true);
                Ok(value)
            }
            Err(error) => {
                self.finish_substep(false);
                Err(error.context(format!("{label} failed")))
            }
        }
    }

    pub(super) fn test_command(
        &self,
        label: &str,
        command: Command,
        total: Option<usize>,
    ) -> Result<()> {
        self.begin_substep(ReleaseStage::Checks, label, total.map(|total| (0, total)));
        match run_test_command(command, self, total) {
            Ok(()) => {
                if let Some(total) = total {
                    self.set_substep_progress(total, total);
                }
                self.finish_substep(true);
                Ok(())
            }
            Err(error) => {
                self.finish_substep(false);
                Err(error.context(format!("{label} failed")))
            }
        }
    }

    fn begin_substep(&self, stage: ReleaseStage, label: &str, progress: Option<(usize, usize)>) {
        {
            let mut state = self.state.borrow_mut();
            if state.current_stage != Some(stage) {
                if let Some(previous) = state.current_stage {
                    let previous_state = &mut state.stages[previous.index()];
                    if previous_state.status == StageStatus::Active {
                        previous_state.status = StageStatus::Complete;
                        for substep in &mut previous_state.substeps {
                            match substep.status {
                                SubstepStatus::Active => substep.status = SubstepStatus::Complete,
                                SubstepStatus::Pending => substep.status = SubstepStatus::Skipped,
                                SubstepStatus::Complete
                                | SubstepStatus::Skipped
                                | SubstepStatus::Failed => {}
                            }
                        }
                    }
                }
                state.current_stage = Some(stage);
                state.stages[stage.index()].status = StageStatus::Active;
            }
            let substeps = &mut state.stages[stage.index()].substeps;
            if let Some(substep) = substeps.iter_mut().find(|substep| substep.label == label) {
                substep.status = SubstepStatus::Active;
                substep.progress = progress;
            } else {
                substeps.push(SubstepState {
                    label: label.to_owned(),
                    status: SubstepStatus::Active,
                    progress,
                });
            }
        }
        self.refresh();
    }

    pub(super) fn set_substep_progress(&self, completed: usize, total: usize) {
        {
            let mut state = self.state.borrow_mut();
            if let Some(stage) = state.current_stage
                && let Some(substep) = state.stages[stage.index()]
                    .substeps
                    .iter_mut()
                    .rev()
                    .find(|substep| substep.status == SubstepStatus::Active)
            {
                substep.progress = Some((completed, total));
            }
        }
        self.refresh();
    }

    fn finish_substep(&self, success: bool) {
        let plain_line = {
            let mut state = self.state.borrow_mut();
            let Some(stage) = state.current_stage else {
                return;
            };
            let stage_state = &mut state.stages[stage.index()];
            if let Some(substep) = stage_state
                .substeps
                .iter_mut()
                .rev()
                .find(|substep| substep.status == SubstepStatus::Active)
            {
                substep.status = if success {
                    SubstepStatus::Complete
                } else {
                    SubstepStatus::Failed
                };
            }
            if !success {
                stage_state.status = StageStatus::Failed;
            }
            (!self.interactive).then(|| render_stage_line(stage, stage_state, false))
        };
        self.refresh();
        if let Some(line) = plain_line {
            eprintln!("{line}");
        }
    }

    pub(super) fn complete_current_stage(&self) {
        {
            let mut state = self.state.borrow_mut();
            if let Some(stage) = state.current_stage
                && state.stages[stage.index()].status == StageStatus::Active
            {
                state.stages[stage.index()].status = StageStatus::Complete;
                for substep in &mut state.stages[stage.index()].substeps {
                    match substep.status {
                        SubstepStatus::Active => substep.status = SubstepStatus::Complete,
                        SubstepStatus::Pending => substep.status = SubstepStatus::Skipped,
                        SubstepStatus::Complete
                        | SubstepStatus::Skipped
                        | SubstepStatus::Failed => {}
                    }
                }
            }
        }
        self.refresh();
    }

    pub(super) fn fail_current_stage(&self) {
        {
            let mut state = self.state.borrow_mut();
            if let Some(stage) = state.current_stage {
                state.stages[stage.index()].status = StageStatus::Failed;
            }
        }
        self.refresh();
    }

    fn refresh(&self) {
        if !self.interactive {
            return;
        }
        let mut state = self.state.borrow_mut();
        if !state.prompt_active {
            render_frame_locked(&mut state, self.color);
        }
    }

    fn begin_prompt(&self) {
        if !self.interactive {
            return;
        }
        let mut state = self.state.borrow_mut();
        let term = Term::stderr();
        let separator = "────────────────────────────────────────────────────────";
        let separator = if self.color {
            term.style()
                .dim()
                .apply_to(separator)
                .force_styling(true)
                .to_string()
        } else {
            separator.to_owned()
        };
        let _ = term.write_line(&separator);
        let _ = term.flush();
        state.prompt_active = true;
        state.prompt_lines = 1;
    }

    fn replace_prompt(&self, lines: &[String]) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let term = Term::stderr();
        let prompt_lines = state.prompt_lines.saturating_sub(1);
        if prompt_lines > 0 {
            term.clear_last_lines(prompt_lines)
                .context("clear release prompt")?;
        }
        for line in lines {
            term.write_line(line).context("write release prompt")?;
        }
        term.flush().context("flush release prompt")?;
        state.prompt_lines = 1 + lines.len();
        Ok(())
    }

    fn finish_prompt(&self) -> Result<()> {
        if !self.interactive {
            return Ok(());
        }
        let mut state = self.state.borrow_mut();
        let term = Term::stderr();
        let lines = state.frame_lines + state.prompt_lines;
        if lines > 0 {
            term.clear_last_lines(lines)
                .context("clear release prompt frame")?;
        }
        state.frame_lines = 0;
        state.prompt_lines = 0;
        state.prompt_active = false;
        render_frame_locked(&mut state, self.color);
        Ok(())
    }

    pub(super) fn select_prompt_interactive<T: ToString>(
        &self,
        message: &str,
        items: &[T],
    ) -> Result<usize> {
        self.begin_prompt();
        let term = Term::stderr();
        term.hide_cursor().context("hide release prompt cursor")?;
        let result = (|| {
            let mut selection = 0;
            loop {
                let choices = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let line = format!(
                            "{}{}",
                            if index == selection { "→ " } else { "  " },
                            item.to_string()
                        );
                        if self.color && index == selection {
                            Term::stderr()
                                .style()
                                .yellow()
                                .bold()
                                .apply_to(line)
                                .force_styling(true)
                                .to_string()
                        } else {
                            line
                        }
                    })
                    .collect::<Vec<_>>();
                let separator = if message.ends_with('?') { " " } else { ": " };
                self.replace_prompt(&[format!("{message}{separator}{}", choices.join("   "))])?;
                match term.read_key().context("read release selection")? {
                    Key::ArrowRight => {
                        selection = (selection + 1) % items.len();
                    }
                    Key::ArrowLeft => {
                        selection = selection.checked_sub(1).unwrap_or(items.len() - 1);
                    }
                    Key::Enter | Key::Char(' ') => break,
                    Key::Escape | Key::Char('q') => bail!("release selection cancelled"),
                    Key::CtrlC => bail!("release selection interrupted"),
                    _ => {}
                }
            }
            Ok(selection)
        })();
        term.show_cursor().context("show release prompt cursor")?;
        self.finish_prompt()?;
        result
    }

    pub(super) fn edit_prompt_interactive(&self, message: &str, initial: &str) -> Result<String> {
        self.begin_prompt();
        let term = Term::stderr();
        term.hide_cursor().context("hide release prompt cursor")?;
        let result = (|| {
            let mut chars = initial.chars().collect::<Vec<_>>();
            let mut position = chars.len();
            loop {
                let value = chars.iter().collect::<String>();
                let line = format!("{message}{value}");
                self.replace_prompt(&[line])?;
                let left = value.chars().count().saturating_sub(position);
                if left > 0 {
                    term.move_cursor_left(left)
                        .context("move release version cursor")?;
                }
                term.flush().context("flush release version prompt")?;
                let old_position = position;
                let old_length = chars.len();
                match term.read_key().context("read release version")? {
                    Key::Enter => {
                        term.write_str("\n")
                            .context("finish release version prompt")?;
                        break Ok(value);
                    }
                    Key::ArrowLeft if position > 0 => position -= 1,
                    Key::ArrowRight if position < chars.len() => position += 1,
                    Key::Home => position = 0,
                    Key::End => position = chars.len(),
                    Key::Backspace if position > 0 => {
                        position -= 1;
                        chars.remove(position);
                    }
                    Key::Del if position < chars.len() => {
                        chars.remove(position);
                    }
                    Key::Char(chr) if !chr.is_ascii_control() => {
                        chars.insert(position, chr);
                        position += 1;
                    }
                    Key::Escape => break Err(anyhow::anyhow!("release version edit cancelled")),
                    Key::CtrlC => break Err(anyhow::anyhow!("release version edit interrupted")),
                    _ => {}
                }
                term.move_cursor_right(old_length.saturating_sub(old_position))
                    .context("move release version cursor to line end")?;
            }
        })();
        term.show_cursor().context("show release prompt cursor")?;
        self.finish_prompt()?;
        result
    }
}

impl Drop for ReleaseProgress {
    fn drop(&mut self) {
        if self.terminal_renderer {
            let _ = Term::stderr().show_cursor();
        }
    }
}

fn render_frame_locked(state: &mut ReleaseProgressState, color: bool) {
    let term = Term::stderr();
    if state.frame_lines > 0 && term.clear_last_lines(state.frame_lines).is_err() {
        return;
    }
    let title = if color {
        Term::stderr()
            .style()
            .cyan()
            .bold()
            .apply_to(RELEASE_TITLE)
            .force_styling(true)
            .to_string()
    } else {
        RELEASE_TITLE.to_owned()
    };
    if term.write_line(&title).is_err() {
        return;
    }
    for stage in ReleaseStage::ALL {
        let stage_state = &state.stages[stage.index()];
        let line = render_stage_line(stage, stage_state, color);
        if term.write_line(&line).is_err() {
            return;
        }
    }
    let _ = term.flush();
    state.frame_lines = 1 + ReleaseStage::ALL.len();
}

fn render_stage_line(stage: ReleaseStage, stage_state: &StageState, color: bool) -> String {
    let stage_text = format!(
        "{} {:<width$}:",
        stage.icon(),
        stage.label(),
        width = STAGE_COLUMN_WIDTH
    );
    let children = stage_state
        .substeps
        .iter()
        .map(|substep| {
            let marker = match substep.status {
                SubstepStatus::Pending | SubstepStatus::Active | SubstepStatus::Skipped => "",
                SubstepStatus::Complete => "",
                SubstepStatus::Failed => "✖ ",
            };
            let label = short_substep_label(&substep.label);
            let label = match substep.progress {
                Some((completed, total)) => format!("{label} {completed}/{total}"),
                None => label.to_owned(),
            };
            format!("{marker}{label}")
        })
        .collect::<Vec<_>>();

    if !color {
        return format!("{stage_text}  {}", children.join(" → "));
    }

    let stage_label = match stage_state.status {
        StageStatus::Active => style_active_stage(stage, stage_text),
        StageStatus::Pending | StageStatus::Complete | StageStatus::Failed => {
            style_stage_category(stage, stage_text)
        }
    };
    let children = stage_state
        .substeps
        .iter()
        .zip(children)
        .map(|(substep, text)| style_substep(stage, substep.status, text))
        .collect::<Vec<_>>();
    format!("{stage_label}  {}", children.join(" → "))
}

fn style_active_stage(stage: ReleaseStage, text: String) -> String {
    let styled = match stage {
        ReleaseStage::Infos => Term::stderr().style().blue().bold().apply_to(text),
        ReleaseStage::Checks => Term::stderr().style().magenta().bold().apply_to(text),
        ReleaseStage::Preparation => Term::stderr().style().yellow().bold().apply_to(text),
        ReleaseStage::Build => Term::stderr().style().cyan().bold().apply_to(text),
        ReleaseStage::Publish => Term::stderr().style().green().bold().apply_to(text),
    };
    styled.force_styling(true).to_string()
}

fn style_stage_category(stage: ReleaseStage, text: String) -> String {
    let styled = match stage {
        ReleaseStage::Infos => Term::stderr().style().blue().apply_to(text),
        ReleaseStage::Checks => Term::stderr().style().magenta().apply_to(text),
        ReleaseStage::Preparation => Term::stderr().style().yellow().apply_to(text),
        ReleaseStage::Build => Term::stderr().style().cyan().apply_to(text),
        ReleaseStage::Publish => Term::stderr().style().green().apply_to(text),
    };
    styled.force_styling(true).to_string()
}

fn style_substep(stage: ReleaseStage, status: SubstepStatus, text: String) -> String {
    let styled = match status {
        SubstepStatus::Active => return style_active_stage(stage, text),
        SubstepStatus::Complete | SubstepStatus::Skipped => {
            Term::stderr().style().green().dim().apply_to(text)
        }
        SubstepStatus::Failed => Term::stderr().style().red().bold().apply_to(text),
        SubstepStatus::Pending => Term::stderr().style().dim().apply_to(text),
    };
    styled.force_styling(true).to_string()
}

fn short_substep_label(label: &str) -> &str {
    match label {
        "Check release environment" => "Environment",
        "Load release context" => "Context",
        "Check release target" => "Target",
        "Select release track" => "Track",
        "Select version bump" => "Bump",
        "Prepare release version" => "Version",
        "Prepare verification package" => "Verification",
        "Build media-source DLL" => "Media DLL",
        "Build StageSwap executable" => "App",
        "Package verification build" | "Package release (reuse verified build)" => "Package",
        "Validate verification package" | "Validate release package" => "Verify",
        "Commit release version" => "Commit",
        "Publish release" => "Publish",
        "Check formatting" => "Format",
        "Run host Clippy" => "Clippy",
        "Run Windows-target Clippy" => "Win Clippy",
        "Run workspace tests" => "Tests",
        "Run release workspace tests" => "Release tests",
        _ => label,
    }
}
