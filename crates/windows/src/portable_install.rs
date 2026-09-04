use crate::dialog::{TaskDialogIcon, TaskDialogSpec, show_task_dialog};
use crate::instance_scope::install_mutex_name;
use crate::user_identity::current_user_sid;
use crate::{
    InstanceCommand, InstanceStatus, SingleInstance, instance_status, send_instance_command,
};
use stageswap_i18n::{Locale, format_text, text};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW,
    VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::TDCBF_CANCEL_BUTTON;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{Interface, PCWSTR};

const MANAGED_DIRECTORY: &str = "Programs\\StageSwap";
const MANAGED_EXECUTABLE: &str = "StageSwap.exe";
const STAGING_EXECUTABLE: &str = "StageSwap.exe.installing";
const PREVIOUS_EXECUTABLE: &str = "StageSwap.exe.previous";
const INSTANCE_EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const REPLACEMENT_READY_TIMEOUT: Duration = Duration::from_secs(120);
const INSTALL_BUTTON: i32 = 100;
const RUN_ONCE_BUTTON: i32 = 101;
const UPDATE_BUTTON: i32 = 102;
const OPEN_INSTALLED_BUTTON: i32 = 103;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortableMode {
    Managed,
    RunOnce,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchContext {
    pub mode: PortableMode,
    pub force_visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapResult {
    Continue(LaunchContext),
    Exit,
}

pub fn bootstrap(payload: &[u8], locale: Locale) -> Result<BootstrapResult, String> {
    let argument = env::args().nth(1);
    if matches!(
        argument.as_deref(),
        Some("--register-elevated" | "--cleanup-elevated" | "--cleanup")
    ) {
        return Ok(BootstrapResult::Continue(LaunchContext {
            mode: PortableMode::Managed,
            force_visible: false,
        }));
    }
    if argument.as_deref() == Some("--uninstall") {
        uninstall()?;
        return Ok(BootstrapResult::Exit);
    }

    let current = env::current_exe()
        .map_err(|error| format!("could not locate the StageSwap executable: {error}"))?;
    let managed = managed_executable_path()?;
    if paths_match(&current, &managed) {
        return Ok(BootstrapResult::Continue(LaunchContext {
            mode: PortableMode::Managed,
            force_visible: matches!(
                argument.as_deref(),
                Some("--show" | "--post-install" | "--post-update" | "--rollback")
            ),
        }));
    }

    if argument.as_deref() == Some("--install-request") {
        install_or_replace(&current, &managed, managed.exists(), payload)?;
        return Ok(BootstrapResult::Exit);
    }
    if argument.as_deref() == Some("--startup") {
        if managed.is_file() {
            launch(&managed, "--startup")?;
        }
        return Ok(BootstrapResult::Exit);
    }

    if managed.is_file() {
        if files_equal(&current, &managed)? {
            open_installed(&managed, true)?;
            return Ok(BootstrapResult::Exit);
        }
        let installed_version = product_version(&managed).unwrap_or_else(|| "unknown".into());
        let candidate_version = product_version(&current).unwrap_or_else(|| "unknown".into());
        let relation = version_relation(&candidate_version, &installed_version, locale);
        let content = format_text(
            locale,
            "Installed version: {0}\nCandidate version: {1}\n\n{2}",
            &[&installed_version, &candidate_version, relation.as_ref()],
        );
        match task_dialog(
            locale,
            "Replace the installed StageSwap?",
            &content,
            &[
                (UPDATE_BUTTON, "Update installed StageSwap"),
                (OPEN_INSTALLED_BUTTON, "Open installed StageSwap"),
            ],
            UPDATE_BUTTON,
        )? {
            UPDATE_BUTTON => install_or_replace(&current, &managed, true, payload)?,
            OPEN_INSTALLED_BUTTON => open_installed(&managed, true)?,
            _ => {}
        }
        return Ok(BootstrapResult::Exit);
    }

    match task_dialog(
        locale,
        "Install StageSwap for this user?",
        "Installation keeps the app at a stable per-user path and creates Start Menu and Desktop shortcuts. The virtual-camera component still requires administrator approval.",
        &[
            (
                INSTALL_BUTTON,
                "Install StageSwap\nRecommended for startup and upgrades",
            ),
            (RUN_ONCE_BUTTON, "Run once\nDo not copy this executable"),
        ],
        INSTALL_BUTTON,
    )? {
        INSTALL_BUTTON => {
            install_or_replace(&current, &managed, false, payload)?;
            Ok(BootstrapResult::Exit)
        }
        RUN_ONCE_BUTTON => Ok(BootstrapResult::Continue(LaunchContext {
            mode: PortableMode::RunOnce,
            force_visible: true,
        })),
        _ => Ok(BootstrapResult::Exit),
    }
}

pub fn managed_executable_path() -> Result<PathBuf, String> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is not defined".to_string())
        .map(|path| path.join(MANAGED_DIRECTORY).join(MANAGED_EXECUTABLE))
}

pub fn request_install() -> Result<(), String> {
    let current = env::current_exe()
        .map_err(|error| format!("could not locate the StageSwap executable: {error}"))?;
    launch(&current, "--install-request")
}

pub fn launch_update(candidate: &Path) -> Result<(), String> {
    if !candidate.is_file() {
        return Err(format!(
            "the downloaded update does not exist at {}",
            candidate.display()
        ));
    }
    launch(candidate, "--install-request")
}

fn install_or_replace(
    source: &Path,
    managed: &Path,
    replacing: bool,
    payload: &[u8],
) -> Result<(), String> {
    let _install_lock = InstallLock::acquire()?;
    crate::deployment::validate_release(payload)?;
    let directory = managed
        .parent()
        .ok_or_else(|| "managed executable path has no parent".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let staging = directory.join(STAGING_EXECUTABLE);
    let previous = directory.join(PREVIOUS_EXECUTABLE);
    remove_file_if_present(&staging)?;
    copy_verified(source, &staging)?;

    stop_running_instance()?;
    remove_file_if_present(&previous)?;
    if replacing && managed.exists() {
        fs::rename(managed, &previous).map_err(|error| {
            format!(
                "could not preserve the previous StageSwap executable at {}: {error}",
                previous.display()
            )
        })?;
    }
    if let Err(error) = fs::rename(&staging, managed) {
        if previous.exists() {
            let _ = fs::rename(&previous, managed);
        }
        return Err(format!(
            "could not activate the StageSwap executable at {}: {error}",
            managed.display()
        ));
    }

    if let Err(error) = create_shortcuts(managed) {
        rollback_files(managed, &previous);
        if replacing && managed.exists() {
            let _ = launch(managed, "--rollback");
        } else {
            let _ = remove_shortcuts();
        }
        return Err(error);
    }
    let child = match launch_child(
        managed,
        if replacing {
            "--post-update"
        } else {
            "--post-install"
        },
    ) {
        Ok(child) => child,
        Err(error) => {
            rollback_files(managed, &previous);
            if replacing && managed.exists() {
                let _ = launch(managed, "--rollback");
            } else {
                let _ = remove_shortcuts();
            }
            return Err(error);
        }
    };
    if let Err(error) = wait_until_ready(child) {
        if SingleInstance::exists()? {
            return Err(format!(
                "the replacement did not become ready and is still running. It was not forced closed; the rollback executable remains at {}.\n\n{error}",
                previous.display()
            ));
        }
        rollback_files(managed, &previous);
        if previous.exists() || managed.exists() {
            let restored = if previous.exists() {
                &previous
            } else {
                managed
            };
            if restored.exists() && restored != managed {
                let _ = fs::rename(restored, managed);
            }
            if managed.exists() {
                let _ = launch(managed, "--rollback");
            }
        }
        if !replacing {
            let _ = remove_shortcuts();
        }
        return Err(format!("the replacement did not become ready: {error}"));
    }
    remove_file_if_present(&previous)?;
    Ok(())
}

fn rollback_files(managed: &Path, previous: &Path) {
    let _ = remove_file_if_present(managed);
    if previous.exists() {
        let _ = fs::rename(previous, managed);
    }
}

fn stop_running_instance() -> Result<(), String> {
    if !SingleInstance::exists()? {
        return Ok(());
    }
    send_instance_command(InstanceCommand::ShutdownForReplacement).map_err(|error| {
        format!(
            "StageSwap is already running but cannot accept an upgrade request. Exit it from the system tray and try again.\n\n{error}"
        )
    })?;
    let deadline = Instant::now() + INSTANCE_EXIT_TIMEOUT;
    while Instant::now() < deadline {
        if !SingleInstance::exists()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(
        "the running StageSwap instance did not exit within 10 seconds; it was not forced closed"
            .into(),
    )
}

fn wait_until_ready(mut child: Child) -> Result<(), String> {
    let deadline = Instant::now() + REPLACEMENT_READY_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not inspect replacement process: {error}"))?
        {
            return Err(format!("process exited with {status}"));
        }
        match instance_status() {
            Ok(InstanceStatus::Ready) => return Ok(()),
            Ok(InstanceStatus::Starting) | Err(_) => {}
        }
        thread::sleep(Duration::from_millis(200));
    }
    if send_instance_command(InstanceCommand::ShutdownForReplacement).is_ok() {
        let exit_deadline = Instant::now() + INSTANCE_EXIT_TIMEOUT;
        while Instant::now() < exit_deadline {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    Err("timed out after 120 seconds; the process was not forced closed".into())
}

fn open_installed(managed: &Path, show: bool) -> Result<(), String> {
    if SingleInstance::exists()? {
        if show {
            send_instance_command(InstanceCommand::Show).map_err(|error| {
                format!(
                    "StageSwap is already running but could not show its window. Exit the legacy tray instance and try again.\n\n{error}"
                )
            })?;
        }
        Ok(())
    } else {
        launch(managed, if show { "--show" } else { "--startup" })
    }
}

fn launch(path: &Path, argument: &str) -> Result<(), String> {
    launch_child(path, argument).map(|_| ())
}

fn launch_child(path: &Path, argument: &str) -> Result<Child, String> {
    Command::new(path)
        .arg(argument)
        .spawn()
        .map_err(|error| format!("could not launch {}: {error}", path.display()))
}

fn uninstall() -> Result<(), String> {
    let _install_lock = InstallLock::acquire()?;
    stop_running_instance()?;
    crate::uninstall_deployment()?;
    remove_shortcuts()?;
    remove_managed_files()?;
    Ok(())
}

struct InstallLock(HANDLE);

impl InstallLock {
    fn acquire() -> Result<Self, String> {
        let name = wide(&install_mutex_name(&current_user_sid()?));
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|error| format!("could not create installation lock: {error}"))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = unsafe { CloseHandle(handle) };
            return Err("another StageSwap installation or removal is already in progress".into());
        }
        Ok(Self(handle))
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub(crate) fn remove_managed_files() -> Result<(), String> {
    let managed = managed_executable_path()?;
    let Some(directory) = managed.parent() else {
        return Ok(());
    };
    remove_file_if_present(&directory.join(STAGING_EXECUTABLE))?;
    remove_file_if_present(&directory.join(PREVIOUS_EXECUTABLE))?;
    let current = env::current_exe().ok();
    if managed.exists()
        && current
            .as_deref()
            .is_some_and(|path| paths_match(path, &managed))
    {
        let managed_wide = wide_os(managed.as_os_str());
        unsafe {
            MoveFileExW(
                PCWSTR(managed_wide.as_ptr()),
                PCWSTR::null(),
                MOVEFILE_DELAY_UNTIL_REBOOT,
            )
        }
        .map_err(|error| format!("could not schedule managed executable removal: {error}"))?;
        let directory_wide = wide_os(directory.as_os_str());
        let _ = unsafe {
            MoveFileExW(
                PCWSTR(directory_wide.as_ptr()),
                PCWSTR::null(),
                MOVEFILE_DELAY_UNTIL_REBOOT,
            )
        };
    } else {
        remove_file_if_present(&managed)?;
        let _ = fs::remove_dir(directory);
    }
    Ok(())
}

fn copy_verified(source: &Path, destination: &Path) -> Result<(), String> {
    fs::copy(source, destination).map_err(|error| {
        format!(
            "could not copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    if !files_equal(source, destination)? {
        let _ = fs::remove_file(destination);
        return Err("the staged StageSwap executable did not match the downloaded file".into());
    }
    Ok(())
}

fn files_equal(left: &Path, right: &Path) -> Result<bool, String> {
    let left_metadata = fs::metadata(left)
        .map_err(|error| format!("could not inspect {}: {error}", left.display()))?;
    let right_metadata = fs::metadata(right)
        .map_err(|error| format!("could not inspect {}: {error}", right.display()))?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }
    let mut left = fs::File::open(left).map_err(|error| error.to_string())?;
    let mut right = fs::File::open(right).map_err(|error| error.to_string())?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left
            .read(&mut left_buffer)
            .map_err(|error| error.to_string())?;
        let right_read = right
            .read(&mut right_buffer)
            .map_err(|error| error.to_string())?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn remove_file_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not remove {}: {error}", path.display())),
    }
}

fn paths_match(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}

fn version_relation(
    candidate: &str,
    installed: &str,
    locale: Locale,
) -> std::borrow::Cow<'static, str> {
    let message = match (parse_version(candidate), parse_version(installed)) {
        (Some(candidate), Some(installed)) if candidate < installed => {
            "This replaces the installed app with an older version."
        }
        (Some(candidate), Some(installed)) if candidate == installed => {
            "This replaces it with a different build of the same version."
        }
        _ => "The running app will close gracefully before replacement.",
    };
    text(locale, message)
}

fn parse_version(value: &str) -> Option<Vec<u32>> {
    value
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()
}

fn product_version(path: &Path) -> Option<String> {
    let path = wide_os(path.as_os_str());
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0_u8; size as usize];
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .ok()?;
    let root = wide("\\");
    let mut fixed = std::ptr::null_mut();
    let mut length = 0;
    if !unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(root.as_ptr()),
            &mut fixed,
            &mut length,
        )
    }
    .as_bool()
        || length < size_of::<VS_FIXEDFILEINFO>() as u32
    {
        return None;
    }
    let fixed = unsafe { fixed.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    Some(format!(
        "{}.{}.{}.{}",
        fixed.dwProductVersionMS >> 16,
        fixed.dwProductVersionMS & 0xffff,
        fixed.dwProductVersionLS >> 16,
        fixed.dwProductVersionLS & 0xffff
    ))
}

fn task_dialog(
    locale: Locale,
    instruction: &str,
    content: &str,
    choices: &[(i32, &str)],
    default: i32,
) -> Result<i32, String> {
    let instruction = text(locale, instruction);
    let content = text(locale, content);
    let localized_choices: Vec<(i32, std::borrow::Cow<'_, str>)> = choices
        .iter()
        .map(|(id, label)| (*id, text(locale, label)))
        .collect();
    let choice_refs: Vec<(i32, &str)> = localized_choices
        .iter()
        .map(|(id, label)| (*id, label.as_ref()))
        .collect();
    show_task_dialog(TaskDialogSpec {
        instruction: instruction.as_ref(),
        content: content.as_ref(),
        icon: TaskDialogIcon::Information,
        choices: &choice_refs,
        default_button: default,
        common_buttons: TDCBF_CANCEL_BUTTON,
        command_links: true,
    })
}

fn create_shortcuts(executable: &Path) -> Result<(), String> {
    for folder in [FOLDERID_Programs, FOLDERID_Desktop] {
        let folder = known_folder(&folder)?;
        create_shortcut(executable, &folder.join("StageSwap.lnk"))?;
    }
    Ok(())
}

fn remove_shortcuts() -> Result<(), String> {
    for folder in [FOLDERID_Programs, FOLDERID_Desktop] {
        let folder = known_folder(&folder)?;
        remove_file_if_present(&folder.join("StageSwap.lnk"))?;
    }
    Ok(())
}

fn create_shortcut(executable: &Path, shortcut: &Path) -> Result<(), String> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = (|| {
        let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| format!("could not create shortcut object: {error}"))?;
        let executable = wide_os(executable.as_os_str());
        unsafe {
            link.SetPath(PCWSTR(executable.as_ptr()))
                .map_err(|error| format!("could not set shortcut target: {error}"))?;
            link.SetDescription(PCWSTR(wide("StageSwap").as_ptr()))
                .map_err(|error| format!("could not set shortcut description: {error}"))?;
            link.SetIconLocation(PCWSTR(executable.as_ptr()), 0)
                .map_err(|error| format!("could not set shortcut icon: {error}"))?;
            link.SetShowCmd(SW_SHOWNORMAL)
                .map_err(|error| format!("could not set shortcut visibility: {error}"))?;
        }
        let persist: IPersistFile = link
            .cast()
            .map_err(|error| format!("could not persist shortcut: {error}"))?;
        let shortcut = wide_os(shortcut.as_os_str());
        unsafe { persist.Save(PCWSTR(shortcut.as_ptr()), true) }
            .map_err(|error| format!("could not save shortcut: {error}"))
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

fn known_folder(folder: &windows::core::GUID) -> Result<PathBuf, String> {
    let path = unsafe { SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None) }
        .map_err(|error| format!("could not locate Windows known folder: {error}"))?;
    let mut length = 0;
    while unsafe { *path.0.add(length) } != 0 {
        length += 1;
    }
    let value = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(path.0, length)) };
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(path.0.cast())) };
    Ok(PathBuf::from(value))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn wide_os(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_managed_location_and_transient_names_are_stable() {
        assert_eq!(MANAGED_DIRECTORY, "Programs\\StageSwap");
        assert_eq!(MANAGED_EXECUTABLE, "StageSwap.exe");
        assert_eq!(STAGING_EXECUTABLE, "StageSwap.exe.installing");
        assert_eq!(PREVIOUS_EXECUTABLE, "StageSwap.exe.previous");
    }

    #[test]
    fn native_version_relation_calls_out_downgrades_and_rebuilds() {
        assert!(version_relation("1.0.0", "2.0.0", Locale::English).contains("older"));
        assert!(version_relation("2.0.0", "2.0.0", Locale::English).contains("different build"));
        assert!(version_relation("3.0.0", "2.0.0", Locale::English).contains("close gracefully"));
    }

    #[test]
    fn native_content_comparison_detects_identical_and_changed_executables() {
        let directory = env::temp_dir().join(format!(
            "stageswap-portable-install-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.exe");
        let second = directory.join("second.exe");
        fs::write(&first, b"same bytes").unwrap();
        fs::write(&second, b"same bytes").unwrap();
        assert!(files_equal(&first, &second).unwrap());
        fs::write(&second, b"changed bytes").unwrap();
        assert!(!files_equal(&first, &second).unwrap());
        fs::remove_dir_all(directory).unwrap();
    }
}
