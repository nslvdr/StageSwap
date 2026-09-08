use crate::ScreenInput;
use stageswap_core::{
    CAPTURE_FRAME_POOL_CAPACITY, Frame, FrameBufferPool, FramePacer, MonitorDescriptor,
    PIPELINE_SIZE, Size, aspect_fit_bgra_into,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Graphics::Capture::GraphicsCaptureSession;
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame as CaptureFrame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

const SCREEN_FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 30);
const SCREEN_FRAME_EARLY_TOLERANCE: Duration = Duration::from_millis(1);

fn display_uses_hdr_or_ten_bit(display_name: &str) -> Result<bool, String> {
    let mut path_count = 0;
    let mut mode_count = 0;
    // SAFETY: both count pointers are writable.
    unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count) }
        .ok()
        .map_err(|error| format!("could not query active display paths: {error}"))?;
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![Default::default(); mode_count as usize];
    // SAFETY: arrays have the capacities supplied in their writable count values.
    unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    }
    .ok()
    .map_err(|error| format!("could not read active display paths: {error}"))?;
    paths.truncate(path_count as usize);
    for path in paths {
        let mut source_name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..DISPLAYCONFIG_SOURCE_DEVICE_NAME::default()
        };
        // SAFETY: the packet header identifies the enclosing writable structure.
        let status = unsafe { DisplayConfigGetDeviceInfo(&raw mut source_name.header) };
        if status != 0 {
            continue;
        }
        let length = source_name
            .viewGdiDeviceName
            .iter()
            .position(|word| *word == 0)
            .unwrap_or(source_name.viewGdiDeviceName.len());
        if !String::from_utf16_lossy(&source_name.viewGdiDeviceName[..length])
            .eq_ignore_ascii_case(display_name)
        {
            continue;
        }
        let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default()
        };
        // SAFETY: the packet header identifies the enclosing writable structure.
        let status = unsafe { DisplayConfigGetDeviceInfo(&raw mut color.header) };
        if status != 0 {
            return Err(format!(
                "could not read advanced color information for {display_name}: error {status}"
            ));
        }
        // Windows defines bit 1 as advancedColorEnabled. Ten-bit SDR is also outside
        // StageSwap's current 8-bit matching contract.
        let flags = unsafe { color.Anonymous.value };
        return Ok(flags & 0b10 != 0 || color.bitsPerColorChannel > 8);
    }
    Err(format!(
        "could not resolve selected display {display_name} in the active display topology"
    ))
}

#[derive(Default)]
struct Shared {
    latest: Mutex<Option<Arc<Frame>>>,
    failure: Mutex<Option<String>>,
    generation: AtomicU64,
    sequence: AtomicU64,
    pool_fallbacks: AtomicU64,
    dropped_frames: AtomicU64,
}

impl Shared {
    fn is_current(&self, expected_generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == expected_generation
    }

    fn record_failure(&self, expected_generation: u64, message: impl Into<String>) {
        if !self.is_current(expected_generation) {
            return;
        }
        if let Ok(mut latest) = self.latest.lock() {
            *latest = None;
        }
        if let Ok(mut failure) = self.failure.lock() {
            *failure = Some(message.into());
        }
    }
}

struct CaptureFlags {
    shared: Arc<Shared>,
    expected_generation: u64,
}

#[derive(Default)]
struct ProcessingFailureStreak(u8);

impl ProcessingFailureStreak {
    fn failed(&mut self) -> bool {
        self.0 = self.0.saturating_add(1);
        self.0 >= 3
    }

    fn succeeded(&mut self) {
        self.0 = 0;
    }
}

struct CaptureHandler {
    shared: Arc<Shared>,
    expected_generation: u64,
    staging: Option<(D3D11_TEXTURE2D_DESC, ID3D11Texture2D)>,
    pool: FrameBufferPool,
    pacer: Option<FramePacer>,
    processing_failures: ProcessingFailureStreak,
}

/// Keeps the staging resource mapped only while normalization reads its rows.
struct MappedScreen<'a> {
    context: &'a ID3D11DeviceContext,
    texture: &'a ID3D11Texture2D,
}

impl Drop for MappedScreen<'_> {
    fn drop(&mut self) {
        // SAFETY: constructed only after Map succeeds, with both interfaces retained.
        unsafe { self.context.Unmap(self.texture, 0) };
    }
}

/// Captures keep one staging texture per geometry/format, owned by their handler.
fn screen_staging(
    staging: &mut Option<(D3D11_TEXTURE2D_DESC, ID3D11Texture2D)>,
    device: &ID3D11Device,
    source_desc: &D3D11_TEXTURE2D_DESC,
) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        ..*source_desc
    };
    if let Some((previous, texture)) = staging
        && *previous == desc
    {
        return Ok(texture.clone());
    }
    let mut texture = None;
    // SAFETY: the capture device is live and desc describes a CPU-readable copy.
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }
        .map_err(|error| format!("could not create screen staging texture: {error}"))?;
    let texture = texture.ok_or("screen staging texture was not created")?;
    *staging = Some((desc, texture.clone()));
    Ok(texture)
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = CaptureFlags;
    type Error = String;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            shared: context.flags.shared,
            expected_generation: context.flags.expected_generation,
            staging: None,
            pool: FrameBufferPool::new(
                (PIPELINE_SIZE.width * PIPELINE_SIZE.height * 4) as usize,
                CAPTURE_FRAME_POOL_CAPACITY,
            ),
            pacer: None,
            processing_failures: ProcessingFailureStreak::default(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut CaptureFrame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if !self.shared.is_current(self.expected_generation) {
            return Ok(());
        }
        let now = Instant::now();
        let pacer = self
            .pacer
            .get_or_insert_with(|| FramePacer::new(now, SCREEN_FRAME_INTERVAL));
        if !pacer.is_due(now, SCREEN_FRAME_EARLY_TOLERANCE) {
            return Ok(());
        }
        pacer.advance(now);
        let result = (|| -> Result<(), String> {
            let timestamp = frame.timestamp().map_or(0, |time| time.Duration);
            let width = frame.width();
            let height = frame.height();
            let texture = screen_staging(&mut self.staging, frame.device(), frame.desc())?;
            let context = frame.device_context();
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            // SAFETY: the staging texture matches the source and is not mapped yet.
            unsafe {
                context.CopyResource(&texture, frame.as_raw_texture());
                context.Map(&texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            }
            .map_err(|error| format!("could not map screen staging texture: {error}"))?;
            let _mapping = MappedScreen {
                context,
                texture: &texture,
            };
            let length = (mapped.RowPitch as usize)
                .checked_mul(height as usize)
                .filter(|length| *length <= isize::MAX as usize)
                .ok_or("screen mapped buffer size overflowed")?;
            if mapped.pData.is_null() {
                return Err("screen mapped buffer is null".into());
            }
            // SAFETY: Map exposes RowPitch * height readable bytes until _mapping drops.
            let pixels = unsafe { std::slice::from_raw_parts(mapped.pData.cast(), length) };
            let previous_exhaustions = self.pool.exhaustion_count();
            let pixels = self
                .pool
                .write_with_fallback(|destination| {
                    aspect_fit_bgra_into(
                        pixels,
                        Size::new(width, height),
                        mapped.RowPitch,
                        destination,
                        PIPELINE_SIZE,
                    )
                })
                .map_err(|error| format!("could not normalize screen frame: {error:?}"))?;
            if self.pool.exhaustion_count() > previous_exhaustions {
                self.shared.pool_fallbacks.fetch_add(1, Ordering::Relaxed);
            }
            let sequence = self.shared.sequence.fetch_add(1, Ordering::Relaxed) + 1;
            let frame = Frame::new(
                pixels,
                PIPELINE_SIZE,
                PIPELINE_SIZE.width * 4,
                sequence,
                timestamp,
                now,
            )
            .map_err(|error| format!("invalid screen frame: {error:?}"))?;
            if self.shared.is_current(self.expected_generation) {
                *self
                    .shared
                    .latest
                    .lock()
                    .map_err(|_| "screen frame state is poisoned")? = Some(Arc::new(frame));
                if let Ok(mut failure) = self.shared.failure.lock() {
                    *failure = None;
                }
                self.processing_failures.succeeded();
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.shared.dropped_frames.fetch_add(1, Ordering::Relaxed);
            if !self.processing_failures.failed() {
                return Ok(());
            }
            self.shared
                .record_failure(self.expected_generation, error.clone());
        }
        result
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.shared.record_failure(
            self.expected_generation,
            "selected display closed the screen-capture session",
        );
        Ok(())
    }
}

pub struct WindowsGraphicsScreenInput {
    shared: Arc<Shared>,
    control: Option<CaptureControl<CaptureHandler, String>>,
}

impl Default for WindowsGraphicsScreenInput {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            control: None,
        }
    }
}

impl WindowsGraphicsScreenInput {
    pub fn dropped_frame_count(&self) -> u64 {
        self.shared.dropped_frames.load(Ordering::Relaxed)
    }

    pub fn pool_fallback_count(&self) -> u64 {
        self.shared.pool_fallbacks.load(Ordering::Relaxed)
    }

    pub fn last_error(&self) -> Option<String> {
        if self
            .control
            .as_ref()
            .is_some_and(CaptureControl::is_finished)
        {
            let has_failure = self
                .shared
                .failure
                .lock()
                .ok()
                .is_some_and(|failure| failure.is_some());
            if !has_failure {
                self.shared.record_failure(
                    self.generation(),
                    "screen capture worker stopped unexpectedly",
                );
            }
        }
        self.shared
            .failure
            .lock()
            .ok()
            .and_then(|failure| failure.clone())
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    pub fn display_uses_hdr_or_ten_bit(
        &self,
        descriptor: &MonitorDescriptor,
    ) -> Result<bool, String> {
        display_uses_hdr_or_ten_bit(&descriptor.display_name)
    }
}

impl ScreenInput for WindowsGraphicsScreenInput {
    fn enumerate(&self) -> Result<Vec<MonitorDescriptor>, String> {
        let primary = Monitor::primary().ok();
        let mut monitors = Monitor::enumerate()
            .map_err(|error| format!("could not enumerate monitors: {error}"))?;
        monitors.sort_by_key(|monitor| u8::from(Some(*monitor) != primary));
        monitors.into_iter().map(describe_monitor).collect()
    }

    fn start(
        &mut self,
        descriptor: &MonitorDescriptor,
        cursor_visible: bool,
    ) -> Result<(), String> {
        self.stop();
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(
                "Windows Graphics Capture is unavailable in this Windows session or configuration"
                    .into(),
            );
        }
        let monitor = Monitor::enumerate()
            .map_err(|error| format!("could not enumerate monitors: {error}"))?
            .into_iter()
            .find(|monitor| {
                monitor
                    .device_name()
                    .is_ok_and(|name| name == descriptor.display_name)
            })
            .ok_or_else(|| format!("monitor {} is no longer available", descriptor.display_name))?;
        let cursor = if cursor_visible {
            CursorCaptureSettings::WithCursor
        } else {
            CursorCaptureSettings::WithoutCursor
        };
        let expected_generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Ok(mut failure) = self.shared.failure.lock() {
            *failure = None;
        }
        let settings = Settings::new(
            monitor,
            cursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            // Custom minimum update intervals are unavailable on some Windows versions.
            // The system default keeps capture working across all supported platforms.
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            CaptureFlags {
                shared: Arc::clone(&self.shared),
                expected_generation,
            },
        );
        self.control = Some(
            CaptureHandler::start_free_threaded(settings)
                .map_err(|error| format!("could not start Windows Graphics Capture: {error}"))?,
        );
        Ok(())
    }

    fn stop(&mut self) {
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
        if let Ok(mut latest) = self.shared.latest.lock() {
            *latest = None;
        }
        if let Ok(mut failure) = self.shared.failure.lock() {
            *failure = None;
        }
    }

    fn latest_frame(&self) -> Option<Arc<Frame>> {
        self.shared
            .latest
            .lock()
            .ok()
            .and_then(|frame| frame.clone())
    }
}

impl Drop for WindowsGraphicsScreenInput {
    fn drop(&mut self) {
        self.stop();
    }
}

fn describe_monitor(monitor: Monitor) -> Result<MonitorDescriptor, String> {
    let display_name = monitor
        .device_name()
        .map_err(|error| format!("could not read monitor device name: {error}"))?;
    let label = monitor.name().unwrap_or_else(|_| display_name.clone());
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: the wrapper exposes a live HMONITOR and info is writable.
    unsafe { GetMonitorInfoW(HMONITOR(monitor.as_raw_hmonitor()), (&raw mut info).cast()) }
        .ok()
        .map_err(|error| format!("could not read monitor bounds: {error}"))?;
    Ok(MonitorDescriptor {
        display_name,
        label,
        x: info.rcMonitor.left,
        y: info.rcMonitor.top,
        width: (info.rcMonitor.right - info.rcMonitor.left) as u32,
        height: (info.rcMonitor.bottom - info.rcMonitor.top) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_frame() -> Arc<Frame> {
        Arc::new(
            Frame::new(
                vec![255, 255, 255, 255].into(),
                Size::new(1, 1),
                4,
                1,
                0,
                Instant::now(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn native_screen_staging_reuses_textures_and_reads_pitched_rows() {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
            D3D11_USAGE_DEFAULT, D3D11CreateDevice,
        };
        use windows::Win32::Graphics::Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
        };
        let mut device = None;
        let mut context = None;
        // SAFETY: initializes owned test-only WARP interfaces through writable outputs.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .unwrap();
        let device = device.unwrap();
        let context = context.unwrap();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: 3,
            Height: 2,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            ..Default::default()
        };
        let pixels = [1, 2, 3, 255].repeat(6);
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: 12,
            SysMemSlicePitch: 24,
        };
        let mut source = None;
        // SAFETY: initial pixels cover every row of the single-subresource texture.
        unsafe { device.CreateTexture2D(&desc, Some(&initial), Some(&mut source)) }.unwrap();
        let source = source.unwrap();
        let mut staging = None;
        let texture = screen_staging(&mut staging, &device, &desc).unwrap();
        assert_eq!(
            texture,
            screen_staging(&mut staging, &device, &desc).unwrap()
        );
        for _ in 0..2 {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            // SAFETY: matching live resources; the previous iteration has unmapped staging.
            unsafe {
                context.CopyResource(&texture, &source);
                context.Map(&texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            }
            .unwrap();
            let _mapping = MappedScreen {
                context: &context,
                texture: &texture,
            };
            // SAFETY: staging remains mapped for the full read and normalization.
            let rows = unsafe {
                std::slice::from_raw_parts(mapped.pData.cast(), mapped.RowPitch as usize * 2)
            };
            let mut output = vec![0; 24];
            aspect_fit_bgra_into(
                rows,
                Size::new(3, 2),
                mapped.RowPitch,
                &mut output,
                Size::new(3, 2),
            )
            .unwrap();
            assert_eq!(output, pixels);
        }
        let resized = D3D11_TEXTURE2D_DESC { Width: 4, ..desc };
        assert_ne!(
            texture,
            screen_staging(&mut staging, &device, &resized).unwrap()
        );
    }

    #[test]
    fn native_replacement_generation_rejects_old_callbacks() {
        let shared = Shared::default();
        shared.generation.store(7, Ordering::Release);
        assert!(shared.is_current(7));
        shared.generation.fetch_add(1, Ordering::AcqRel);
        assert!(!shared.is_current(7));
        assert!(shared.is_current(8));
    }

    #[test]
    fn native_transient_processing_failures_retain_the_session_frame() {
        let shared = Shared::default();
        shared.generation.store(3, Ordering::Release);
        let frame = test_frame();
        *shared.latest.lock().unwrap() = Some(Arc::clone(&frame));

        let mut streak = ProcessingFailureStreak::default();
        for _ in 0..2 {
            assert!(!streak.failed());
            assert!(Arc::ptr_eq(
                shared.latest.lock().unwrap().as_ref().unwrap(),
                &frame
            ));
            assert!(shared.failure.lock().unwrap().is_none());
        }
    }

    #[test]
    fn native_third_consecutive_processing_failure_is_terminal() {
        let shared = Shared::default();
        shared.generation.store(3, Ordering::Release);
        *shared.latest.lock().unwrap() = Some(test_frame());

        let mut streak = ProcessingFailureStreak::default();
        assert!(!streak.failed());
        assert!(!streak.failed());
        assert!(streak.failed());
        shared.record_failure(3, "normalization failed three times");
        assert!(shared.latest.lock().unwrap().is_none());
        assert_eq!(
            shared.failure.lock().unwrap().as_deref(),
            Some("normalization failed three times")
        );
    }

    #[test]
    fn native_success_resets_the_processing_failure_streak() {
        let mut streak = ProcessingFailureStreak::default();
        assert!(!streak.failed());
        assert!(!streak.failed());
        streak.succeeded();
        assert!(!streak.failed());
        assert!(!streak.failed());
        assert!(streak.failed());
    }

    #[test]
    fn native_an_old_generation_cannot_clear_or_fail_a_new_session() {
        let shared = Shared::default();
        shared.generation.store(4, Ordering::Release);
        let frame = test_frame();
        *shared.latest.lock().unwrap() = Some(Arc::clone(&frame));

        shared.record_failure(3, "old callback failed");

        assert!(Arc::ptr_eq(
            shared.latest.lock().unwrap().as_ref().unwrap(),
            &frame
        ));
        assert!(shared.failure.lock().unwrap().is_none());
    }
}
