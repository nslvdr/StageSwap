use crate::instance_scope::control_pipe_name;
use crate::user_identity::current_user_sid;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_PIPE_BUSY, GENERIC_READ, GENERIC_WRITE, GetLastError,
    HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows::Win32::System::IO::CancelSynchronousIo;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows::core::{BOOL, PCWSTR};

const PROTOCOL_VERSION: u8 = 1;
const RESPONSE_OK: u8 = 1;
const RESPONSE_READY: u8 = 2;
const RESPONSE_STARTING: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceCommand {
    Show,
    ShutdownForReplacement,
}

impl InstanceCommand {
    fn code(self) -> u8 {
        match self {
            Self::Show => 1,
            Self::ShutdownForReplacement => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceStatus {
    Starting,
    Ready,
}

pub struct InstanceControl {
    ready: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct InstanceReadiness(Arc<AtomicBool>);

impl InstanceReadiness {
    pub fn mark_ready(&self) {
        self.0.store(true, Ordering::Release);
    }
}

impl InstanceControl {
    pub fn start(commands: mpsc::Sender<InstanceCommand>) -> Result<Self, String> {
        Self::start_at_path(control_pipe_name(&current_user_sid()?), commands)
    }

    fn start_at_path(
        path: String,
        commands: mpsc::Sender<InstanceCommand>,
    ) -> Result<Self, String> {
        let ready = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_ready = Arc::clone(&ready);
        let worker_stop = Arc::clone(&stop);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("stageswap-instance-control".into())
            .spawn(move || {
                let result = server_loop(
                    &path,
                    &commands,
                    &worker_ready,
                    &worker_stop,
                    &startup_sender,
                );
                let _ = startup_sender.try_send(result);
            })
            .map_err(|error| format!("could not start instance control: {error}"))?;
        match startup_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                ready,
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err("instance control stopped during startup".into())
            }
        }
    }

    pub fn readiness(&self) -> InstanceReadiness {
        InstanceReadiness(Arc::clone(&self.ready))
    }
}

impl Drop for InstanceControl {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            // Cancellation affects only pending I/O. Keep canceling until the
            // worker exits, including operations begun just after the first call.
            while !worker.is_finished() {
                // SAFETY: cancellation is limited to this live worker's pipe calls.
                let _ = unsafe { CancelSynchronousIo(HANDLE(worker.as_raw_handle())) };
                thread::sleep(std::time::Duration::from_millis(10));
            }
            let _ = worker.join();
        }
    }
}

pub fn send_instance_command(command: InstanceCommand) -> Result<(), String> {
    let response = transact(command.code())?;
    if response == RESPONSE_OK {
        Ok(())
    } else {
        Err("running StageSwap returned an invalid control response".into())
    }
}

pub fn instance_status() -> Result<InstanceStatus, String> {
    match transact(3)? {
        RESPONSE_READY => Ok(InstanceStatus::Ready),
        RESPONSE_STARTING => Ok(InstanceStatus::Starting),
        _ => Err("running StageSwap returned an invalid status response".into()),
    }
}

fn transact(command: u8) -> Result<u8, String> {
    let path = wide(&control_pipe_name(&current_user_sid()?));
    // Give a busy server a short opportunity to finish the preceding request.
    let _ = unsafe { WaitNamedPipeW(PCWSTR(path.as_ptr()), 1_000) };
    // SAFETY: the path is terminated and the returned handle is owned below.
    let pipe = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            windows::Win32::Storage::FileSystem::FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|error| format!("could not contact the running StageSwap instance: {error}"))?;
    let pipe = OwnedHandle(pipe);
    let request = [PROTOCOL_VERSION, command];
    write_exact(pipe.0, &request)?;
    let mut response = [0_u8; 2];
    read_exact(pipe.0, &mut response)?;
    if response[0] != PROTOCOL_VERSION {
        return Err("running StageSwap uses an incompatible control protocol".into());
    }
    Ok(response[1])
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

fn server_loop(
    path: &str,
    commands: &mpsc::Sender<InstanceCommand>,
    ready: &AtomicBool,
    stop: &AtomicBool,
    startup: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let sddl = wide("D:P(A;;GA;;;OW)");
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|error| format!("could not secure instance control: {error}"))?;
    let _descriptor = SecurityDescriptor(descriptor);
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: BOOL(0),
    };
    let path = wide(path);
    let mut startup = Some(startup);
    while !stop.load(Ordering::Acquire) {
        let raw = unsafe {
            CreateNamedPipeW(
                PCWSTR(path.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                16,
                16,
                0,
                Some(&security),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };
            if error == ERROR_PIPE_BUSY {
                return Err("another StageSwap instance owns the control pipe".into());
            }
            return Err(format!("could not create instance control: {error:?}"));
        }
        let pipe = OwnedHandle(raw);
        if let Some(startup) = startup.take() {
            startup
                .send(Ok(()))
                .map_err(|_| "instance-control startup receiver was dropped".to_string())?;
        }
        let connected = unsafe { ConnectNamedPipe(pipe.0, None) }.is_ok()
            || unsafe { GetLastError() } == windows::Win32::Foundation::ERROR_PIPE_CONNECTED;
        if !connected {
            continue;
        }
        let mut request = [0_u8; 2];
        let response = if read_exact(pipe.0, &mut request).is_ok() && request[0] == PROTOCOL_VERSION
        {
            match request[1] {
                1 => {
                    let _ = commands.send(InstanceCommand::Show);
                    RESPONSE_OK
                }
                2 => {
                    let _ = commands.send(InstanceCommand::ShutdownForReplacement);
                    RESPONSE_OK
                }
                3 if ready.load(Ordering::Acquire) => RESPONSE_READY,
                3 => RESPONSE_STARTING,
                _ => 0,
            }
        } else {
            0
        };
        let _ = write_exact(pipe.0, &[PROTOCOL_VERSION, response]);
        let _ = unsafe { DisconnectNamedPipe(pipe.0) };
    }
    Ok(())
}

fn write_exact(handle: HANDLE, mut bytes: &[u8]) -> Result<(), String> {
    while !bytes.is_empty() {
        let mut written = 0;
        unsafe { WriteFile(handle, Some(bytes), Some(&mut written), None) }
            .map_err(|error| format!("instance-control write failed: {error}"))?;
        if written == 0 {
            return Err("instance-control pipe closed while writing".into());
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

fn read_exact(handle: HANDLE, mut bytes: &mut [u8]) -> Result<(), String> {
    while !bytes.is_empty() {
        let mut read = 0;
        if let Err(error) = unsafe { ReadFile(handle, Some(bytes), Some(&mut read), None) } {
            if error.code() == ERROR_BROKEN_PIPE.to_hresult() {
                return Err("instance-control pipe closed while reading".into());
            }
            return Err(format!("instance-control read failed: {error}"));
        }
        if read == 0 {
            return Err("instance-control pipe closed while reading".into());
        }
        let (_, remainder) = bytes.split_at_mut(read as usize);
        bytes = remainder;
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_control_shutdown_releases_idle_and_partial_request_connections() {
        let (done, completion) = mpsc::sync_channel(1);
        thread::spawn(move || {
            for scenario in 0..3 {
                let name = format!(
                    r"\\.\pipe\StageSwap.ControlShutdown.{}.{}",
                    std::process::id(),
                    scenario
                );
                let (sender, _receiver) = mpsc::channel();
                let control = InstanceControl::start_at_path(name.clone(), sender).unwrap();
                let mut client = (scenario != 0).then(|| {
                    std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&name)
                        .unwrap()
                });
                if scenario == 2 {
                    std::io::Write::write_all(client.as_mut().unwrap(), &[PROTOCOL_VERSION])
                        .unwrap();
                }
                drop(control);
                drop(client);
            }
            done.send(()).unwrap();
        });
        completion
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    }

    #[test]
    fn native_control_identity_and_protocol_are_stable() {
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(InstanceCommand::Show.code(), 1);
        assert_eq!(InstanceCommand::ShutdownForReplacement.code(), 2);
    }
}
