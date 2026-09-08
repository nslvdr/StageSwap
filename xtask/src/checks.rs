use super::progress::{ReleaseProgress, ReleaseStage};
use super::{discover_test_count, run};
use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Runs the same gate for local validation and release publishing.
pub(super) fn run_checks(workspace: &Path, progress: &ReleaseProgress) -> Result<()> {
    for (label, args, tests) in [
        (
            "Check formatting",
            &["fmt", "--all", "--", "--check"][..],
            false,
        ),
        (
            "Run host Clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ][..],
            false,
        ),
        (
            "Run Windows-target Clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--target",
                "x86_64-pc-windows-msvc",
                "--",
                "-D",
                "warnings",
            ][..],
            false,
        ),
        (
            "Run workspace tests",
            &["test", "--workspace", "--all-targets"][..],
            true,
        ),
        (
            "Run release workspace tests",
            &["test", "--release", "--workspace", "--all-targets"][..],
            true,
        ),
    ] {
        let mut command = Command::new("cargo");
        command.current_dir(workspace).args(args);
        if tests {
            let total = (!args.contains(&"--release"))
                .then(|| discover_test_count(workspace))
                .flatten();
            progress.test_command(label, command, total)?;
        } else {
            progress.step(ReleaseStage::Checks, label, || run(command))?;
        }
    }
    Ok(())
}
