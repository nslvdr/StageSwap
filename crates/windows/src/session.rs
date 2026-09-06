use windows::Win32::System::RemoteDesktop::{
    WTS_CONNECTSTATE_CLASS, WTS_CURRENT_SESSION, WTSActive, WTSConnectState, WTSFreeMemory,
    WTSQuerySessionInformationW,
};
use windows_core::PWSTR;

/// Queries the calling process's session, including when another user owns the console.
pub fn current_session_active() -> Result<bool, String> {
    let mut buffer = PWSTR::null();
    let mut bytes = 0;
    // SAFETY: both output pointers are live; None selects the local server.
    unsafe {
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSConnectState,
            &mut buffer,
            &mut bytes,
        )
    }
    .map_err(|error| format!("could not query Windows session state: {error}"))?;
    let result = if buffer.is_null() || (bytes as usize) < size_of::<WTS_CONNECTSTATE_CLASS>() {
        Err("Windows returned an invalid session state".into())
    } else {
        // SAFETY: WTSConnectState returns a WTS_CONNECTSTATE_CLASS of the checked size.
        let state = unsafe { buffer.0.cast::<WTS_CONNECTSTATE_CLASS>().read_unaligned() };
        Ok(state == WTSActive)
    };
    // SAFETY: the query allocated this buffer; WTSFreeMemory releases it once.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    result
}
