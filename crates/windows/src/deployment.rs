use crate::component_compatibility::{ComponentCompatibility, ComponentDecision, decide_component};
use crate::remove_virtual_camera;
use semver::Version;
use stageswap_core::{AppConfig, ConfigStore, IPC_PROTOCOL_VERSION};
use std::env;
use std::fs;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, FreeLibrary, GetLastError, HANDLE,
    HLOCAL, LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    MOVEFILE_DELAY_UNTIL_REBOOT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegGetValueW,
    RegSetValueExW,
};
use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64;
use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_UNKNOWN;
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, GetExitCodeProcess, INFINITE, IsWow64Process2, ReleaseMutex,
    WaitForSingleObject,
};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_core::{HRESULT, PCSTR, PCWSTR};

const SOURCE_FILE: &str = "StageSwapSource.dll";
const SOURCE_FILE_PREFIX: &str = "StageSwapSource-";
const DEPLOYMENT_DIRECTORY_NAME: &str = "StageSwap";
const SOURCE_CLASS_KEY: &str = r"Software\Classes\CLSID\{4ABA794D-7B23-449C-8467-CE74A41C2820}";
const SOURCE_INPROC_KEY: &str =
    r"Software\Classes\CLSID\{4ABA794D-7B23-449C-8467-CE74A41C2820}\InprocServer32";
const COMPONENT_VERSION_VALUE: &str = "StageSwapComponentVersion";
const PROTOCOL_MIN_VALUE: &str = "StageSwapProtocolMin";
const PROTOCOL_MAX_VALUE: &str = "StageSwapProtocolMax";
const MACHINE_DEPLOYMENT_MUTEX: &str = r"Global\StageSwap.SourceDeployment";
const MACHINE_DEPLOYMENT_TIMEOUT_MS: u32 = 120_000;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "StageSwap";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupRegistrationStatus {
    Missing,
    Current,
    Different(String),
}

fn expected_startup_command() -> Result<String, String> {
    let executable = crate::managed_executable_path()?;
    Ok(startup_command(&executable))
}

fn startup_command(executable: &Path) -> String {
    format!("\"{}\" --startup", executable.display())
}

fn classify_startup_command(actual: String, expected: &str) -> StartupRegistrationStatus {
    if actual.eq_ignore_ascii_case(expected) {
        StartupRegistrationStatus::Current
    } else {
        StartupRegistrationStatus::Different(actual)
    }
}

pub fn startup_registration_status() -> Result<StartupRegistrationStatus, String> {
    let key: Vec<u16> = RUN_KEY.encode_utf16().chain([0]).collect();
    let value_name: Vec<u16> = RUN_VALUE.encode_utf16().chain([0]).collect();
    let mut bytes = 0;
    // SAFETY: key and value_name are terminated; this call only requests the value size.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(StartupRegistrationStatus::Missing);
    }
    if status.is_err() {
        return Err(format!(
            "could not read Windows startup preference: {status:?}"
        ));
    }
    let mut value = vec![0_u16; (bytes as usize).div_ceil(size_of::<u16>())];
    // SAFETY: value has the byte capacity reported by RegGetValueW.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if status.is_err() {
        return Err(format!(
            "could not read Windows startup preference: {status:?}"
        ));
    }
    let length = value
        .iter()
        .position(|word| *word == 0)
        .unwrap_or(value.len());
    let actual = String::from_utf16_lossy(&value[..length]);
    Ok(classify_startup_command(
        actual,
        &expected_startup_command()?,
    ))
}

pub fn configure_startup(enabled: bool) -> Result<(), String> {
    let executable = enabled.then(crate::managed_executable_path).transpose()?;
    let path: Vec<u16> = RUN_KEY.encode_utf16().chain([0]).collect();
    let value_name: Vec<u16> = RUN_VALUE.encode_utf16().chain([0]).collect();
    let mut key = HKEY::default();
    // SAFETY: path is terminated and key is writable.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if status.is_err() {
        return Err(format!("could not open Windows startup key: {status:?}"));
    }
    let result = if enabled {
        let executable = executable.expect("enabled startup has a managed executable path");
        if !executable.is_file() {
            let _ = unsafe { RegCloseKey(key) };
            return Err("Start with Windows requires the managed StageSwap installation".into());
        }
        let command = startup_command(&executable);
        let command: Vec<u16> = command.encode_utf16().chain([0]).collect();
        // SAFETY: the key and both buffers are live for the call.
        unsafe {
            RegSetValueExW(
                key,
                PCWSTR(value_name.as_ptr()),
                None,
                REG_SZ,
                Some(core::slice::from_raw_parts(
                    command.as_ptr().cast::<u8>(),
                    command.len() * 2,
                )),
            )
        }
    } else {
        // SAFETY: the key is live and value name is terminated.
        unsafe { RegDeleteValueW(key, PCWSTR(value_name.as_ptr())) }
    };
    let _ = unsafe { RegCloseKey(key) };
    if result.is_ok() || (!enabled && result == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(format!(
            "could not update Windows startup preference: {result:?}"
        ))
    }
}

pub fn cleanup_deployment() -> Result<(), String> {
    configure_startup(false)?;
    remove_virtual_camera()?;
    run_elevated("--cleanup-elevated")
}

pub fn uninstall_deployment() -> Result<(), String> {
    configure_startup(false)?;
    remove_virtual_camera()
}

pub(crate) fn validate_release(payload: &[u8]) -> Result<(), String> {
    validate_native_architecture()?;
    validate_embedded_source(payload)
}

pub fn save_config_atomic(store: &ConfigStore, config: &AppConfig) -> std::io::Result<()> {
    store.save_with_replace(config, replace_file_atomic)
}

pub fn replace_file_atomic(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    let source: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<u16> = destination.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: both paths are terminated and remain live for the call.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::other)
}

/// Handles internal elevated deployment commands and ensures that the release
/// has extracted and registered its embedded, same-architecture DLL.
/// Returns true when the current process should exit without opening the UI.
pub fn deployment_startup(payload: &[u8]) -> Result<bool, String> {
    let argument = env::args().nth(1);
    match argument.as_deref() {
        Some("--register-elevated") => {
            validate_native_architecture()?;
            validate_embedded_source(payload)?;
            install(payload)?;
            Ok(true)
        }
        Some("--cleanup-elevated") => {
            cleanup_machine_deployment()?;
            Ok(true)
        }
        Some("--cleanup") => {
            cleanup_deployment()?;
            Ok(true)
        }
        Some("--startup" | "--show" | "--post-install" | "--post-update" | "--rollback") => {
            ensure_deployment(payload)?;
            Ok(false)
        }
        Some(_) => Ok(false),
        None if payload.is_empty() => Ok(false),
        None => {
            ensure_deployment(payload)?;
            Ok(false)
        }
    }
}

fn ensure_deployment(payload: &[u8]) -> Result<(), String> {
    if payload.is_empty() {
        return Ok(());
    }
    validate_native_architecture()?;
    validate_embedded_source(payload)?;
    let directory = deployment_directory()?;
    match machine_component_decision(&directory) {
        ComponentDecision::UseInstalled => return Ok(()),
        ComponentDecision::RejectNewerIncompatible => {
            return Err(incompatible_component_error(&directory));
        }
        ComponentDecision::InstallEmbedded => run_elevated("--register-elevated")?,
    }
    match machine_component_decision(&directory) {
        ComponentDecision::UseInstalled => Ok(()),
        ComponentDecision::RejectNewerIncompatible => Err(incompatible_component_error(&directory)),
        ComponentDecision::InstallEmbedded => {
            Err("camera source was not installed correctly".into())
        }
    }
}

struct RegisteredComponent {
    compatibility: ComponentCompatibility,
}

fn embedded_component_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("workspace version must be valid semver")
}

fn machine_component_decision(directory: &Path) -> ComponentDecision {
    let installed = registered_component(directory);
    decide_component(
        installed.as_ref().map(|component| &component.compatibility),
        &embedded_component_version(),
        IPC_PROTOCOL_VERSION,
    )
}

fn registered_component(directory: &Path) -> Option<RegisteredComponent> {
    let path = PathBuf::from(registry_string(SOURCE_INPROC_KEY, None)?);
    let threading_model = registry_string(SOURCE_INPROC_KEY, Some("ThreadingModel"))?;
    if !path.is_file()
        || !is_managed_source(&path, directory)
        || !threading_model
            .to_string_lossy()
            .eq_ignore_ascii_case("Both")
    {
        return None;
    }
    let version = registry_string(SOURCE_CLASS_KEY, Some(COMPONENT_VERSION_VALUE))?;
    let protocol_min = registry_string(SOURCE_CLASS_KEY, Some(PROTOCOL_MIN_VALUE))?;
    let protocol_max = registry_string(SOURCE_CLASS_KEY, Some(PROTOCOL_MAX_VALUE))?;
    Some(RegisteredComponent {
        compatibility: ComponentCompatibility {
            version: Version::parse(&version.to_string_lossy()).ok()?,
            protocol_min: protocol_min.to_string_lossy().parse().ok()?,
            protocol_max: protocol_max.to_string_lossy().parse().ok()?,
        },
    })
}

fn incompatible_component_error(directory: &Path) -> String {
    let version = registered_component(directory)
        .map(|component| component.compatibility.version.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!(
        "the installed StageSwap camera source ({version}) is newer but incompatible with this app; update StageSwap for this Windows user"
    )
}

fn registry_string(key: &str, value_name: Option<&str>) -> Option<std::ffi::OsString> {
    let key: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let value_name = value_name.map(|value| value.encode_utf16().chain([0]).collect::<Vec<_>>());
    let value_name = value_name
        .as_ref()
        .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr()));
    let mut bytes = 0;
    // SAFETY: the key is terminated and the first call only requests the value size.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            value_name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if status.is_err() || bytes < 2 {
        return None;
    }
    let mut value = vec![0_u16; (bytes as usize).div_ceil(size_of::<u16>())];
    // SAFETY: value has the byte capacity reported by RegGetValueW.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            value_name,
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if status.is_err() {
        return None;
    }
    let length = value
        .iter()
        .position(|word| *word == 0)
        .unwrap_or(value.len());
    Some(std::ffi::OsString::from_wide(&value[..length]))
}

fn validate_embedded_source(payload: &[u8]) -> Result<(), String> {
    if payload.get(..2) != Some(b"MZ") {
        return Err("embedded camera-source payload is not a PE file".into());
    }
    let pe_offset = payload
        .get(0x3c..0x40)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "embedded camera-source DOS header is truncated".to_string())?
        as usize;
    if payload.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
        return Err("embedded camera-source PE signature is missing".into());
    }
    let machine = payload
        .get(pe_offset + 4..pe_offset + 6)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| "embedded camera-source COFF header is truncated".to_string())?;
    let expected = IMAGE_FILE_MACHINE_AMD64.0;
    if machine != expected {
        return Err(format!(
            "embedded camera-source architecture 0x{machine:04x} does not match this executable"
        ));
    }
    Ok(())
}

fn validate_native_architecture() -> Result<(), String> {
    let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    // SAFETY: both machine outputs are writable and the pseudo-handle is valid.
    unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            Some(&mut native_machine),
        )
    }
    .map_err(|error| format!("could not validate native package architecture: {error}"))?;
    let package_machine = IMAGE_FILE_MACHINE_AMD64;
    if native_machine != package_machine || process_machine != IMAGE_FILE_MACHINE_UNKNOWN {
        return Err("this executable must run on matching native Windows architecture".into());
    }
    Ok(())
}

fn delete_registry_tree(root: HKEY, key: &str) -> Result<(), String> {
    let key: Vec<u16> = key.encode_utf16().chain([0]).collect();
    // SAFETY: the registry path is terminated for the duration of the call.
    let status = unsafe { RegDeleteTreeW(root, PCWSTR(key.as_ptr())) };
    if status.is_ok() || status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("could not remove registry key: {status:?}"))
    }
}

fn deployment_directory() -> Result<PathBuf, String> {
    env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .ok_or_else(|| "ProgramFiles is not defined".into())
        .map(|path| path.join(DEPLOYMENT_DIRECTORY_NAME))
}

fn payload_source_file_name(payload: &[u8]) -> String {
    // The filename changes with the payload so an in-use previous DLL never
    // needs to be overwritten. FNV-1a is sufficient here because the trusted
    // embedded payload, rather than untrusted input, selects the local name.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in payload {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{SOURCE_FILE_PREFIX}{hash:016x}.dll")
}

fn install(payload: &[u8]) -> Result<(), String> {
    let _deployment_lock = MachineDeploymentLock::acquire()?;
    let directory = deployment_directory()?;
    match machine_component_decision(&directory) {
        ComponentDecision::UseInstalled => return Ok(()),
        ComponentDecision::RejectNewerIncompatible => {
            return Err(incompatible_component_error(&directory));
        }
        ComponentDecision::InstallEmbedded => {}
    }
    fs::create_dir_all(&directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let source = directory.join(payload_source_file_name(payload));
    let staging = source.with_extension("dll.installing");
    let _ = fs::remove_file(&staging);
    let source_exists = source.exists();
    if source_exists {
        if fs::read(&source).ok().as_deref() != Some(payload) {
            return Err(format!(
                "camera source payload identity collision at {}",
                source.display()
            ));
        }
    } else {
        fs::write(&staging, payload)
            .map_err(|error| format!("could not stage {}: {error}", staging.display()))?;
        if let Err(error) = fs::rename(&staging, &source) {
            let _ = fs::remove_file(&staging);
            if fs::read(&source).ok().as_deref() != Some(payload) {
                return Err(format!("could not install {}: {error}", source.display()));
            }
        }
    }

    let previous_source = registered_managed_source(&directory);
    if let Err(error) = invoke_registration(&source, b"DllRegisterServer\0") {
        if let Some(previous) = previous_source
            && previous != source
        {
            let _ = invoke_registration(&previous, b"DllRegisterServer\0");
        }
        if !source_exists {
            let _ = fs::remove_file(&source);
        }
        return Err(error);
    }
    cleanup_managed_sources(&directory, Some(&source));
    Ok(())
}

fn cleanup_machine_deployment() -> Result<(), String> {
    let _deployment_lock = MachineDeploymentLock::acquire()?;
    let directory = deployment_directory()?;
    if let Some(source) = registered_managed_source(&directory) {
        let _ = invoke_registration(&source, b"DllUnregisterServer\0");
    }
    delete_registry_tree(HKEY_LOCAL_MACHINE, SOURCE_CLASS_KEY)?;
    cleanup_managed_sources(&directory, None);
    let _ = fs::remove_dir(&directory);
    Ok(())
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

struct MachineDeploymentLock(HANDLE);

impl MachineDeploymentLock {
    /// Serializes the Program Files payload and HKLM registration across users.
    fn acquire() -> Result<Self, String> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let sddl: Vec<u16> = "D:P(A;;GA;;;BA)(A;;GA;;;SY)"
            .encode_utf16()
            .chain([0])
            .collect();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|error| format!("could not secure machine deployment lock: {error}"))?;
        let descriptor = SecurityDescriptor(descriptor);
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: false.into(),
        };
        let name: Vec<u16> = MACHINE_DEPLOYMENT_MUTEX.encode_utf16().chain([0]).collect();
        let handle = unsafe { CreateMutexW(Some(&security), false, PCWSTR(name.as_ptr())) }
            .map_err(|error| format!("could not create machine deployment lock: {error}"))?;
        drop(descriptor);
        let wait = unsafe { WaitForSingleObject(handle, MACHINE_DEPLOYMENT_TIMEOUT_MS) };
        if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
            Ok(Self(handle))
        } else {
            let _ = unsafe { CloseHandle(handle) };
            if wait == WAIT_TIMEOUT {
                Err("another StageSwap machine deployment did not finish within 120 seconds".into())
            } else {
                let error = unsafe { GetLastError() };
                Err(format!(
                    "could not wait for the machine deployment lock: {error:?}"
                ))
            }
        }
    }
}

impl Drop for MachineDeploymentLock {
    fn drop(&mut self) {
        let _ = unsafe { ReleaseMutex(self.0) };
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn registered_managed_source(directory: &Path) -> Option<PathBuf> {
    let source = PathBuf::from(registry_string(SOURCE_INPROC_KEY, None)?);
    is_managed_source(&source, directory).then_some(source)
}

fn is_managed_source(source: &Path, directory: &Path) -> bool {
    let Some(parent) = source.parent() else {
        return false;
    };
    let Some(file_name) = source.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    parent
        .to_string_lossy()
        .eq_ignore_ascii_case(&directory.to_string_lossy())
        && (file_name.eq_ignore_ascii_case(SOURCE_FILE)
            || (file_name.len() > SOURCE_FILE_PREFIX.len() + 4
                && file_name
                    .get(..SOURCE_FILE_PREFIX.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(SOURCE_FILE_PREFIX))
                && file_name
                    .get(file_name.len() - 4..)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(".dll"))))
}

fn cleanup_managed_sources(directory: &Path, keep: Option<&Path>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep.is_some_and(|keep| {
            path.to_string_lossy()
                .eq_ignore_ascii_case(&keep.to_string_lossy())
        }) {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name
            .get(.."StageSwapSource".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("StageSwapSource"))
        {
            let _ = remove_or_schedule(&path);
        }
    }
}

fn remove_or_schedule(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {}
    }
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the source path is terminated, and a null destination requests
    // deletion at reboot after any process holding the old DLL has exited.
    unsafe {
        MoveFileExW(
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    }
    .map_err(|error| format!("could not remove or schedule camera source deletion: {error}"))
}

fn invoke_registration(path: &Path, export: &'static [u8]) -> Result<(), String> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the path and export name are terminated and remain live.
    let module = unsafe { LoadLibraryW(PCWSTR(path.as_ptr())) }
        .map_err(|error| format!("could not load camera source: {error}"))?;
    // SAFETY: module is live and export is a terminated static string.
    let address = unsafe { GetProcAddress(module, PCSTR(export.as_ptr())) };
    let Some(address) = address else {
        let _ = unsafe { FreeLibrary(module) };
        return Err("camera source registration export is missing".into());
    };
    type Registration = unsafe extern "system" fn() -> HRESULT;
    // SAFETY: the named DLL exports use this standard COM registration signature.
    let registration: Registration = unsafe { core::mem::transmute(address) };
    let result = unsafe { registration() };
    let _ = unsafe { FreeLibrary(module) };
    result
        .ok()
        .map_err(|error| format!("camera source registration failed: {error}"))
}

fn run_elevated(argument: &str) -> Result<(), String> {
    let executable =
        env::current_exe().map_err(|error| format!("could not locate executable: {error}"))?;
    let executable: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
    let argument: Vec<u16> = argument.encode_utf16().chain([0]).collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain([0]).collect();
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(executable.as_ptr()),
        lpParameters: PCWSTR(argument.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: all referenced buffers remain live until ShellExecuteExW returns.
    unsafe { ShellExecuteExW(&mut execute) }
        .map_err(|error| format!("deployment elevation was cancelled or failed: {error}"))?;
    if execute.hProcess.is_invalid() {
        return Err("elevated deployment process was not created".into());
    }
    // SAFETY: ShellExecuteExW returned an owned process handle.
    unsafe { WaitForSingleObject(execute.hProcess, INFINITE) };
    let mut code = 1;
    let status = unsafe { GetExitCodeProcess(execute.hProcess, &mut code) };
    let _ = unsafe { CloseHandle(execute.hProcess) };
    status.map_err(|error| format!("could not read deployment result: {error}"))?;
    if code != 0 {
        return Err(format!("elevated deployment failed with exit code {code}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_payload_source_names_are_stable_and_versioned() {
        assert_eq!(
            payload_source_file_name(b"test"),
            "StageSwapSource-f9e6e6ef197c2b25.dll"
        );
        assert_ne!(
            payload_source_file_name(b"first"),
            payload_source_file_name(b"second")
        );
    }

    #[test]
    fn native_deployment_identity_is_stageswap() {
        assert_eq!(DEPLOYMENT_DIRECTORY_NAME, "StageSwap");
        assert_eq!(RUN_VALUE, "StageSwap");
        assert_eq!(SOURCE_FILE, "StageSwapSource.dll");
        assert_eq!(SOURCE_FILE_PREFIX, "StageSwapSource-");
        assert!(SOURCE_CLASS_KEY.ends_with("{4ABA794D-7B23-449C-8467-CE74A41C2820}"));
        assert!(
            SOURCE_INPROC_KEY.ends_with("{4ABA794D-7B23-449C-8467-CE74A41C2820}\\InprocServer32")
        );
    }

    #[test]
    fn native_only_owned_dll_names_are_managed() {
        let directory = Path::new(r"C:\Program Files\StageSwap");
        assert!(is_managed_source(&directory.join(SOURCE_FILE), directory));
        assert!(is_managed_source(
            &directory.join("StageSwapSource-0123456789abcdef.dll"),
            directory
        ));
        assert!(!is_managed_source(
            &directory.join("StageSwapSource-0123456789abcdef.dll.installing"),
            directory
        ));
        assert!(!is_managed_source(
            Path::new(r"C:\Windows\System32\StageSwapSource.dll"),
            directory
        ));
    }

    #[test]
    fn native_automatic_screen_camera_paths_are_not_owned() {
        let directory = Path::new(r"C:\Program Files\StageSwap");
        assert!(!is_managed_source(
            Path::new(
                r"C:\Program Files\Automatic Screen Camera Rust Portable\AutomaticScreenCameraSource-0123456789abcdef.dll"
            ),
            directory
        ));
    }

    #[test]
    fn native_startup_command_is_quoted_and_status_comparison_is_case_insensitive() {
        let expected = startup_command(Path::new(r"C:\Program Files\StageSwap\StageSwap.exe"));
        assert_eq!(
            expected,
            r#""C:\Program Files\StageSwap\StageSwap.exe" --startup"#
        );
        assert_eq!(
            classify_startup_command(expected.to_ascii_uppercase(), &expected),
            StartupRegistrationStatus::Current
        );
        assert_eq!(
            classify_startup_command("StageSwap.exe --show".into(), &expected),
            StartupRegistrationStatus::Different("StageSwap.exe --show".into())
        );
    }
}
