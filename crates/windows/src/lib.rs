#![cfg_attr(not(windows), forbid(unsafe_code))]

#[cfg(all(windows, not(target_arch = "x86_64")))]
compile_error!("StageSwap supports only x64 Windows");

use stageswap_core::{Frame, MonitorDescriptor};
use std::sync::Arc;

#[cfg(any(windows, test))]
mod component_compatibility;
#[cfg(any(windows, test))]
mod instance_scope;
#[cfg(windows)]
mod pipe;
#[cfg(windows)]
pub use pipe::{FramePublisher, FramePublisherDiagnostics, FramePublisherSink};
#[cfg(windows)]
mod virtual_camera;
#[cfg(windows)]
pub use virtual_camera::{VirtualCameraController, frame_pipe_name, remove_virtual_camera};
#[cfg(windows)]
mod video_input;
#[cfg(windows)]
pub use video_input::{MediaFoundationVideoInput, WebcamCaptureFailure, enumerate_video_devices};
#[cfg(windows)]
mod deployment;
#[cfg(windows)]
pub use deployment::{
    StartupRegistrationStatus, cleanup_deployment, configure_startup, deployment_startup,
    startup_registration_status, uninstall_deployment,
};
#[cfg(windows)]
mod notification;
#[cfg(windows)]
pub use notification::show_error_dialog;
#[cfg(windows)]
mod http;
#[cfg(windows)]
pub use http::{https_download, https_get};
#[cfg(windows)]
mod system_locale;
#[cfg(windows)]
pub use system_locale::{preferred_interface_locale, user_interface_locale};
#[cfg(windows)]
mod system_animation;
#[cfg(windows)]
pub use system_animation::client_area_animations_enabled;
#[cfg(windows)]
mod dialog;
#[cfg(windows)]
mod shell_ui;
#[cfg(windows)]
pub use shell_ui::{
    open_camera_privacy_settings, open_directory, pick_log_export_path, pick_reference_image,
};
#[cfg(windows)]
mod instance_control;
#[cfg(windows)]
mod portable_install;
#[cfg(windows)]
mod screen_input;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
mod user_identity;
#[cfg(windows)]
pub use deployment::{replace_file_atomic, save_config_atomic};
#[cfg(windows)]
pub use instance_control::{
    InstanceCommand, InstanceControl, InstanceReadiness, InstanceStatus, instance_status,
    send_instance_command,
};
#[cfg(windows)]
pub use portable_install::{
    BootstrapResult, LaunchContext, PortableMode, bootstrap as portable_bootstrap, launch_update,
    managed_executable_path, request_install,
};
#[cfg(windows)]
pub use screen_input::WindowsGraphicsScreenInput;
#[cfg(windows)]
pub use session::current_session_active;
#[cfg(windows)]
pub use single_instance::SingleInstance;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_virtual: bool,
}

pub trait VideoInput: Send {
    fn enumerate(&self) -> Result<Vec<InputDevice>, String>;
    fn start(&mut self, device_id: &str) -> Result<(), String>;
    fn stop(&mut self);
    fn latest_frame(&self) -> Option<Arc<Frame>>;
}

pub trait ScreenInput: Send {
    fn enumerate(&self) -> Result<Vec<MonitorDescriptor>, String>;
    fn start(&mut self, monitor: &MonitorDescriptor, cursor_visible: bool) -> Result<(), String>;
    fn stop(&mut self);
    fn latest_frame(&self) -> Option<Arc<Frame>>;
}

pub fn choose_video_device(
    saved_id: &str,
    devices: &[InputDevice],
    saved_opened: bool,
) -> Option<String> {
    if saved_opened && !saved_id.is_empty() {
        return Some(saved_id.into());
    }
    let mut physical = devices.iter().filter(|device| !device.is_virtual);
    let only = physical.next()?;
    if physical.next().is_none() {
        Some(only.id.clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn device(id: &str, is_virtual: bool) -> InputDevice {
        InputDevice {
            id: id.into(),
            name: id.into(),
            is_virtual,
        }
    }
    #[test]
    fn native_selection_is_saved_then_unique_physical() {
        let devices = [device("virtual", true), device("physical", false)];
        assert_eq!(
            choose_video_device("saved", &devices, true).as_deref(),
            Some("saved")
        );
        assert_eq!(
            choose_video_device("missing", &devices, false).as_deref(),
            Some("physical")
        );
        let ambiguous = [device("one", false), device("two", false)];
        assert_eq!(choose_video_device("", &ambiguous, false), None);
    }
}

#[cfg(all(test, windows))]
mod interactive_windows_tests {
    use super::*;
    use stageswap_core::{FramePacer, PIPELINE_FPS, PIPELINE_SIZE};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    fn wait_for_sequences(input: &dyn ScreenInput, count: usize) -> Vec<Instant> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut samples = Vec::with_capacity(count);
        let mut last_sequence = None;
        while samples.len() < count && Instant::now() < deadline {
            if let Some(frame) = input.latest_frame()
                && last_sequence != Some(frame.sequence)
            {
                last_sequence = Some(frame.sequence);
                samples.push(frame.received_at);
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            samples.len(),
            count,
            "capture did not produce {count} frames"
        );
        samples
    }

    fn collect_video_sequences(input: &dyn VideoInput, count: usize) -> Vec<(Instant, i64)> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut samples = Vec::with_capacity(count);
        let mut last_sequence = None;
        while samples.len() < count && Instant::now() < deadline {
            if let Some(frame) = input.latest_frame()
                && last_sequence != Some(frame.sequence)
            {
                last_sequence = Some(frame.sequence);
                samples.push((frame.received_at, frame.timestamp_100ns));
            }
            thread::sleep(Duration::from_millis(2));
        }
        samples
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn native_screen_capture_produces_300_frames_with_cursor_on_and_off_and_stops_cleanly() {
        let mut input = WindowsGraphicsScreenInput::default();
        let monitor = input.enumerate().unwrap().into_iter().next().unwrap();
        for cursor_visible in [true, false] {
            input.start(&monitor, cursor_visible).unwrap();
            let samples = wait_for_sequences(&input, 300);
            let elapsed = samples.last().unwrap().duration_since(samples[0]);
            let fps = (samples.len() - 1) as f64 / elapsed.as_secs_f64();
            let maximum_gap = samples
                .windows(2)
                .map(|pair| pair[1].duration_since(pair[0]))
                .max()
                .unwrap();
            assert!(fps >= 29.0, "screen capture averaged only {fps:.2} fps");
            assert!(
                maximum_gap <= Duration::from_millis(100),
                "screen capture stalled for {maximum_gap:?}"
            );
            input.stop();
            assert!(input.latest_frame().is_none());
        }
    }

    #[test]
    #[ignore = "requires a physical webcam"]
    fn native_webcam_enumeration_excludes_virtual_camera_and_restarts_without_stale_frame() {
        let devices = enumerate_video_devices().unwrap();
        assert!(devices.iter().all(|device| !device.is_virtual));
        let device = devices.first().expect("a physical webcam is required");
        let mut input = MediaFoundationVideoInput::default();
        for _ in 0..2 {
            input.start(&device.id).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while input.latest_frame().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(input.latest_frame().is_some());
            input.stop();
            assert!(input.latest_frame().is_none());
        }
    }

    #[test]
    #[ignore = "requires the COM source to be installed"]
    fn native_virtual_camera_can_restart_manually() {
        let pipe = frame_pipe_name().unwrap();
        let mut camera = VirtualCameraController::start(pipe).unwrap();
        assert!(camera.is_running());
        camera.restart().unwrap();
        assert!(camera.is_running());
    }

    #[test]
    #[ignore = "requires the COM source to be installed and no running app instance"]
    fn native_virtual_camera_delivers_three_hundred_frames_at_thirty_fps() {
        let pipe_name = frame_pipe_name().unwrap();
        let publisher = FramePublisher::start(&pipe_name).unwrap();
        let _camera = VirtualCameraController::start(pipe_name.clone()).unwrap();
        let mut input = MediaFoundationVideoInput::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        let device = loop {
            if let Some(device) = video_input::enumerate_all_video_devices()
                .unwrap_or_default()
                .into_iter()
                .find(|device| device.is_virtual)
            {
                break device;
            }
            assert!(
                Instant::now() < deadline,
                "virtual camera did not appear in device enumeration"
            );
            thread::sleep(Duration::from_millis(100));
        };
        input.start(&device.id).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let publisher_worker = thread::spawn(move || {
            let interval = Duration::from_nanos(1_000_000_000 / u64::from(PIPELINE_FPS));
            let mut pacer = FramePacer::new(Instant::now(), interval);
            let frame = Frame::placeholder(PIPELINE_SIZE, 0xff20_3040, 1, 0, Instant::now());
            while !worker_stop.load(Ordering::Acquire) {
                let wait = pacer.wait_duration(Instant::now());
                if !wait.is_zero() {
                    thread::sleep(wait);
                }
                let now = Instant::now();
                pacer.advance(now);
                if publisher.publish(&frame).is_err() {
                    break;
                }
            }
        });
        let _warmup = collect_video_sequences(&input, 30);
        let samples = collect_video_sequences(&input, 300);
        stop.store(true, Ordering::Release);
        publisher_worker.join().unwrap();

        assert_eq!(
            samples.len(),
            300,
            "virtual camera did not deliver 300 frames"
        );
        assert!(
            samples.windows(2).all(|pair| pair[1].1 > pair[0].1),
            "virtual camera sample timestamps were not monotonic"
        );
        let elapsed = samples.last().unwrap().0.duration_since(samples[0].0);
        let fps = (samples.len() - 1) as f64 / elapsed.as_secs_f64();
        let maximum_gap = samples
            .windows(2)
            .map(|pair| pair[1].0.duration_since(pair[0].0))
            .max()
            .unwrap();
        assert!(fps >= 29.0, "virtual camera averaged only {fps:.2} fps");
        assert!(
            maximum_gap <= Duration::from_millis(100),
            "virtual camera stalled for {maximum_gap:?}"
        );

        let sequence_before_restart = input.latest_frame().unwrap().sequence;
        let restarted_publisher = FramePublisher::start(&pipe_name).unwrap();
        let restart_stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&restart_stop);
        let restarted_worker = thread::spawn(move || {
            let interval = Duration::from_nanos(1_000_000_000 / u64::from(PIPELINE_FPS));
            let mut pacer = FramePacer::new(Instant::now(), interval);
            let frame = Frame::placeholder(PIPELINE_SIZE, 0xff60_4020, 1, 0, Instant::now());
            while !worker_stop.load(Ordering::Acquire) {
                let wait = pacer.wait_duration(Instant::now());
                if !wait.is_zero() {
                    thread::sleep(wait);
                }
                let now = Instant::now();
                pacer.advance(now);
                if restarted_publisher.publish(&frame).is_err() {
                    break;
                }
            }
        });
        let resumed = collect_video_sequences(&input, 30);
        restart_stop.store(true, Ordering::Release);
        restarted_worker.join().unwrap();
        assert_eq!(
            resumed.len(),
            30,
            "video did not resume after publisher restart"
        );
        assert!(
            input.latest_frame().unwrap().sequence > sequence_before_restart,
            "consumer did not advance after the publisher sequence restarted"
        );
        input.stop();
    }
}
