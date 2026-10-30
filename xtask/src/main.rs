mod checks;
mod progress;
mod validation;

use anyhow::{Context, Result, bail};
use checks::run_checks;
use dialoguer::console::Term;
use progress::{ReleaseProgress, ReleaseStage};
use stageswap_core::{hex_digest as hex, release::ReleaseVersion, sha256};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use validation::{
    validate_embedded_payload, validate_pe, validate_pe_path, validate_release_outputs,
};

const REQUIRED_WINDOWS_SDK: &str = "10.0.22621.0";
const APP_PACKAGE: &str = "stageswap";
const MEDIA_SOURCE_PACKAGE: &str = "stageswap-media-source";
const APP_EXECUTABLE: &str = "StageSwap.exe";
const MEDIA_SOURCE_DLL: &str = "stageswap_media_source.dll";
const RELEASE_PREFIX: &str = "StageSwap_win64_v";

fn main() -> Result<()> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("validate-pe") => validate_pe(
            arguments.next().context("missing PE path")?,
            arguments.next().context("missing architecture")?,
        ),
        Some("check") => {
            if arguments.next().is_some() {
                bail!("check does not accept positional arguments");
            }
            run_checks(&workspace_root(), &ReleaseProgress::for_checks())
        }
        Some("publish-release") => {
            if arguments.next().is_some() {
                bail!("publish-release does not accept positional arguments");
            }
            publish_release_cli()
        }
        Some(command) => bail!("unknown xtask command: {command}"),
        None => bail!("usage: cargo xtask <check | validate-pe PATH x64 | publish-release>"),
    }
}

fn build_release(release_version: ReleaseVersion, progress: &ReleaseProgress) -> Result<PathBuf> {
    let workspace = workspace_root();
    let _windows_sdk = selected_windows_sdk()?;
    let architecture = "x64";
    let target = "x86_64-pc-windows-msvc";
    let manifest = workspace.join("Cargo.toml");
    let application_version = read_workspace_version(&manifest)?;
    if application_version != release_version {
        bail!(
            "workspace version {application_version} does not match release version {release_version}"
        );
    }
    let (_, executable) = build_release_pair(&workspace, target, architecture, progress)?;
    Ok(executable)
}

fn package_release(
    executable: &Path,
    release_version: ReleaseVersion,
    output: &Path,
    workspace: &Path,
) -> Result<PathBuf> {
    let windows_sdk = selected_windows_sdk()?;
    let architecture = "x64";
    let executable_bytes =
        fs::read(executable).with_context(|| format!("could not read {}", executable.display()))?;
    let digest = sha256(&executable_bytes);
    let artifact = format!("{RELEASE_PREFIX}{release_version}.exe");
    let revision = capture(git(workspace, &["rev-parse", "HEAD"]))?;
    let checksum = format!(
        "# applicationVersion={release_version}\n# releaseVersion={release_version}\n# sourceRevision={}\n# architecture={architecture}\n# configuration=Release\n# windowsSdk={windows_sdk}\n{} *{artifact}\n",
        revision.trim(),
        hex(&digest)
    );
    publish_release(output, &artifact, &executable_bytes, checksum.as_bytes())
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is inside the workspace")
        .to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleaseTrack {
    Development,
    Release,
}

fn publish_release_cli() -> Result<()> {
    let progress = ReleaseProgress::new();
    let workspace = workspace_root();
    let branch = progress.step(ReleaseStage::Infos, "Check release environment", || {
        preflight_repository(&workspace)?;
        let mut github_auth = Command::new("gh");
        github_auth.args(["auth", "status"]);
        run(github_auth)?;
        let branch = capture(git(&workspace, &["branch", "--show-current"]))?;
        let branch = branch.trim().to_owned();
        if branch.is_empty() {
            bail!("publish-release requires a checked-out branch");
        }
        Ok(branch)
    })?;

    let track = prompt_track(&progress)?;
    if track == ReleaseTrack::Release && branch != "main" {
        bail!("stable releases must be published from main");
    }

    let (published, current) =
        progress.step(ReleaseStage::Infos, "Load release context", || {
            let published = published_versions()?;
            let current = read_workspace_version(&workspace.join("Cargo.toml"))?;
            Ok((published, current))
        })?;
    let highest = published.iter().copied().max();
    let version_suggestions = VersionSuggestions::new(current, highest)?;
    let version = prompt_version(&progress, version_suggestions, track, highest)?;
    let tag = format!("v{version}");
    let incomplete_draft = progress.step(ReleaseStage::Infos, "Check release target", || {
        let incomplete_draft = incomplete_draft_exists(&tag)?;
        if !incomplete_draft {
            ensure_tag_absent(&workspace, &tag)?;
        }
        Ok(incomplete_draft)
    })?;

    run_checks(&workspace, &progress)?;

    let manifest = workspace.join("Cargo.toml");
    let lockfile = workspace.join("Cargo.lock");
    let mut version_transaction = None;
    let outcome = (|| -> Result<PathBuf> {
        version_transaction = if version != current {
            Some(
                progress.step(ReleaseStage::Preparation, "Prepare release version", || {
                    let transaction = VersionTransaction::begin(&manifest, &lockfile, version)?;
                    run(cargo_metadata(&workspace))?;
                    Ok(transaction)
                })?,
            )
        } else {
            None
        };
        let verification = progress.step(
            ReleaseStage::Preparation,
            "Prepare verification package",
            || tempfile::tempdir().context("create release verification directory"),
        )?;
        let executable = build_release(version, &progress)?;
        let verification_artifact =
            progress.step(ReleaseStage::Build, "Package verification build", || {
                package_release(&executable, version, verification.path(), &workspace)
            })?;
        progress.step(ReleaseStage::Build, "Validate verification package", || {
            validate_release_outputs(&verification_artifact, version)
        })?;

        let message = format!("Release {tag}");
        progress.step(ReleaseStage::Publish, "Commit release version", || {
            run(git(&workspace, &["add", "Cargo.toml", "Cargo.lock"]))?;
            run(git(
                &workspace,
                &["commit", "--allow-empty", "-m", &message],
            ))
        })?;
        if let Some(transaction) = version_transaction.as_mut() {
            transaction.commit();
        }
        let output = workspace.join("dist");
        let artifact = progress.step(
            ReleaseStage::Publish,
            "Package release (reuse verified build)",
            || {
                clear_local_release_outputs(&output, version)?;
                package_release(&executable, version, &output, &workspace)
            },
        )?;
        progress.step(ReleaseStage::Publish, "Validate release package", || {
            validate_release_outputs(&artifact, version)
        })?;
        progress.step(ReleaseStage::Publish, "Publish release", || {
            run(git(&workspace, &["push", "origin", &branch]))?;
            let revision = capture(git(&workspace, &["rev-parse", "HEAD"]))?;
            if incomplete_draft {
                cleanup_incomplete_draft(&tag)?;
            }
            publish_github_release(track, version, revision.trim(), &artifact, &workspace)
        })?;
        Ok(artifact)
    })();

    match outcome {
        Ok(artifact) => {
            progress.complete_current_stage();
            let message = format!("🎉 Release {tag} published successfully!");
            if Term::stdout().is_term() && env::var_os("NO_COLOR").is_none() {
                println!(
                    "{}",
                    Term::stdout()
                        .style()
                        .green()
                        .bold()
                        .apply_to(message)
                        .force_styling(true)
                );
            } else {
                println!("{message}");
            }
            println!("  {}", artifact.display());
            Ok(())
        }
        Err(error) => {
            progress.fail_current_stage();
            if let Some(transaction) = version_transaction.as_mut()
                && let Err(rollback_error) = transaction.rollback()
            {
                return Err(error.context(format!(
                    "release also failed to restore the workspace version: {rollback_error:#}"
                )));
            }
            Err(error)
        }
    }
}

fn preflight_repository(workspace: &Path) -> Result<()> {
    let status = capture(git(
        workspace,
        &["status", "--porcelain", "--untracked-files=all"],
    ))?;
    if !status.trim().is_empty() {
        bail!("publish-release requires a clean worktree");
    }
    let remote = capture(git(workspace, &["remote", "get-url", "origin"]))?;
    if !remote.contains("nslvdr/StageSwap") {
        bail!("origin does not point to nslvdr/StageSwap");
    }
    run(git(workspace, &["fetch", "origin"]))?;
    let head = capture(git(workspace, &["rev-parse", "HEAD"]))?;
    let upstream = capture(git(workspace, &["rev-parse", "@{upstream}"]))?;
    if head.trim() != upstream.trim() {
        bail!("the current branch must exactly match its pushed upstream before publishing");
    }
    Ok(())
}

fn prompt_track(progress: &ReleaseProgress) -> Result<ReleaseTrack> {
    let selection = progress.step(ReleaseStage::Infos, "Select release track", || {
        if Term::stderr().is_term() {
            progress
                .select_prompt_interactive("Release track", &["Development (default)", "Release"])
        } else {
            eprintln!("Release track:");
            eprintln!("  1. Development (default)");
            eprintln!("  2. Release");
            match read_prompt("Select [1]: ")?.as_str() {
                "" | "1" => Ok(0),
                "2" => Ok(1),
                _ => bail!("release track must be 1 or 2"),
            }
        }
    })?;
    match selection {
        0 => Ok(ReleaseTrack::Development),
        1 => Ok(ReleaseTrack::Release),
        _ => bail!("release track selection was out of range"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VersionSuggestions {
    patch: ReleaseVersion,
    minor: ReleaseVersion,
    major: ReleaseVersion,
}

impl VersionSuggestions {
    fn new(current: ReleaseVersion, highest: Option<ReleaseVersion>) -> Result<Self> {
        let base = highest
            .filter(|highest| *highest > current)
            .unwrap_or(current);
        Ok(Self {
            patch: base.increment_patch()?,
            minor: base.increment_minor()?,
            major: base.increment_major()?,
        })
    }
}

fn prompt_version(
    progress: &ReleaseProgress,
    suggestions: VersionSuggestions,
    track: ReleaseTrack,
    highest: Option<ReleaseVersion>,
) -> Result<ReleaseVersion> {
    let items = [
        format!("Patch ({})", suggestions.patch),
        format!("Minor ({})", suggestions.minor),
        format!("Major ({})", suggestions.major),
        format!("Manual (edit {})", suggestions.patch),
    ];
    progress.step(ReleaseStage::Infos, "Select version bump", || {
        let selection = if Term::stderr().is_term() {
            progress.select_prompt_interactive("Version bump", &items)
        } else {
            eprintln!("Version bump:");
            for (index, item) in items.iter().enumerate() {
                eprintln!("  {}. {item}", index + 1);
            }
            match read_prompt("Select [1]: ")?.as_str() {
                "" | "1" => Ok(0),
                "2" => Ok(1),
                "3" => Ok(2),
                "4" => Ok(3),
                _ => bail!("version bump must be 1, 2, 3, or 4"),
            }
        }?;
        let version = match selection {
            0 => suggestions.patch,
            1 => suggestions.minor,
            2 => suggestions.major,
            3 => prompt_manual_version(progress, suggestions.patch)?,
            _ => bail!("version bump selection was out of range"),
        };
        if highest.is_some_and(|highest| version <= highest) {
            bail!("release version {version} must be newer than every published version");
        }
        if track == ReleaseTrack::Release
            && !confirm_prompt(
                progress,
                &format!("Publish v{version} as a stable release?"),
            )?
        {
            bail!("stable release confirmation declined");
        }
        Ok(version)
    })
}

fn prompt_manual_version(
    progress: &ReleaseProgress,
    suggested: ReleaseVersion,
) -> Result<ReleaseVersion> {
    let value = if Term::stderr().is_term() {
        progress.edit_prompt_interactive("Version: ", &suggested.to_string())?
    } else {
        read_prompt(&format!("Version [{suggested}]: "))?
    };
    parse_version_input(&value, suggested)
}

fn confirm_prompt(progress: &ReleaseProgress, message: &str) -> Result<bool> {
    let selection = if Term::stderr().is_term() {
        progress.select_prompt_interactive(message, &["No", "Yes"])?
    } else {
        eprintln!("{message} [y/N]");
        match read_prompt("Confirm: ")?.to_ascii_lowercase().as_str() {
            "" | "n" | "no" => 0,
            "y" | "yes" => 1,
            _ => bail!("confirmation must be yes or no"),
        }
    };
    Ok(selection == 1)
}

fn read_prompt(message: &str) -> Result<String> {
    eprint!("{message}");
    io::stderr().flush().context("flush prompt")?;
    let mut value = String::new();
    io::stdin().read_line(&mut value).context("read prompt")?;
    eprintln!();
    Ok(value.trim().to_owned())
}

fn parse_version_input(value: &str, suggested: ReleaseVersion) -> Result<ReleaseVersion> {
    if value.trim().is_empty() {
        Ok(suggested)
    } else {
        Ok(ReleaseVersion::parse(value.trim())?)
    }
}

fn published_versions() -> Result<Vec<ReleaseVersion>> {
    let output = capture({
        let mut command = Command::new("gh");
        command.args([
            "release",
            "list",
            "--repo",
            "nslvdr/StageSwap",
            "--limit",
            "100",
            "--json",
            "tagName,isDraft",
            "--jq",
            ".[] | select(.isDraft == false) | .tagName",
        ]);
        command
    })?;
    Ok(output
        .lines()
        .filter_map(|tag| {
            let version = ReleaseVersion::parse(tag).ok()?;
            (tag == format!("v{version}")).then_some(version)
        })
        .collect())
}

fn incomplete_draft_exists(tag: &str) -> Result<bool> {
    let mut inspect = Command::new("gh");
    inspect.args([
        "release",
        "view",
        tag,
        "--repo",
        "nslvdr/StageSwap",
        "--json",
        "isDraft",
        "--jq",
        ".isDraft",
    ]);
    let output = inspect
        .output()
        .with_context(|| format!("could not inspect GitHub release {tag}"))?;
    if output.status.success() {
        let is_draft =
            String::from_utf8(output.stdout).context("GitHub release state was not valid UTF-8")?;
        if is_draft.trim() != "true" {
            bail!("release {tag} already exists");
        }
        Ok(true)
    } else {
        let diagnostics = command_diagnostics(&output.stdout, &output.stderr);
        let diagnostics_lower = diagnostics.to_ascii_lowercase();
        if !diagnostics_lower.contains("release not found")
            && !diagnostics_lower.contains("not found")
        {
            bail!("could not inspect GitHub release {tag}{diagnostics}");
        }
        Ok(false)
    }
}

fn cleanup_incomplete_draft(tag: &str) -> Result<()> {
    let mut delete = Command::new("gh");
    delete.args([
        "release",
        "delete",
        tag,
        "--repo",
        "nslvdr/StageSwap",
        "--yes",
        "--cleanup-tag",
    ]);
    run(delete)
}

fn ensure_tag_absent(workspace: &Path, tag: &str) -> Result<()> {
    let reference = format!("refs/tags/{tag}");
    let output = capture(git(
        workspace,
        &["ls-remote", "--tags", "origin", &reference],
    ))?;
    if !output.trim().is_empty() {
        bail!("tag {tag} already exists");
    }
    Ok(())
}

fn clear_local_release_outputs(output: &Path, version: ReleaseVersion) -> Result<()> {
    let executable = output.join(format!("{RELEASE_PREFIX}{version}.exe"));
    let checksum = output.join(format!("{RELEASE_PREFIX}{version}.exe.sha256"));
    for path in [executable, checksum] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("remove stale release output {}", path.display()));
            }
        }
    }
    Ok(())
}

fn publish_github_release(
    track: ReleaseTrack,
    version: ReleaseVersion,
    revision: &str,
    artifact: &Path,
    workspace: &Path,
) -> Result<()> {
    let tag = format!("v{version}");
    let title = match track {
        ReleaseTrack::Development => format!("{tag} beta"),
        ReleaseTrack::Release => tag.clone(),
    };
    let checksum = artifact.with_file_name(format!("{}{}.exe.sha256", RELEASE_PREFIX, version));
    let mut command = Command::new("gh");
    command.current_dir(workspace).args([
        "release",
        "create",
        &tag,
        artifact.to_str().context("artifact path is not UTF-8")?,
        checksum.to_str().context("checksum path is not UTF-8")?,
        "--repo",
        "nslvdr/StageSwap",
        "--target",
        revision,
        "--title",
        &title,
        "--generate-notes",
    ]);
    match track {
        ReleaseTrack::Development => {
            command.args(["--prerelease", "--latest=false"]);
        }
        ReleaseTrack::Release => {
            command.arg("--latest");
        }
    }
    run(command)
}

fn git(workspace: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(workspace).args(arguments);
    command
}

fn capture(mut command: Command) -> Result<String> {
    let description = format!("{command:?}");
    let output = command
        .output()
        .with_context(|| format!("could not run {description}"))?;
    if !output.status.success() {
        bail!(
            "command failed ({}): {description}{}",
            output.status,
            command_diagnostics(&output.stdout, &output.stderr)
        );
    }
    String::from_utf8(output.stdout).context("command output was not valid UTF-8")
}

fn command_diagnostics(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let mut diagnostics = String::new();
    if !stdout.trim().is_empty() {
        diagnostics.push_str("\nstdout:\n");
        diagnostics.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        diagnostics.push_str("\nstderr:\n");
        diagnostics.push_str(stderr.trim());
    }
    diagnostics
}

fn discover_test_count(workspace: &Path) -> Option<usize> {
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        .args(["test", "--workspace", "--all-targets", "--", "--list"]);
    capture(command)
        .ok()
        .and_then(|output| parse_test_list_count(&output))
}

fn parse_test_list_count(output: &str) -> Option<usize> {
    let mut total = 0;
    let mut found_summary = false;
    for line in output.lines() {
        let mut words = line.split_whitespace();
        let Some(count) = words.next().and_then(|value| value.parse::<usize>().ok()) else {
            continue;
        };
        let Some(kind) = words.next() else {
            continue;
        };
        if matches!(kind.trim_end_matches(','), "test" | "tests") && line.contains("benchmark") {
            total += count;
            found_summary = true;
        }
    }
    found_summary.then_some(total)
}

#[derive(Clone, Copy)]
enum ChildStream {
    Stdout,
    Stderr,
}

struct ChildLine {
    stream: ChildStream,
    bytes: Vec<u8>,
}

fn run_test_command(
    mut command: Command,
    progress: &ReleaseProgress,
    total: Option<usize>,
) -> Result<()> {
    let description = format!("{command:?}");
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not run {description}"))?;
    let stdout = child.stdout.take().context("capture test stdout")?;
    let stderr = child.stderr.take().context("capture test stderr")?;
    let (sender, receiver) = mpsc::channel();
    let stdout_thread = spawn_child_reader(stdout, ChildStream::Stdout, sender.clone());
    let stderr_thread = spawn_child_reader(stderr, ChildStream::Stderr, sender);
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let mut completed = 0;
    for line in receiver {
        match line.stream {
            ChildStream::Stdout => {
                if parse_test_completion(&line.bytes) {
                    completed += 1;
                    if let Some(total) = total {
                        progress.set_substep_progress(completed.min(total), total);
                    }
                }
                stdout_bytes.extend_from_slice(&line.bytes);
            }
            ChildStream::Stderr => stderr_bytes.extend_from_slice(&line.bytes),
        }
    }
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    let status = child
        .wait()
        .with_context(|| format!("wait for {description}"))?;
    if !status.success() {
        bail!(
            "command failed ({}): {description}{}",
            status,
            command_diagnostics(&stdout_bytes, &stderr_bytes)
        );
    }
    Ok(())
}

fn spawn_child_reader<R>(
    reader: R,
    stream: ChildStream,
    sender: mpsc::Sender<ChildLine>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if sender
                        .send(ChildLine {
                            stream,
                            bytes: bytes.clone(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

fn parse_test_completion(bytes: &[u8]) -> bool {
    let line = String::from_utf8_lossy(bytes);
    let line = line.trim();
    let Some(line) = line.strip_prefix("test ") else {
        return false;
    };
    let Some((_, status)) = line.rsplit_once(" ... ") else {
        return false;
    };
    matches!(status.trim(), "ok" | "FAILED" | "ignored")
}

fn build_release_pair(
    workspace: &Path,
    target: &str,
    architecture: &str,
    progress: &ReleaseProgress,
) -> Result<(PathBuf, PathBuf)> {
    progress.step(ReleaseStage::Build, "Build media-source DLL", || {
        run(cargo_build(workspace, MEDIA_SOURCE_PACKAGE, target, None))
    })?;
    let dll = release_artifact(workspace, target, MEDIA_SOURCE_DLL);
    if !dll.is_file() {
        bail!("media-source build did not produce {}", dll.display());
    }
    validate_pe_path(&dll, architecture)?;
    let embedded_dll = dll
        .canonicalize()
        .context("canonicalize media-source DLL")?;
    progress.step(ReleaseStage::Build, "Build StageSwap executable", || {
        run(cargo_build(
            workspace,
            APP_PACKAGE,
            target,
            Some(&embedded_dll),
        ))
    })?;
    let executable = release_artifact(workspace, target, APP_EXECUTABLE);
    validate_pe_path(&executable, architecture)?;
    validate_embedded_payload(&executable, &dll)?;
    Ok((dll, executable))
}

fn release_artifact(workspace: &Path, target: &str, name: &str) -> PathBuf {
    workspace
        .join("target")
        .join(target)
        .join("release")
        .join(name)
}

fn cargo_build(
    workspace: &Path,
    package: &str,
    target: &str,
    embedded_dll: Option<&Path>,
) -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(workspace);
    if env::var_os("STAGESWAP_USE_CARGO_XWIN").is_some() {
        command.args(["xwin", "build"]);
    } else {
        command.arg("build");
    }
    command.args(["-p", package, "--release", "--target", target]);
    if let Some(dll) = embedded_dll {
        command.env("STAGESWAP_MEDIA_SOURCE_DLL", dll);
    }
    command
}

fn cargo_metadata(workspace: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        .args(["metadata", "--format-version", "1"])
        .stdout(Stdio::null());
    command
}

fn read_workspace_version(manifest: &Path) -> Result<ReleaseVersion> {
    let contents = fs::read_to_string(manifest)
        .with_context(|| format!("read workspace manifest {}", manifest.display()))?;
    let (version, _) = workspace_version_range(&contents)?;
    Ok(version)
}

fn workspace_version_range(contents: &str) -> Result<(ReleaseVersion, Range<usize>)> {
    let mut in_workspace_package = false;
    let mut offset = 0;
    for line in contents.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_workspace_package = trimmed == "[workspace.package]";
        } else if in_workspace_package
            && let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == "version"
        {
            let value = value
                .split('#')
                .next()
                .expect("split always returns one item")
                .trim();
            let unquoted = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .context("workspace package version must be a quoted string")?;
            let version = ReleaseVersion::parse(unquoted)?;
            let value_offset = line
                .find(unquoted)
                .context("could not locate workspace version value")?;
            let start = offset + value_offset;
            return Ok((version, start..start + unquoted.len()));
        }
        offset += line.len();
    }
    bail!("workspace manifest has no [workspace.package] version")
}

fn replace_workspace_version(contents: &str, version: ReleaseVersion) -> Result<String> {
    let (_, range) = workspace_version_range(contents)?;
    let mut updated = contents.to_owned();
    updated.replace_range(range, &version.to_string());
    Ok(updated)
}

struct VersionTransaction {
    manifest: PathBuf,
    manifest_contents: Vec<u8>,
    lockfile: PathBuf,
    lockfile_contents: Option<Vec<u8>>,
    active: bool,
}

impl VersionTransaction {
    fn begin(manifest: &Path, lockfile: &Path, version: ReleaseVersion) -> Result<Self> {
        let manifest_contents = fs::read(manifest)
            .with_context(|| format!("read workspace manifest {}", manifest.display()))?;
        let manifest_text = std::str::from_utf8(&manifest_contents)
            .context("workspace manifest is not valid UTF-8")?;
        let updated = replace_workspace_version(manifest_text, version)?;
        let lockfile_contents = match fs::read(lockfile) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("read lockfile {}", lockfile.display()));
            }
        };
        fs::write(manifest, updated)
            .with_context(|| format!("update workspace version in {}", manifest.display()))?;
        Ok(Self {
            manifest: manifest.to_owned(),
            manifest_contents,
            lockfile: lockfile.to_owned(),
            lockfile_contents,
            active: true,
        })
    }

    fn commit(&mut self) {
        self.active = false;
    }

    fn rollback(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        fs::write(&self.manifest, &self.manifest_contents)
            .with_context(|| format!("restore workspace manifest {}", self.manifest.display()))?;
        match &self.lockfile_contents {
            Some(contents) => fs::write(&self.lockfile, contents)
                .with_context(|| format!("restore lockfile {}", self.lockfile.display()))?,
            None if self.lockfile.exists() => {
                fs::remove_file(&self.lockfile).with_context(|| {
                    format!("remove generated lockfile {}", self.lockfile.display())
                })?
            }
            None => {}
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for VersionTransaction {
    fn drop(&mut self) {
        if self.active {
            let _ = self.rollback();
        }
    }
}

fn publish_release(
    output: &Path,
    artifact: &str,
    executable: &[u8],
    checksum: &[u8],
) -> Result<PathBuf> {
    fs::create_dir_all(output).context("create dist directory")?;
    let checksum_name = format!("{artifact}.sha256");
    let destination = output.join(artifact);
    let checksum_destination = output.join(&checksum_name);
    ensure_existing_output_matches(&destination, executable)?;
    ensure_existing_output_matches(&checksum_destination, checksum)?;

    let staging = tempfile::tempdir_in(output).context("create release staging directory")?;
    let staged_artifact = staging.path().join(artifact);
    let staged_checksum = staging.path().join(&checksum_name);
    fs::write(&staged_artifact, executable).context("stage release executable")?;
    fs::write(&staged_checksum, checksum).context("stage release checksum")?;

    let artifact_created = if destination.exists() {
        false
    } else {
        fs::rename(&staged_artifact, &destination)
            .with_context(|| format!("publish release executable {}", destination.display()))?;
        true
    };
    if !checksum_destination.exists()
        && let Err(error) = fs::rename(&staged_checksum, &checksum_destination)
    {
        if artifact_created {
            let _ = fs::remove_file(&destination);
        }
        return Err(error).with_context(|| {
            format!(
                "publish release checksum {}",
                checksum_destination.display()
            )
        });
    }
    Ok(destination)
}

fn ensure_existing_output_matches(path: &Path, expected: &[u8]) -> Result<()> {
    if path.exists() {
        let existing = fs::read(path)
            .with_context(|| format!("read existing release output {}", path.display()))?;
        if existing != expected {
            bail!(
                "release output {} already exists with different contents",
                path.display()
            );
        }
    }
    Ok(())
}

fn selected_windows_sdk() -> Result<String> {
    let value = env::var("STAGESWAP_WINDOWS_SDK_VERSION")
        .or_else(|_| env::var("WindowsSDKVersion"))
        .context(
            "Windows SDK is not selected; run from a VS Developer shell or set \
             STAGESWAP_WINDOWS_SDK_VERSION",
        )?;
    let normalized = value.trim().trim_end_matches(['\\', '/']);
    if normalized != REQUIRED_WINDOWS_SDK {
        bail!("Windows SDK {normalized} is selected; packaging requires {REQUIRED_WINDOWS_SDK}");
    }
    Ok(normalized.to_owned())
}

fn run(mut command: Command) -> Result<()> {
    let description = format!("{command:?}");
    let output = command
        .output()
        .with_context(|| format!("could not run {description}"))?;
    if !output.status.success() {
        bail!(
            "command failed ({}): {description}{}",
            output.status,
            command_diagnostics(&output.stdout, &output.stderr)
        );
    }
    Ok(())
}
