use crate::user_identity::current_user_sid;
use core::ffi::c_void;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use windows::Win32::Foundation::E_NOTIMPL;
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncCallback_Impl, IMFAsyncResult, IMFMediaEvent, IMFVirtualCamera,
    MEError, MEExtendedType, MF_E_INVALIDREQUEST,
    MF_FRAMESERVER_VCAMEVENT_EXTENDED_PIPELINE_SHUTDOWN, MF_VERSION, MFSTARTUP_FULL, MFShutdown,
    MFStartup, MFVirtualCameraAccess, MFVirtualCameraAccess_CurrentUser, MFVirtualCameraLifetime,
    MFVirtualCameraLifetime_System, MFVirtualCameraType, MFVirtualCameraType_SoftwareCameraSource,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_core::{Error, GUID, HRESULT, Interface, PCSTR, PCWSTR, Ref, implement};

const SOURCE_ID: &str = "{4ABA794D-7B23-449C-8467-CE74A41C2820}";
const VIRTUAL_CAMERA_NAME: &str = "Stageswap Camera";
const PIPE_ATTRIBUTE: GUID = GUID::from_u128(0x75c753a0_587b_4064_bb77_f0171fcd4ad7);
const PIPE_NAME_PREFIX: &str = r"\\.\pipe\StageSwap.FinalFrame.";

type CreateVirtualCamera = unsafe extern "system" fn(
    MFVirtualCameraType,
    MFVirtualCameraLifetime,
    MFVirtualCameraAccess,
    PCWSTR,
    PCWSTR,
    *const GUID,
    u32,
    *mut *mut c_void,
) -> HRESULT;

fn virtual_camera_factory() -> Result<CreateVirtualCamera, String> {
    static FACTORY: OnceLock<Result<CreateVirtualCamera, String>> = OnceLock::new();
    FACTORY
        .get_or_init(|| {
            let library_name = wide("mfsensorgroup.dll");
            // The module is intentionally retained for the process lifetime because
            // returned camera objects own vtables from it. OnceLock limits this to
            // one LoadLibrary reference instead of leaking one on every restart.
            let library = unsafe { LoadLibraryW(PCWSTR(library_name.as_ptr())) }
                .map_err(|error| format!("Windows virtual cameras are unavailable: {error}"))?;
            // SAFETY: the symbol has the signature published for MFCreateVirtualCamera.
            Ok(unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateVirtualCamera>(
                    GetProcAddress(library, PCSTR(c"MFCreateVirtualCamera".as_ptr().cast()))
                        .ok_or("Windows 11 virtual camera support is not installed")?,
                )
            })
        })
        .as_ref()
        .copied()
        .map_err(Clone::clone)
}

fn create_virtual_camera(
    friendly_name: PCWSTR,
    source_id: PCWSTR,
) -> Result<IMFVirtualCamera, String> {
    let create = virtual_camera_factory()?;
    let mut camera = std::ptr::null_mut();
    let result = unsafe {
        create(
            MFVirtualCameraType_SoftwareCameraSource,
            MFVirtualCameraLifetime_System,
            MFVirtualCameraAccess_CurrentUser,
            friendly_name,
            source_id,
            std::ptr::null(),
            0,
            &mut camera,
        )
    };
    result
        .ok()
        .map_err(|error| format!("could not create virtual camera: {error}"))?;
    if camera.is_null() {
        return Err("Windows returned an empty virtual camera controller".into());
    }
    // SAFETY: a successful call returned ownership of an IMFVirtualCamera pointer.
    Ok(unsafe { IMFVirtualCamera::from_raw(camera) })
}

pub fn frame_pipe_name() -> Result<String, String> {
    current_user_sid().map(|sid| format!("{PIPE_NAME_PREFIX}{sid}"))
}

#[implement(IMFAsyncCallback)]
struct CameraCallback {
    running: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    expected_generation: u64,
}

impl IMFAsyncCallback_Impl for CameraCallback_Impl {
    fn GetParameters(&self, _flags: *mut u32, _queue: *mut u32) -> windows::core::Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn Invoke(&self, result: Ref<IMFAsyncResult>) -> windows::core::Result<()> {
        if self.generation.load(Ordering::Acquire) != self.expected_generation {
            return Ok(());
        }
        let failed = result.as_ref().is_none_or(|result| {
            // SAFETY: the callback supplies a live async result for this invocation.
            if unsafe { result.GetStatus() }.is_err() {
                return true;
            }
            // SAFETY: any event object remains owned for the duration of this check.
            unsafe { result.GetObject() }
                .ok()
                .and_then(|object| object.cast::<IMFMediaEvent>().ok())
                .is_some_and(|event| camera_event_failed(&event))
        });
        if failed {
            self.running.store(false, Ordering::Release);
        }
        Ok(())
    }
}

fn camera_event_failed(event: &IMFMediaEvent) -> bool {
    // SAFETY: the event remains live for these read-only queries.
    unsafe {
        event.GetType().is_ok_and(|kind| kind == MEError.0 as u32)
            || event.GetStatus().is_ok_and(|status| status.is_err())
            || (event
                .GetType()
                .is_ok_and(|kind| kind == MEExtendedType.0 as u32)
                && event.GetExtendedType().is_ok_and(|extended| {
                    extended == MF_FRAMESERVER_VCAMEVENT_EXTENDED_PIPELINE_SHUTDOWN
                }))
    }
}

/// Owns COM and Media Foundation on one runtime thread and controls the
/// system-lifetime virtual camera registration. Dropping it deliberately
/// releases the controller without calling `Stop`, preserving off-screen
/// output for existing Frame Server sessions.
pub struct VirtualCameraController {
    camera: Option<IMFVirtualCamera>,
    callback: Option<IMFAsyncCallback>,
    running: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    pipe_name: String,
    mf_started: bool,
    com_initialized: bool,
}

impl VirtualCameraController {
    pub fn start(pipe_name: String) -> Result<Self, String> {
        // SAFETY: this constructor and Drop run on the owning runtime thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| format!("could not initialize COM: {error}"))?;
        // SAFETY: balanced by MFShutdown in Drop.
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            // SAFETY: CoInitializeEx succeeded on this thread.
            unsafe { CoUninitialize() };
            return Err(format!("could not initialize Media Foundation: {error}"));
        }
        let mut controller = Self {
            camera: None,
            callback: None,
            running: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            pipe_name,
            mf_started: true,
            com_initialized: true,
        };
        controller.open()?;
        Ok(controller)
    }

    fn open(&mut self) -> Result<(), String> {
        let friendly = wide(VIRTUAL_CAMERA_NAME);
        let source = wide(SOURCE_ID);
        let camera = create_virtual_camera(PCWSTR(friendly.as_ptr()), PCWSTR(source.as_ptr()))?;
        let pipe = wide(&self.pipe_name);
        // SAFETY: attributes and terminated string are valid for each call.
        (|| -> windows_core::Result<()> {
            // SAFETY: attributes and terminated string are valid for each call.
            unsafe {
                camera.SetString(&PIPE_ATTRIBUTE, PCWSTR(pipe.as_ptr()))?;
            }
            Ok(())
        })()
        .map_err(|error| format!("could not configure virtual camera: {error}"))?;
        let expected_generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let callback: IMFAsyncCallback = CameraCallback {
            running: Arc::clone(&self.running),
            generation: Arc::clone(&self.generation),
            expected_generation,
        }
        .into();
        self.running.store(true, Ordering::Release);
        // SAFETY: callback remains owned by this controller while the camera runs.
        if let Err(error) = unsafe { camera.Start(&callback) } {
            self.running.store(false, Ordering::Release);
            return Err(format!("could not start virtual camera: {error}"));
        }
        self.callback = Some(callback);
        self.camera = Some(camera);
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn restart(&mut self) -> Result<(), String> {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.running.store(false, Ordering::Release);
        self.callback = None;
        self.camera = None;
        self.open()
    }
}

impl Drop for VirtualCameraController {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.running.store(false, Ordering::Release);
        self.callback = None;
        self.camera = None;
        if self.mf_started {
            // SAFETY: balanced with this thread's successful MFStartup.
            let _ = unsafe { MFShutdown() };
        }
        if self.com_initialized {
            // SAFETY: balanced with this thread's successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

pub fn remove_virtual_camera() -> Result<(), String> {
    remove_virtual_camera_for_source(SOURCE_ID)
}

fn remove_virtual_camera_for_source(source_id: &str) -> Result<(), String> {
    // This cleanup entry point is expected to run in its own short-lived process.
    // SAFETY: all initialization and shutdown calls are balanced on this thread.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|error| format!("could not initialize COM: {error}"))?;
    if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
        unsafe { CoUninitialize() };
        return Err(format!("could not initialize Media Foundation: {error}"));
    }
    let result = (|| {
        let friendly = wide(VIRTUAL_CAMERA_NAME);
        let source = wide(source_id);
        let camera = create_virtual_camera(PCWSTR(friendly.as_ptr()), PCWSTR(source.as_ptr()))
            .map_err(|error| format!("could not open virtual camera registration: {error}"))?;
        (|| -> windows_core::Result<()> {
            // SAFETY: camera is a live controller created on this thread.
            unsafe {
                if let Err(error) = camera.Remove()
                    && !virtual_camera_registration_is_absent(&error)
                {
                    return Err(error);
                }
                camera.Shutdown()?;
            }
            Ok(())
        })()
        .map_err(|error| format!("could not remove virtual camera registration: {error}"))
    })();
    let _ = unsafe { MFShutdown() };
    unsafe { CoUninitialize() };
    result
}

fn virtual_camera_registration_is_absent(error: &windows_core::Error) -> bool {
    // MFCreateVirtualCamera also returns a controller when this identity has not
    // been registered. Remove reports that state as MF_E_INVALIDREQUEST, so
    // cleanup must treat it as the idempotent "already removed" case.
    error.code() == MF_E_INVALIDREQUEST
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::Media::MediaFoundation::MFCreateMediaEvent;

    struct MediaFoundation;

    impl Drop for MediaFoundation {
        fn drop(&mut self) {
            // SAFETY: balances the successful MFStartup in this test.
            let _ = unsafe { MFShutdown() };
        }
    }

    #[test]
    fn native_pipeline_shutdown_extended_event_marks_camera_failed() -> windows_core::Result<()> {
        // SAFETY: initializes Media Foundation for event construction.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        // SAFETY: the event constructor copies the live extended-type GUID.
        let event = unsafe {
            MFCreateMediaEvent(
                MEExtendedType.0 as u32,
                &MF_FRAMESERVER_VCAMEVENT_EXTENDED_PIPELINE_SHUTDOWN,
                S_OK,
                None,
            )?
        };
        assert!(camera_event_failed(&event));
        Ok(())
    }

    #[test]
    fn native_missing_virtual_camera_registration_is_already_removed() {
        let error = Error::from_hresult(MF_E_INVALIDREQUEST);
        assert!(virtual_camera_registration_is_absent(&error));
        assert!(!virtual_camera_registration_is_absent(
            &Error::from_hresult(windows::Win32::Foundation::E_ACCESSDENIED,)
        ));
    }

    #[test]
    fn native_virtual_camera_identity_is_stageswap() {
        assert_eq!(SOURCE_ID, "{4ABA794D-7B23-449C-8467-CE74A41C2820}");
        assert_eq!(
            PIPE_ATTRIBUTE,
            GUID::from_u128(0x75c753a0_587b_4064_bb77_f0171fcd4ad7)
        );
        assert_eq!(PIPE_NAME_PREFIX, r"\\.\pipe\StageSwap.FinalFrame.");
    }
}
