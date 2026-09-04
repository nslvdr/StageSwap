use core::ffi::c_void;
use stageswap_core::IPC_PROTOCOL_VERSION;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, ERROR_FILE_NOT_FOUND,
    ERROR_SUCCESS, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_core::{BOOL, Error, GUID, HRESULT, IUnknown, Interface, PCWSTR, Ref, implement};

mod activation;
mod diagnostics;
mod media_source;
mod media_stream;
mod pipe_reader;
use activation::Activation;

const SOURCE_CLSID: GUID = GUID::from_u128(0x4aba794d_7b23_449c_8467_ce74a41c2820);
pub(super) static OBJECTS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
pub(super) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static SERVER_LOCKS: AtomicUsize = AtomicUsize::new(0);
static MODULE_HANDLE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

const CLASS_KEY: &str = r"Software\Classes\CLSID\{4ABA794D-7B23-449C-8467-CE74A41C2820}";
const COMPONENT_VERSION_VALUE: &str = "StageSwapComponentVersion";
const PROTOCOL_MIN_VALUE: &str = "StageSwapProtocolMin";
const PROTOCOL_MAX_VALUE: &str = "StageSwapProtocolMax";

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns a key returned by RegCreateKeyExW.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn wide_bytes(value: &[u16]) -> &[u8] {
    // SAFETY: u16 slices have a valid byte representation and the returned
    // slice cannot outlive the input.
    unsafe { core::slice::from_raw_parts(value.as_ptr().cast::<u8>(), size_of_val(value)) }
}

fn win32_hresult(status: windows::Win32::Foundation::WIN32_ERROR) -> HRESULT {
    HRESULT::from_win32(status.0)
}

fn create_key(parent: HKEY, name: &str) -> Result<RegistryKey, HRESULT> {
    let name = wide(name);
    let mut key = HKEY::default();
    // SAFETY: all pointers refer to live buffers and the output is writable.
    let status = unsafe {
        RegCreateKeyExW(
            parent,
            PCWSTR(name.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(RegistryKey(key))
    } else {
        Err(win32_hresult(status))
    }
}

fn set_string(key: HKEY, name: Option<&str>, value: &str) -> Result<(), HRESULT> {
    let name = name.map(wide);
    let value = wide(value);
    // SAFETY: the optional value-name and data buffers remain live for the call.
    let status = unsafe {
        RegSetValueExW(
            key,
            name.as_ref()
                .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr())),
            None,
            REG_SZ,
            Some(wide_bytes(&value)),
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win32_hresult(status))
    }
}

fn register_server() -> Result<(), HRESULT> {
    let module = MODULE_HANDLE.load(Ordering::Acquire);
    if module.is_null() {
        return Err(E_FAIL);
    }
    let mut path = [0u16; 32_768];
    // SAFETY: DllMain stored this process module and the output buffer is writable.
    let length = unsafe { GetModuleFileNameW(Some(HMODULE(module)), &mut path) } as usize;
    if length == 0 || length >= path.len() {
        return Err(E_FAIL);
    }
    let path = String::from_utf16(&path[..length]).map_err(|_| E_FAIL)?;
    let class = create_key(HKEY_LOCAL_MACHINE, CLASS_KEY)?;
    if let Err(error) = (|| {
        set_string(class.0, None, "StageSwap Media Source")?;
        set_string(
            class.0,
            Some(COMPONENT_VERSION_VALUE),
            env!("CARGO_PKG_VERSION"),
        )?;
        let protocol = IPC_PROTOCOL_VERSION.to_string();
        set_string(class.0, Some(PROTOCOL_MIN_VALUE), &protocol)?;
        set_string(class.0, Some(PROTOCOL_MAX_VALUE), &protocol)?;
        let server = create_key(class.0, "InprocServer32")?;
        set_string(server.0, None, &path)?;
        set_string(server.0, Some("ThreadingModel"), "Both")
    })() {
        drop(class);
        let key = wide(CLASS_KEY);
        // SAFETY: key is a terminated UTF-16 string.
        let _ = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr())) };
        return Err(error);
    }
    Ok(())
}

fn unregister_server() -> Result<(), HRESULT> {
    let key = wide(CLASS_KEY);
    // SAFETY: key is a terminated UTF-16 string.
    let status = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr())) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(win32_hresult(status))
    }
}

#[implement(IClassFactory)]
struct ClassFactory;

impl ClassFactory {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for ClassFactory {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Release);
    }
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        interface_id: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if object.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        // SAFETY: COM guarantees a writable out pointer after the null check.
        unsafe { object.write(core::ptr::null_mut()) };
        if !outer.is_null() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let activation = Activation::new()?;
        let activation: windows::Win32::Media::MediaFoundation::IMFActivate = activation.into();
        // SAFETY: the COM caller supplied the requested IID and writable out pointer.
        unsafe { activation.query(interface_id, object) }.ok()
    }

    fn LockServer(&self, lock: BOOL) -> windows::core::Result<()> {
        if lock.as_bool() {
            SERVER_LOCKS.fetch_add(1, Ordering::Relaxed);
        } else {
            SERVER_LOCKS
                .fetch_update(Ordering::Release, Ordering::Relaxed, |count| {
                    count.checked_sub(1)
                })
                .ok();
        }
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class_id: *const GUID,
    interface_id: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if class_id.is_null() || interface_id.is_null() || object.is_null() {
        return E_POINTER;
    }
    // SAFETY: all pointers were checked, and COM supplies GUID inputs plus a
    // writable interface output pointer for this standard DLL export.
    unsafe { object.write(core::ptr::null_mut()) };
    // SAFETY: class_id points to a GUID for the duration of this call.
    if unsafe { *class_id } != SOURCE_CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = ClassFactory::new().into();
    // SAFETY: interface_id and object satisfy Interface::query's COM contract.
    unsafe { factory.query(interface_id, object) }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::Acquire) == 0 && SERVER_LOCKS.load(Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    instance: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE_HANDLE.store(instance.0, Ordering::Release);
        // SAFETY: the handle is the DLL instance supplied by the loader.
        let _ = unsafe { DisableThreadLibraryCalls(HMODULE(instance.0)) };
    }
    BOOL(1)
}

#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    register_server().map_or_else(|error| error, |()| S_OK)
}

#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    unregister_server().map_or_else(|error| error, |()| S_OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::KernelStreaming::IKsControl;
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFGetService, IMFMediaSourceEx, MF_VERSION, MFMEDIASOURCE_IS_LIVE,
        MFSTARTUP_FULL, MFShutdown, MFStartup,
    };

    struct MediaFoundation;

    impl Drop for MediaFoundation {
        fn drop(&mut self) {
            // SAFETY: balances the successful MFStartup in this test.
            let _ = unsafe { MFShutdown() };
        }
    }

    #[test]
    fn native_exported_class_factory_activates_all_required_source_interfaces()
    -> windows_core::Result<()> {
        let _test_lock = TEST_LOCK.lock().expect("media-source test lock poisoned");
        // SAFETY: initializes Media Foundation for the activation test.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        let mut raw = core::ptr::null_mut();
        // SAFETY: all GUID inputs and the output slot remain valid for the call.
        unsafe { DllGetClassObject(&SOURCE_CLSID, &IClassFactory::IID, &mut raw) }.ok()?;
        // SAFETY: DllGetClassObject returned an owned IClassFactory pointer.
        let factory = unsafe { IClassFactory::from_raw(raw) };
        // SAFETY: the factory creates a non-aggregated owned activation object.
        let activation: IMFActivate = unsafe { factory.CreateInstance(None::<&IUnknown>)? };
        // SAFETY: ActivateObject returns an owned interface implemented by MediaSource.
        let source: IMFMediaSourceEx = unsafe { activation.ActivateObject()? };
        let _: IMFGetService = source.cast()?;
        let _: IKsControl = source.cast()?;
        // The virtual-camera Frame Server can invoke this during registration.
        // It must not invalidate an interface the Frame Server still owns.
        unsafe { activation.ShutdownObject()? };
        assert_eq!(
            unsafe { source.GetCharacteristics()? },
            MFMEDIASOURCE_IS_LIVE.0 as u32
        );
        // SAFETY: source is live and shutdown is part of its public state contract.
        unsafe { source.Shutdown()? };
        drop(source);
        // SAFETY: releases the activation object's retained source reference.
        unsafe { activation.DetachObject()? };
        drop(activation);
        drop(factory);
        assert_eq!(DllCanUnloadNow(), S_OK);
        Ok(())
    }

    #[test]
    fn native_releasing_activation_does_not_shutdown_activated_source() -> windows_core::Result<()>
    {
        let _test_lock = TEST_LOCK.lock().expect("media-source test lock poisoned");
        // SAFETY: initializes Media Foundation for the activation test.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        let mut raw = core::ptr::null_mut();
        // SAFETY: all GUID inputs and the output slot remain valid for the call.
        unsafe { DllGetClassObject(&SOURCE_CLSID, &IClassFactory::IID, &mut raw) }.ok()?;
        // SAFETY: DllGetClassObject returned an owned IClassFactory pointer.
        let factory = unsafe { IClassFactory::from_raw(raw) };
        // SAFETY: the factory creates a non-aggregated owned activation object.
        let activation: IMFActivate = unsafe { factory.CreateInstance(None::<&IUnknown>)? };
        // SAFETY: ActivateObject returns an owned interface implemented by MediaSource.
        let source: IMFMediaSourceEx = unsafe { activation.ActivateObject()? };

        // Windows can release IMFActivate while IMFVirtualCamera::Start continues
        // using the activated source. The source must remain usable in that case.
        drop(activation);
        assert_eq!(
            unsafe { source.GetCharacteristics()? },
            MFMEDIASOURCE_IS_LIVE.0 as u32
        );

        // SAFETY: source is live and shutdown breaks its source/stream ownership cycle.
        unsafe { source.Shutdown()? };
        drop(source);
        drop(factory);
        assert_eq!(DllCanUnloadNow(), S_OK);
        Ok(())
    }
}
