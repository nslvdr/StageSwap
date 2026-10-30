#[cfg(any(windows, test))]
use serde::Deserialize;
#[cfg(any(windows, test))]
use stageswap_core::UpdateChannel;
#[cfg(any(windows, test))]
use std::fs;
#[cfg(any(windows, test))]
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::sync::mpsc::{self, Receiver, SyncSender};
#[cfg(windows)]
use std::thread;

#[cfg(windows)]
pub(crate) const RELEASES_API: &str =
    "https://api.github.com/repos/nslvdr/StageSwap/releases?per_page=100";
#[cfg(any(windows, test))]
const RELEASE_ASSET_PREFIX: &str = "StageSwap_win64_v";
#[cfg(windows)]
const MAX_RELEASES_JSON: usize = 2 * 1024 * 1024;
#[cfg(windows)]
const MAX_CHECKSUM_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) use stageswap_core::release::ReleaseVersion;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvailableUpdate {
    pub(crate) version: ReleaseVersion,
    pub(crate) prerelease: bool,
    executable_url: String,
    checksum_url: String,
    asset_digest: Option<String>,
}

#[cfg(any(windows, test))]
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[cfg(any(windows, test))]
#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[cfg(any(windows, test))]
pub(crate) fn select_update(
    json: &[u8],
    channel: UpdateChannel,
    current: ReleaseVersion,
) -> Result<Option<AvailableUpdate>, String> {
    Ok(select_versions(json, channel)?
        .into_iter()
        .find(|release| release.version > current))
}

/// Lists installable releases newest first, including rollback candidates.
#[cfg(any(windows, test))]
pub(crate) fn select_versions(
    json: &[u8],
    channel: UpdateChannel,
) -> Result<Vec<AvailableUpdate>, String> {
    let releases: Vec<GithubRelease> = serde_json::from_slice(json)
        .map_err(|error| format!("invalid GitHub response: {error}"))?;
    let mut candidates = Vec::new();
    for release in releases {
        if release.draft || (channel == UpdateChannel::Stable && release.prerelease) {
            continue;
        }
        let Ok(version) = ReleaseVersion::parse(&release.tag_name) else {
            continue;
        };
        if release.tag_name != format!("v{version}") {
            continue;
        }
        let executable_name = format!("{RELEASE_ASSET_PREFIX}{version}.exe");
        let checksum_name = format!("{executable_name}.sha256");
        let Some(executable) = release
            .assets
            .iter()
            .find(|asset| asset.name == executable_name)
        else {
            continue;
        };
        let Some(checksum) = release
            .assets
            .iter()
            .find(|asset| asset.name == checksum_name)
        else {
            continue;
        };
        if !is_https_github_url(&executable.browser_download_url)
            || !is_https_github_url(&checksum.browser_download_url)
        {
            continue;
        }
        candidates.push(AvailableUpdate {
            version,
            prerelease: release.prerelease,
            executable_url: executable.browser_download_url.clone(),
            checksum_url: checksum.browser_download_url.clone(),
            asset_digest: executable.digest.clone(),
        });
    }
    candidates.sort_by_key(|release| std::cmp::Reverse(release.version));
    candidates.dedup_by_key(|release| release.version);
    Ok(candidates)
}

#[cfg(any(windows, test))]
fn is_https_github_url(url: &str) -> bool {
    let Some(authority) = url.strip_prefix("https://") else {
        return false;
    };
    let host = authority.split('/').next().unwrap_or_default();
    host == "github.com" || host.ends_with(".githubusercontent.com")
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available(AvailableUpdate),
    Downloading(AvailableUpdate),
    Installing,
    Failed(String),
}

/// Catalog loading is independent of the normal latest-update check.
#[derive(Clone, Debug, Default)]
pub(crate) enum VersionCatalog {
    #[default]
    Idle,
    Loading,
    Ready(Vec<AvailableUpdate>),
    Failed(String),
}

/// Deterministic preview releases cannot be downloaded or installed.
#[cfg(any(not(windows), test))]
pub(crate) fn preview_versions() -> Vec<AvailableUpdate> {
    let mut releases = [(1, 0, 4, true), (1, 0, 3, true), (1, 0, 2, false)]
        .into_iter()
        .map(|(major, minor, patch, prerelease)| AvailableUpdate {
            version: ReleaseVersion {
                major,
                minor,
                patch,
            },
            prerelease,
            executable_url: String::new(),
            checksum_url: String::new(),
            asset_digest: None,
        })
        .collect::<Vec<_>>();
    let mut current = releases[0].clone();
    current.version = ReleaseVersion::parse(env!("CARGO_PKG_VERSION")).expect("package version");
    releases.insert(0, current);
    releases
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) enum UpdateRequest {
    Check {
        channel: UpdateChannel,
        manual: bool,
    },
    ListVersions,
    Install(AvailableUpdate),
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) enum UpdateResult {
    Checked {
        manual: bool,
        result: Result<Option<AvailableUpdate>, String>,
    },
    Versions(Result<Vec<AvailableUpdate>, String>),
    InstallStarted,
    InstallFailed(String),
}

#[cfg(windows)]
pub(crate) struct UpdateWorker {
    sender: SyncSender<UpdateRequest>,
    receiver: Receiver<UpdateResult>,
}

#[cfg(windows)]
impl UpdateWorker {
    pub(crate) fn start(directory: PathBuf, current: ReleaseVersion) -> Result<Self, String> {
        let (sender, requests) = mpsc::sync_channel(2);
        let (results, receiver) = mpsc::sync_channel(2);
        thread::Builder::new()
            .name("stageswap-update".into())
            .spawn(move || {
                while let Ok(request) = requests.recv() {
                    match request {
                        UpdateRequest::Check { channel, manual } => {
                            let result = check_for_update(channel, current);
                            if results
                                .send(UpdateResult::Checked { manual, result })
                                .is_err()
                            {
                                break;
                            }
                        }
                        UpdateRequest::ListVersions => {
                            let result =
                                stageswap_windows::https_get(RELEASES_API, MAX_RELEASES_JSON)
                                    .and_then(|json| select_versions(&json, UpdateChannel::Beta));
                            if results.send(UpdateResult::Versions(result)).is_err() {
                                break;
                            }
                        }
                        UpdateRequest::Install(release) => {
                            let result = download_update(&directory, &release)
                                .and_then(|candidate| stageswap_windows::launch_update(&candidate));
                            let result = match result {
                                Ok(()) => UpdateResult::InstallStarted,
                                Err(error) => UpdateResult::InstallFailed(error),
                            };
                            if results.send(result).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|error| format!("could not start update worker: {error}"))?;
        Ok(Self { sender, receiver })
    }

    pub(crate) fn request(&self, request: UpdateRequest) -> Result<(), String> {
        self.sender.try_send(request).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => "an update request is already running".into(),
            mpsc::TrySendError::Disconnected(_) => "the update worker is unavailable".into(),
        })
    }

    pub(crate) fn poll(&self) -> Option<UpdateResult> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(windows)]
fn check_for_update(
    channel: UpdateChannel,
    current: ReleaseVersion,
) -> Result<Option<AvailableUpdate>, String> {
    let response = stageswap_windows::https_get(RELEASES_API, MAX_RELEASES_JSON)?;
    select_update(&response, channel, current)
}

#[cfg(windows)]
fn download_update(directory: &Path, release: &AvailableUpdate) -> Result<PathBuf, String> {
    let update_directory = directory.join("updates");
    fs::create_dir_all(&update_directory).map_err(|error| {
        format!(
            "could not create update directory {}: {error}",
            update_directory.display()
        )
    })?;
    cleanup_stale_downloads(&update_directory, release.version);
    let executable_name = format!("{RELEASE_ASSET_PREFIX}{}.exe", release.version);
    let final_path = update_directory.join(&executable_name);
    let partial_path = update_directory.join(format!("{executable_name}.partial"));
    let checksum = stageswap_windows::https_get(&release.checksum_url, MAX_CHECKSUM_BYTES)?;
    let expected = parse_checksum(&checksum, release.version, &executable_name)?;
    if final_path.exists() && hash_file(&final_path)? == expected {
        return Ok(final_path);
    }
    let _ = fs::remove_file(&partial_path);
    stageswap_windows::https_download(
        &release.executable_url,
        &partial_path,
        MAX_EXECUTABLE_BYTES,
    )?;
    let actual = hash_file(&partial_path)?;
    if actual != expected {
        let _ = fs::remove_file(&partial_path);
        return Err("the downloaded update did not match its SHA-256 checksum".into());
    }
    if let Some(github_digest) = release.asset_digest.as_deref()
        && github_digest != format!("sha256:{actual}")
    {
        let _ = fs::remove_file(&partial_path);
        return Err("the downloaded update did not match GitHub's asset digest".into());
    }
    if final_path.exists() {
        fs::remove_file(&final_path)
            .map_err(|error| format!("could not replace cached update: {error}"))?;
    }
    fs::rename(&partial_path, &final_path)
        .map_err(|error| format!("could not activate downloaded update: {error}"))?;
    Ok(final_path)
}

#[cfg(any(windows, test))]
fn parse_checksum(bytes: &[u8], version: ReleaseVersion, filename: &str) -> Result<String, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "the update checksum is not valid UTF-8".to_owned())?;
    let application = metadata_value(text, "applicationVersion")
        .ok_or_else(|| "the checksum is missing applicationVersion".to_owned())?;
    let release = metadata_value(text, "releaseVersion")
        .ok_or_else(|| "the checksum is missing releaseVersion".to_owned())?;
    if application != version.to_string() || release != version.to_string() {
        return Err("the checksum metadata does not match the release version".into());
    }
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .ok_or_else(|| "the checksum file contains no digest".to_owned())?;
    let mut parts = line.split_whitespace();
    let digest = parts.next().unwrap_or_default().to_ascii_lowercase();
    let named_file = parts
        .next()
        .unwrap_or_default()
        .trim_start_matches(['*', ' ']);
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the checksum file contains an invalid SHA-256 digest".into());
    }
    if named_file != filename {
        return Err("the checksum file names a different executable".into());
    }
    Ok(digest)
}

#[cfg(any(windows, test))]
fn metadata_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        line.strip_prefix(&format!("# {key}="))
            .or_else(|| line.strip_prefix(&format!("#{key}=")))
    })
}

#[cfg(windows)]
fn hash_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(stageswap_core::hex_digest(&stageswap_core::sha256(&bytes)))
}

#[cfg(windows)]
fn cleanup_stale_downloads(directory: &Path, keep: ReleaseVersion) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.contains(&format!("v{keep}.exe")) {
            continue;
        }
        if name.starts_with(RELEASE_ASSET_PREFIX)
            && (name.ends_with(".exe") || name.ends_with(".partial"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(any(windows, test))]
#[derive(Clone, Debug, Default, Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct UpdateNotificationState {
    schema_version: u32,
    stable: String,
    beta: String,
}

#[cfg(any(windows, test))]
impl UpdateNotificationState {
    #[cfg(any(windows, test))]
    pub(crate) fn load(directory: &Path) -> Self {
        fs::read_to_string(directory.join("update-state.json"))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .filter(|state: &Self| state.schema_version == 1)
            .unwrap_or_else(|| Self {
                schema_version: 1,
                ..Self::default()
            })
    }

    pub(crate) fn should_notify(
        &mut self,
        channel: UpdateChannel,
        version: ReleaseVersion,
    ) -> bool {
        let value = match channel {
            UpdateChannel::Stable => &mut self.stable,
            UpdateChannel::Beta => &mut self.beta,
        };
        let version = version.to_string();
        if *value == version {
            false
        } else {
            *value = version;
            true
        }
    }

    #[cfg(any(windows, test))]
    pub(crate) fn save(&self, directory: &Path) -> Result<(), String> {
        fs::create_dir_all(directory)
            .map_err(|error| format!("could not create update state directory: {error}"))?;
        let path = directory.join("update-state.json");
        let staging = directory.join("update-state.json.tmp");
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("could not serialize update state: {error}"))?;
        fs::write(&staging, json)
            .map_err(|error| format!("could not write update state: {error}"))?;
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        fs::rename(staging, path).map_err(|error| format!("could not save update state: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn releases_json() -> Vec<u8> {
        br#"[
          {"tag_name":"v2.0.0","html_url":"https://github.com/nslvdr/StageSwap/releases/tag/v2.0.0","draft":false,"prerelease":true,"assets":[
            {"name":"StageSwap_win64_v2.0.0.exe","browser_download_url":"https://github.com/nslvdr/StageSwap/releases/download/v2.0.0/StageSwap_win64_v2.0.0.exe","digest":"sha256:bbbb"},
            {"name":"StageSwap_win64_v2.0.0.exe.sha256","browser_download_url":"https://github.com/nslvdr/StageSwap/releases/download/v2.0.0/StageSwap_win64_v2.0.0.exe.sha256"}]},
          {"tag_name":"v1.9.0","html_url":"https://github.com/nslvdr/StageSwap/releases/tag/v1.9.0","draft":false,"prerelease":false,"assets":[
            {"name":"StageSwap_win64_v1.9.0.exe","browser_download_url":"https://github.com/nslvdr/StageSwap/releases/download/v1.9.0/StageSwap_win64_v1.9.0.exe"},
            {"name":"StageSwap_win64_v1.9.0.exe.sha256","browser_download_url":"https://github.com/nslvdr/StageSwap/releases/download/v1.9.0/StageSwap_win64_v1.9.0.exe.sha256"}]},
          {"tag_name":"nightly","html_url":"https://example.invalid","draft":false,"prerelease":false,"assets":[]},
          {"tag_name":"v9.0.0","html_url":"https://example.invalid","draft":true,"prerelease":false,"assets":[]}
        ]"#.to_vec()
    }

    #[test]
    fn contract_stable_and_beta_select_their_expected_versions() {
        let current = ReleaseVersion::parse("1.0.0").unwrap();
        assert_eq!(
            select_update(&releases_json(), UpdateChannel::Stable, current)
                .unwrap()
                .unwrap()
                .version,
            ReleaseVersion::parse("1.9.0").unwrap()
        );
        assert_eq!(
            select_update(&releases_json(), UpdateChannel::Beta, current)
                .unwrap()
                .unwrap()
                .version,
            ReleaseVersion::parse("2.0.0").unwrap()
        );
    }

    #[test]
    fn contract_current_and_older_versions_are_not_updates() {
        let current = ReleaseVersion::parse("2.0.0").unwrap();
        assert!(
            select_update(&releases_json(), UpdateChannel::Beta, current)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn contract_version_picker_includes_rollback_candidates_and_filters_invalid_assets() {
        let mut releases: serde_json::Value = serde_json::from_slice(&releases_json()).unwrap();
        let valid = select_versions(&releases_json(), UpdateChannel::Beta).unwrap();
        assert_eq!(
            valid
                .iter()
                .map(|r| r.version.to_string())
                .collect::<Vec<_>>(),
            ["2.0.0", "1.9.0"]
        );
        assert_eq!(
            select_versions(&releases_json(), UpdateChannel::Stable)
                .unwrap()
                .len(),
            1
        );
        releases[0]["assets"][0]["browser_download_url"] = "http://github.com/unsafe".into();
        releases[1]["assets"].as_array_mut().unwrap().pop();
        assert!(
            select_versions(&serde_json::to_vec(&releases).unwrap(), UpdateChannel::Beta)
                .unwrap()
                .is_empty()
        );
        assert!(select_versions(b"invalid", UpdateChannel::Beta).is_err());
    }

    #[test]
    fn contract_checksum_requires_matching_metadata_filename_and_digest() {
        let version = ReleaseVersion::parse("2.3.4").unwrap();
        let filename = "StageSwap_win64_v2.3.4.exe";
        let text = format!(
            "# applicationVersion=2.3.4\n# releaseVersion=2.3.4\n{} *{}\n",
            "a".repeat(64),
            filename
        );
        assert_eq!(
            parse_checksum(text.as_bytes(), version, filename).unwrap(),
            "a".repeat(64)
        );
        assert!(parse_checksum(text.as_bytes(), version, "other.exe").is_err());
    }

    #[test]
    fn contract_update_notifications_dedupe_and_persist_per_channel() {
        let mut state = UpdateNotificationState {
            schema_version: 1,
            ..UpdateNotificationState::default()
        };
        let version = ReleaseVersion::parse("3.0.0").unwrap();
        assert!(state.should_notify(UpdateChannel::Stable, version));
        assert!(!state.should_notify(UpdateChannel::Stable, version));
        assert!(state.should_notify(UpdateChannel::Beta, version));
        let directory = tempfile::tempdir().unwrap();
        state.should_notify(
            UpdateChannel::Stable,
            ReleaseVersion::parse("4.0.0").unwrap(),
        );
        state.should_notify(UpdateChannel::Beta, ReleaseVersion::parse("4.1.0").unwrap());
        state.save(directory.path()).unwrap();
        let restored = UpdateNotificationState::load(directory.path());
        assert_eq!(restored.stable, "4.0.0");
        assert_eq!(restored.beta, "4.1.0");
    }
}
