use crate::instance_scope::application_mutex_name;
use crate::user_identity::current_user_sid;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows_core::PCWSTR;

pub struct SingleInstance(HANDLE);

impl SingleInstance {
    pub fn acquire() -> Result<Option<Self>, String> {
        // Account scope matches the per-user virtual camera and frame pipe across sessions.
        let name = application_mutex_name(&current_user_sid()?);
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|error| format!("could not create application instance lock: {error}"))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }

    pub fn exists() -> Result<bool, String> {
        Ok(Self::acquire()?.is_none())
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}
