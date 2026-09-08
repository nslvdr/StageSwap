//! Bounded background discovery and monitor scanning.

use super::*;
#[cfg(windows)]
use stageswap_core::MonitorScore;
#[cfg(windows)]
use stageswap_windows::{
    InputDevice, MediaFoundationVideoInput, ScreenInput, VideoInput, WindowsGraphicsScreenInput,
};

#[cfg(any(windows, test))]
pub(super) fn choose_initial_monitor(
    saved_label: &str,
    monitors: &[MonitorDescriptor],
) -> Option<MonitorDescriptor> {
    monitors
        .iter()
        .find(|monitor| monitor.label == saved_label)
        .or_else(|| monitors.get(1))
        .or_else(|| monitors.first())
        .cloned()
}

#[cfg(windows)]
pub(super) struct MonitorScanRequest {
    pub(super) capture_active: Arc<AtomicBool>,
    pub(super) generation: u64,
    pub(super) reference: Option<GrayImage>,
    pub(super) cursor_visible: bool,
}

#[cfg(windows)]
pub(super) struct MonitorScanResult {
    pub(super) generation: u64,
    pub(super) monitors: Vec<stageswap_core::MonitorDescriptor>,
    pub(super) scores: Vec<MonitorScore>,
}

#[cfg(any(windows, test))]
pub(super) struct CoalescingSlot<T> {
    pub(super) pending: Mutex<Option<T>>,
    pub(super) changed: Condvar,
}

#[cfg(any(windows, test))]
impl<T> CoalescingSlot<T> {
    pub(super) fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            changed: Condvar::new(),
        }
    }

    pub(super) fn replace(&self, value: T) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        *pending = Some(value);
        self.changed.notify_one();
        true
    }

    #[cfg(test)]
    pub(super) fn take(&self) -> Option<T> {
        self.pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.take())
    }

    pub(super) fn wait_take(&self, stop: &AtomicBool) -> Option<T> {
        let mut pending = self
            .pending
            .lock()
            .expect("coalescing worker request state is not poisoned");
        while pending.is_none() && !stop.load(Ordering::Acquire) {
            pending = self
                .changed
                .wait(pending)
                .expect("coalescing worker request state is not poisoned");
        }
        if stop.load(Ordering::Acquire) {
            None
        } else {
            pending.take()
        }
    }

    pub(super) fn clear(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending = None;
        }
    }

    /// Changes the stop predicate under the waiter's mutex, preventing lost wakes.
    pub(super) fn stop(&self, stop: &AtomicBool) {
        let _pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        stop.store(true, Ordering::Release);
        self.changed.notify_all();
    }
}

#[cfg(windows)]
pub(super) struct MonitorScanWorker {
    pub(super) capture_active: Arc<AtomicBool>,
    pub(super) pending: Arc<CoalescingSlot<MonitorScanRequest>>,
    pub(super) result: Arc<Mutex<Option<MonitorScanResult>>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) done: Receiver<()>,
    pub(super) worker: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl MonitorScanWorker {
    pub(super) fn start(active: bool) -> Result<Self, String> {
        let capture_active = Arc::new(AtomicBool::new(active));

        let pending = Arc::new(CoalescingSlot::new());
        let worker_pending = Arc::clone(&pending);
        let result = Arc::new(Mutex::new(None));
        let worker_result = Arc::clone(&result);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (done_sender, done) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("stageswap-monitor-scan".into())
            .spawn(move || {
                loop {
                    let request = worker_pending.wait_take(&worker_stop);
                    let Some(request) = request else {
                        break;
                    };
                    let scan = scan_monitors(request, &worker_stop);
                    if let Ok(mut result) = worker_result.lock() {
                        *result = Some(scan);
                    }
                }
                let _ = done_sender.try_send(());
            })
            .map_err(|error| format!("could not start monitor scan worker: {error}"))?;
        Ok(Self {
            capture_active,
            pending,
            result,
            stop,
            done,
            worker: Some(worker),
        })
    }

    pub(super) fn request(&self, request: MonitorScanRequest) -> bool {
        if self.stop.load(Ordering::Acquire) {
            return false;
        }
        self.pending.replace(request)
    }

    pub(super) fn poll(&self) -> Option<MonitorScanResult> {
        self.result.lock().ok().and_then(|mut result| result.take())
    }

    pub(super) fn clear_pending(&self) {
        self.pending.clear();
        if let Ok(mut result) = self.result.lock() {
            *result = None;
        }
    }

    pub(super) fn signal_shutdown(&self) {
        self.pending.stop(&self.stop);
    }

    pub(super) fn finish_shutdown(&mut self) {
        self.signal_shutdown();
        let _ = finish_worker_shutdown(&self.done, &mut self.worker, WORKER_SHUTDOWN_TIMEOUT);
    }
}

#[cfg(windows)]
impl Drop for MonitorScanWorker {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.finish_shutdown();
        }
    }
}

#[cfg(windows)]
type VideoDeviceEnumeration = Result<Vec<InputDevice>, String>;

#[cfg(windows)]
pub(super) struct VideoDeviceWorker {
    pub(super) pending: Arc<CoalescingSlot<()>>,
    pub(super) result: Arc<Mutex<Option<VideoDeviceEnumeration>>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) done: Receiver<()>,
    pub(super) worker: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl VideoDeviceWorker {
    pub(super) fn start() -> Result<Self, String> {
        let pending = Arc::new(CoalescingSlot::new());
        let worker_pending = Arc::clone(&pending);
        let result = Arc::new(Mutex::new(None));
        let worker_result = Arc::clone(&result);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (done_sender, done) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("stageswap-video-device-enumeration".into())
            .spawn(move || {
                let input = MediaFoundationVideoInput::default();
                loop {
                    if worker_pending.wait_take(&worker_stop).is_none() {
                        break;
                    }
                    let enumeration = input.enumerate();
                    if let Ok(mut result) = worker_result.lock() {
                        *result = Some(enumeration);
                    }
                }
                let _ = done_sender.try_send(());
            })
            .map_err(|error| format!("could not start video device enumeration worker: {error}"))?;
        Ok(Self {
            pending,
            result,
            stop,
            done,
            worker: Some(worker),
        })
    }

    pub(super) fn request(&self) -> bool {
        if self.stop.load(Ordering::Acquire) {
            return false;
        }
        self.pending.replace(())
    }

    pub(super) fn poll(&self) -> Option<VideoDeviceEnumeration> {
        self.result.lock().ok().and_then(|mut result| result.take())
    }

    pub(super) fn signal_shutdown(&self) {
        self.pending.stop(&self.stop);
    }

    pub(super) fn finish_shutdown(&mut self) {
        self.signal_shutdown();
        let _ = finish_worker_shutdown(&self.done, &mut self.worker, WORKER_SHUTDOWN_TIMEOUT);
    }
}

#[cfg(windows)]
impl Drop for VideoDeviceWorker {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.finish_shutdown();
        }
    }
}

#[cfg(any(windows, test))]
pub(super) fn monitor_color_allows_matching(color: Result<bool, String>) -> bool {
    matches!(color, Ok(false))
}

#[cfg(windows)]
pub(super) fn scan_monitors(request: MonitorScanRequest, stop: &AtomicBool) -> MonitorScanResult {
    let active = &request.capture_active;
    let input = WindowsGraphicsScreenInput::default();
    let monitors = input.enumerate().unwrap_or_default();
    let scores = request
        .reference
        .as_ref()
        .map_or_else(Vec::new, |reference| {
            monitors
                .iter()
                .cloned()
                .take_while(|_| active.load(Ordering::Acquire) && !stop.load(Ordering::Acquire))
                .map(|monitor| {
                    let mut capture = WindowsGraphicsScreenInput::default();
                    // Unverified and unsupported color cannot participate in reference matching.
                    let valid = monitor_color_allows_matching(
                        capture.display_uses_hdr_or_ten_bit(&monitor),
                    ) && capture.start(&monitor, request.cursor_visible).is_ok();
                    let deadline = Instant::now() + Duration::from_millis(750);
                    let frame = valid
                        .then(|| {
                            loop {
                                if !active.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
                                    break None;
                                }
                                if let Some(frame) = capture.latest_frame() {
                                    break Some(frame);
                                }
                                if Instant::now() >= deadline {
                                    break None;
                                }
                                thread::sleep(Duration::from_millis(25));
                            }
                        })
                        .flatten();
                    capture.stop();
                    let similarity = frame
                        .as_deref()
                        .and_then(gray_thumbnail)
                        .map_or(0.0, |candidate| image_similarity(reference, &candidate));
                    MonitorScore {
                        monitor,
                        similarity,
                        capture_valid: valid && frame.is_some(),
                    }
                })
                .collect()
        });
    MonitorScanResult {
        generation: request.generation,
        monitors,
        scores,
    }
}
