use std::ffi::c_void;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_core::PWSTR;

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns a process-token handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Returns the stable SID for the Windows account owning the current process.
pub(crate) fn current_user_sid() -> Result<String, String> {
    let mut token = HANDLE::default();
    // SAFETY: token points to writable storage and the process pseudo-handle is valid.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|error| format!("could not open process token: {error}"))?;
    let token = OwnedHandle(token);
    let mut bytes = 0;
    // The first call is expected to fail with insufficient buffer and returns the size.
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut bytes) };
    if bytes < size_of::<TOKEN_USER>() as u32 {
        return Err("process token did not contain a user SID".into());
    }
    let words = (bytes as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    // SAFETY: the usize allocation provides TOKEN_USER alignment and sufficient bytes.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast::<c_void>()),
            bytes,
            &mut bytes,
        )
    }
    .map_err(|error| format!("could not read process user SID: {error}"))?;
    // SAFETY: GetTokenInformation initialized TOKEN_USER at the buffer start.
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = PWSTR::null();
    // SAFETY: the token buffer owns a valid SID for the duration of this call.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) }
        .map_err(|error| format!("could not format process user SID: {error}"))?;
    // SAFETY: the conversion returned a terminated LocalAlloc string.
    let value = unsafe { sid.to_string() }
        .map_err(|error| format!("could not decode process user SID: {error}"));
    // SAFETY: ConvertSidToStringSidW allocated this pointer with LocalAlloc.
    let _ = unsafe { LocalFree(Some(HLOCAL(sid.0.cast()))) };
    value
}
