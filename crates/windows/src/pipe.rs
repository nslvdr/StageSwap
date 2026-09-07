use stageswap_core::{
    Frame, FrameHeader, HEADER_LEN, MAX_FRAME_BYTES, is_expected_pipe_disconnect,
};
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{PIPE_ACCESS_OUTBOUND, WriteFile};
use windows::Win32::System::IO::CancelSynchronousIo;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::core::{BOOL, PCWSTR};

struct Latest {
    sequence: u64,
    header: [u8; HEADER_LEN],
    pixels: Arc<[u8]>,
}

impl Default for Latest {
    fn default() -> Self {
        Self {
            sequence: 0,
            header: [0; HEADER_LEN],
            pixels: Arc::from([]),
        }
    }
}

struct Shared {
    latest: Mutex<Latest>,
    changed: Condvar,
    stop: AtomicBool,
    failure: Mutex<Option<String>>,
    published_sequence: AtomicU64,
    connected: AtomicBool,
    transmitted_sequence: AtomicU64,
    write_failures: AtomicU64,
    disconnect_count: AtomicU64,
    last_write_error: AtomicU32,
    last_disconnect_error: AtomicU32,
    connection_count: AtomicU64,
    connection_failures: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FramePublisherDiagnostics {
    pub published_sequence: u64,
    pub transmitted_sequence: u64,
    pub connected: bool,
    pub write_failures: u64,
    pub disconnect_count: u64,
    pub last_write_error: Option<u32>,
    pub last_disconnect_error: Option<u32>,
    pub connection_count: u64,
    pub connection_failures: u64,
}

/// Single-client, bounded-memory publisher used by the out-of-process Media
/// Foundation source. Publishing replaces the previous frame rather than
/// queueing, so a slow consumer cannot make the application accumulate frames.
pub struct FramePublisher {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

/// Cloneable, non-owning publication endpoint. The device thread retains the
/// owning [`FramePublisher`] and therefore controls startup and shutdown; the
/// runtime can only replace the latest frame.
#[derive(Clone)]
pub struct FramePublisherSink {
    shared: Arc<Shared>,
}

impl FramePublisher {
    pub fn start(pipe_name: &str) -> Result<Self, String> {
        if pipe_name.is_empty() {
            return Err("frame pipe name must not be empty".into());
        }
        let path = if pipe_name.starts_with(r"\\.\pipe\") {
            pipe_name.to_owned()
        } else {
            format!(r"\\.\pipe\{pipe_name}")
        };
        let shared = Arc::new(Shared {
            latest: Mutex::new(Latest::default()),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
            failure: Mutex::new(None),
            published_sequence: AtomicU64::new(0),
            connected: AtomicBool::new(false),
            transmitted_sequence: AtomicU64::new(0),
            write_failures: AtomicU64::new(0),
            disconnect_count: AtomicU64::new(0),
            last_write_error: AtomicU32::new(0),
            last_disconnect_error: AtomicU32::new(0),
            connection_count: AtomicU64::new(0),
            connection_failures: AtomicU64::new(0),
        });
        let worker_shared = Arc::clone(&shared);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("stageswap-frame-publisher".into())
            .spawn(move || {
                if let Err(error) = server_loop(&path, &worker_shared, &startup_sender) {
                    if let Ok(mut failure) = worker_shared.failure.lock() {
                        *failure = Some(error.clone());
                    }
                    let _ = startup_sender.try_send(Err(error));
                }
            })
            .map_err(|error| format!("could not start frame publisher: {error}"))?;
        match startup_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                shared,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err("frame publisher stopped before creating its named pipe".into())
            }
        }
    }

    /// Keeps a healthy publisher, or releases a failed server before reopening its pipe.
    pub fn ensure_running(publisher: &mut Option<Self>, pipe_name: &str) -> Result<(), String> {
        if publisher.as_ref().is_some_and(|publisher| {
            publisher.sink().check_worker().is_ok()
                && publisher
                    .worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
        }) {
            return Ok(());
        }
        *publisher = None;
        *publisher = Some(Self::start(pipe_name)?);
        Ok(())
    }

    pub fn publish(&self, frame: &Frame) -> Result<(), String> {
        self.sink().publish(frame)
    }

    pub fn invalidate(&self) -> Result<(), String> {
        self.sink().invalidate()
    }

    pub fn sink(&self) -> FramePublisherSink {
        FramePublisherSink {
            shared: Arc::clone(&self.shared),
        }
    }

    pub fn diagnostics(&self) -> FramePublisherDiagnostics {
        self.sink().diagnostics()
    }
}

impl FramePublisherSink {
    pub fn publish(&self, frame: &Frame) -> Result<(), String> {
        self.check_worker()?;
        let frame_bytes =
            u32::try_from(frame.pixels().len()).map_err(|_| "frame exceeds the IPC capacity")?;
        if frame_bytes > MAX_FRAME_BYTES {
            return Err("frame exceeds the IPC capacity".into());
        }
        let mut latest = self
            .shared
            .latest
            .lock()
            .map_err(|_| "frame publisher state is poisoned")?;
        latest.sequence = latest.sequence.wrapping_add(1).max(1);
        let sequence = latest.sequence;
        latest.header = FrameHeader {
            sequence,
            size: frame.size,
            stride: frame.stride,
            timestamp_100ns: frame.timestamp_100ns,
            frame_bytes,
        }
        .encode()
        .map_err(|error| format!("invalid IPC frame: {error:?}"))?;
        latest.pixels = frame.pixels_arc();
        drop(latest);
        self.shared
            .published_sequence
            .store(sequence, Ordering::Release);
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn invalidate(&self) -> Result<(), String> {
        self.check_worker()?;
        let mut latest = self
            .shared
            .latest
            .lock()
            .map_err(|_| "frame publisher state is poisoned")?;
        latest.sequence = latest.sequence.wrapping_add(1).max(1);
        let sequence = latest.sequence;
        latest.header = FrameHeader::invalidation(sequence)
            .and_then(FrameHeader::encode)
            .map_err(|error| format!("invalid IPC invalidation: {error:?}"))?;
        latest.pixels = Arc::from([]);
        drop(latest);
        self.shared
            .published_sequence
            .store(sequence, Ordering::Release);
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn diagnostics(&self) -> FramePublisherDiagnostics {
        FramePublisherDiagnostics {
            published_sequence: self.shared.published_sequence.load(Ordering::Acquire),
            transmitted_sequence: self.shared.transmitted_sequence.load(Ordering::Acquire),
            connected: self.shared.connected.load(Ordering::Acquire),
            write_failures: self.shared.write_failures.load(Ordering::Acquire),
            disconnect_count: self.shared.disconnect_count.load(Ordering::Acquire),
            last_write_error: nonzero_error(self.shared.last_write_error.load(Ordering::Acquire)),
            last_disconnect_error: nonzero_error(
                self.shared.last_disconnect_error.load(Ordering::Acquire),
            ),
            connection_count: self.shared.connection_count.load(Ordering::Acquire),
            connection_failures: self.shared.connection_failures.load(Ordering::Acquire),
        }
    }

    fn check_worker(&self) -> Result<(), String> {
        if self.shared.stop.load(Ordering::Acquire) {
            return Err("frame publisher is stopped".into());
        }
        self.shared
            .failure
            .lock()
            .map_err(|_| "frame publisher failure state is poisoned")?
            .as_ref()
            .map_or_else(|| Ok(()), |error| Err(error.clone()))
    }
}

impl Drop for FramePublisher {
    fn drop(&mut self) {
        // Serialize the stop predicate with the condition-variable waiter.
        {
            let _latest = self
                .shared
                .latest
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.shared.stop.store(true, Ordering::Release);
            self.shared.changed.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            // SAFETY: this is the live worker thread handle and cancellation is
            // limited to its blocking ConnectNamedPipe/WriteFile calls.
            // Cancellation only covers I/O already in progress. Retry until the
            // worker exits to cover an operation started just after cancellation.
            while !worker.is_finished() {
                let _ = unsafe { CancelSynchronousIo(HANDLE(worker.as_raw_handle())) };
                thread::sleep(std::time::Duration::from_millis(10));
            }
            let _ = worker.join();
        }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns a valid kernel handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the SDDL conversion allocates this buffer with LocalAlloc.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

fn server_loop(
    path: &str,
    shared: &Shared,
    startup: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // Owner, SYSTEM, and LocalService can connect; remote clients are rejected
    // by the named-pipe mode as an additional boundary.
    let sddl: Vec<u16> = "D:P(A;;GA;;;OW)(A;;GA;;;SY)(A;;GA;;;LS)"
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: SDDL is terminated and descriptor is a writable output pointer.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|error| format!("could not create frame-pipe security descriptor: {error}"))?;
    let _descriptor = SecurityDescriptor(descriptor);
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: BOOL(0),
    };
    let path: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let mut startup = Some(startup);
    while !shared.stop.load(Ordering::Acquire) {
        // SAFETY: name and security attributes remain live until the pipe handle
        // is closed, and all sizes fit in u32 by the core IPC limit.
        let raw = unsafe {
            CreateNamedPipeW(
                PCWSTR(path.as_ptr()),
                PIPE_ACCESS_OUTBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                MAX_FRAME_BYTES + 40,
                64 * 1024,
                0,
                Some(&security),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };
            if error == ERROR_PIPE_BUSY {
                return Err(
                    "another StageSwap instance already owns the frame pipe; exit it from the system tray before relaunching"
                        .into(),
                );
            }
            return Err(format!("could not create frame pipe: {error:?}"));
        }
        let pipe = OwnedHandle(raw);
        if let Some(startup) = startup.take() {
            startup
                .send(Ok(()))
                .map_err(|_| "frame publisher startup receiver was dropped".to_string())?;
        }
        // SAFETY: pipe is a live server-side named-pipe handle.
        let connected = if unsafe { ConnectNamedPipe(pipe.0, None) }.is_ok() {
            true
        } else {
            let error = unsafe { GetLastError() };
            if error == ERROR_PIPE_CONNECTED {
                true
            } else {
                if !shared.stop.load(Ordering::Acquire) {
                    shared.connection_failures.fetch_add(1, Ordering::Relaxed);
                }
                false
            }
        };
        if !connected {
            continue;
        }
        shared.connected.store(true, Ordering::Release);
        shared.connection_count.fetch_add(1, Ordering::Relaxed);
        let result = send_frames(pipe.0, shared);
        shared.connected.store(false, Ordering::Release);
        if !shared.stop.load(Ordering::Acquire) {
            match result {
                SendFramesResult::ClientDisconnected(error) => {
                    shared.disconnect_count.fetch_add(1, Ordering::Relaxed);
                    shared.last_disconnect_error.store(error, Ordering::Release);
                }
                SendFramesResult::WriteFailed(error) => {
                    shared.write_failures.fetch_add(1, Ordering::Relaxed);
                    shared.last_write_error.store(error, Ordering::Release);
                }
                SendFramesResult::StatePoisoned => {
                    return Err("frame publisher state is poisoned".into());
                }
                SendFramesResult::Stopped => {}
            }
        }
        // SAFETY: pipe remains live through this call.
        let _ = unsafe { DisconnectNamedPipe(pipe.0) };
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendFramesResult {
    Stopped,
    ClientDisconnected(u32),
    WriteFailed(u32),
    StatePoisoned,
}

fn send_frames(pipe: HANDLE, shared: &Shared) -> SendFramesResult {
    let mut sent = 0;
    while !shared.stop.load(Ordering::Acquire) {
        let Ok(latest) = shared.latest.lock() else {
            return SendFramesResult::StatePoisoned;
        };
        let Ok(latest) = shared.changed.wait_while(latest, |latest| {
            latest.sequence == sent && !shared.stop.load(Ordering::Acquire)
        }) else {
            return SendFramesResult::StatePoisoned;
        };
        if shared.stop.load(Ordering::Acquire) {
            return SendFramesResult::Stopped;
        }
        let sequence = latest.sequence;
        let header = latest.header;
        let pixels = Arc::clone(&latest.pixels);
        drop(latest);
        let write_result = write_all(pipe, &header).and_then(|()| {
            if pixels.is_empty() {
                Ok(())
            } else {
                write_all(pipe, &pixels)
            }
        });
        if let Err(error) = write_result {
            return if is_expected_pipe_disconnect(error) {
                SendFramesResult::ClientDisconnected(error)
            } else {
                SendFramesResult::WriteFailed(error)
            };
        }
        sent = sequence;
        shared
            .transmitted_sequence
            .store(sequence, Ordering::Release);
    }
    SendFramesResult::Stopped
}

fn write_all(pipe: HANDLE, mut bytes: &[u8]) -> Result<(), u32> {
    while !bytes.is_empty() {
        let mut written = 0;
        // SAFETY: pipe is live, the slice is readable, and written is writable.
        if unsafe { WriteFile(pipe, Some(bytes), Some(&mut written), None) }.is_err() {
            return Err(unsafe { GetLastError() }.0);
        }
        if written == 0 {
            return Err(29); // ERROR_WRITE_FAULT
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

fn nonzero_error(error: u32) -> Option<u32> {
    (error != 0).then_some(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stageswap_core::Size;
    use std::time::Instant;

    #[test]
    fn native_failed_publisher_is_replaced_and_healthy_publisher_is_retained() {
        let name = format!(
            r"\\.\pipe\StageSwap.RecoverPublisher.{}",
            std::process::id()
        );
        let mut publisher = None;
        FramePublisher::ensure_running(&mut publisher, &name).unwrap();
        let first = publisher.as_ref().unwrap().sink();
        FramePublisher::ensure_running(&mut publisher, &name).unwrap();
        assert!(Arc::ptr_eq(
            &first.shared,
            &publisher.as_ref().unwrap().shared
        ));
        *first.shared.failure.lock().unwrap() = Some("injected publisher failure".into());
        FramePublisher::ensure_running(&mut publisher, &name).unwrap();
        assert!(!Arc::ptr_eq(
            &first.shared,
            &publisher.as_ref().unwrap().shared
        ));
        let frame = Frame::placeholder(
            stageswap_core::PIPELINE_SIZE,
            0xff00_0000,
            1,
            0,
            Instant::now(),
        );
        assert!(first.publish(&frame).is_err());
        publisher.as_ref().unwrap().publish(&frame).unwrap();
    }

    #[test]
    fn native_duplicate_pipe_publisher_is_rejected_during_startup() {
        let name = format!(r"\\.\pipe\StageSwap.PublisherTest.{}", std::process::id());
        let first = FramePublisher::start(&name).expect("first publisher should own the pipe");
        let second = FramePublisher::start(&name);
        assert!(second.is_err(), "duplicate publisher unexpectedly started");
        drop(first);
    }

    #[test]
    fn native_shutdown_completes_without_a_client_or_with_a_stalled_client() {
        let (done, completion) = mpsc::sync_channel(1);
        thread::spawn(move || {
            for scenario in 0..3 {
                let name = format!(
                    r"\\.\pipe\StageSwap.ShutdownTest.{}.{}",
                    std::process::id(),
                    scenario
                );
                let publisher = FramePublisher::start(&name).unwrap();
                // Keep the client open through shutdown without reading from it.
                let client = (scenario != 0)
                    .then(|| std::fs::OpenOptions::new().read(true).open(&name).unwrap());
                if scenario == 2 {
                    let frame = Frame::placeholder(
                        stageswap_core::PIPELINE_SIZE,
                        0xff00_0000,
                        1,
                        0,
                        Instant::now(),
                    );
                    // Fill the pipe and leave the publisher blocked writing the payload.
                    for _ in 0..4 {
                        publisher.publish(&frame).unwrap();
                        thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
                drop(publisher);
                drop(client);
            }
            done.send(()).unwrap();
        });
        completion
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("pipe shutdown must complete even when the client does not read");
    }

    #[test]
    fn native_publishing_replaces_the_latest_frame_without_copying_its_pixels() {
        let name = format!(r"\\.\pipe\StageSwap.LatestTest.{}", std::process::id());
        let publisher = FramePublisher::start(&name).unwrap();
        let first = Frame::placeholder(Size::new(2, 2), 0xff00_0000, 1, 0, Instant::now());
        let second = Frame::placeholder(Size::new(2, 2), 0xffff_ffff, 2, 1, Instant::now());
        let second_pixels = second.pixels_arc();
        publisher.publish(&first).unwrap();
        publisher.publish(&second).unwrap();
        let latest = publisher.shared.latest.lock().unwrap();
        assert_eq!(latest.sequence, 2);
        assert!(Arc::ptr_eq(&latest.pixels, &second_pixels));
    }
}
