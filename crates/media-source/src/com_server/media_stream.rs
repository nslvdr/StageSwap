use super::OBJECTS;
use super::diagnostics;
use super::pipe_reader::PipeReader;
use stageswap_core::{
    CAPTURE_FRAME_POOL_CAPACITY, Frame, FrameBufferPool, PIPELINE_SIZE, SharedFrameCache,
    off_frame_pixels,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::Foundation::{E_POINTER, S_OK};
use windows::Win32::Media::KernelStreaming::PINNAME_VIDEO_CAPTURE;
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFAsyncCallback, IMFAsyncResult, IMFAttributes, IMFMediaEvent,
    IMFMediaEventGenerator_Impl, IMFMediaEventQueue, IMFMediaSource, IMFMediaStream_Impl,
    IMFMediaStream2, IMFMediaStream2_Impl, IMFSample, IMFStreamDescriptor,
    IMFVideoSampleAllocatorCallback, IMFVideoSampleAllocatorEx, MEMediaSample, MEStreamStarted,
    MEStreamStopped, MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES,
    MF_DEVICESTREAM_FRAMESERVER_SHARED, MF_DEVICESTREAM_STREAM_CATEGORY, MF_DEVICESTREAM_STREAM_ID,
    MF_E_INVALID_STATE_TRANSITION, MF_E_INVALIDREQUEST, MF_E_SHUTDOWN,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE,
    MF_MT_FIXED_SIZE_SAMPLES, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SAMPLE_SIZE, MF_MT_SUBTYPE,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_YUV_MATRIX, MF_STREAM_STATE, MF_STREAM_STATE_PAUSED,
    MF_STREAM_STATE_RUNNING, MF_STREAM_STATE_STOPPED, MFCreateAttributes, MFCreateEventQueue,
    MFCreateMediaType, MFCreateStreamDescriptor, MFCreateVideoSampleAllocatorEx,
    MFFrameSourceTypes_Color, MFGetSystemTime, MFMediaType_Video, MFNominalRange_16_235,
    MFVideoFormat_NV12, MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
    MFVideoTransferMatrix_BT601,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows_core::{Error, GUID, HRESULT, IUnknown, Interface, Ref, implement};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const STRIDE: u32 = WIDTH * 4;
const FRAME_BYTES: u32 = STRIDE * HEIGHT;
const NV12_FRAME_BYTES: u32 = WIDTH * HEIGHT * 3 / 2;
const FRAME_DURATION_100NS: i64 = 10_000_000 / 30;
const SAMPLE_ALLOCATOR_CAPACITY: u32 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BufferFormat {
    Rgb32,
    Nv12,
}

#[derive(Default)]
struct SampleAllocatorState {
    allocator: Option<IMFVideoSampleAllocatorEx>,
    subtype: Option<GUID>,
}

fn create_video_type(
    subtype: &GUID,
    stride: u32,
    sample_bytes: u32,
    bits_per_pixel: u32,
) -> windows_core::Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    // SAFETY: Media Foundation returns an owned, initially empty media type.
    let media_type = unsafe { MFCreateMediaType()? };
    // SAFETY: the media type is exclusively initialized before publication.
    unsafe {
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        media_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            (u64::from(WIDTH) << 32) | u64::from(HEIGHT),
        )?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, (30_u64 << 32) | 1)?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        media_type.SetUINT32(&MF_MT_DEFAULT_STRIDE, stride)?;
        media_type.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)?;
        media_type.SetUINT32(&MF_MT_SAMPLE_SIZE, sample_bytes)?;
        media_type.SetUINT32(&MF_MT_AVG_BITRATE, WIDTH * HEIGHT * bits_per_pixel * 30)?;
        if *subtype == MFVideoFormat_NV12 {
            media_type.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
            media_type.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT601.0 as u32)?;
        }
    }
    Ok(media_type)
}

fn limited_y(r: i32, g: i32, b: i32) -> u8 {
    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8
}

fn limited_u(r: i32, g: i32, b: i32) -> u8 {
    (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8
}

fn limited_v(r: i32, g: i32, b: i32) -> u8 {
    (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8
}

fn bgra_to_nv12(output: &mut [u8], bgra: &[u8]) {
    let y_plane_bytes = (WIDTH * HEIGHT) as usize;
    let (y_plane, uv_plane) = output.split_at_mut(y_plane_bytes);
    for y in (0..HEIGHT as usize).step_by(2) {
        for x in (0..WIDTH as usize).step_by(2) {
            let mut red = 0;
            let mut green = 0;
            let mut blue = 0;
            for offset_y in 0..2 {
                for offset_x in 0..2 {
                    let pixel = ((y + offset_y) * WIDTH as usize + x + offset_x) * 4;
                    let b = i32::from(bgra[pixel]);
                    let g = i32::from(bgra[pixel + 1]);
                    let r = i32::from(bgra[pixel + 2]);
                    y_plane[(y + offset_y) * WIDTH as usize + x + offset_x] = limited_y(r, g, b);
                    red += r;
                    green += g;
                    blue += b;
                }
            }
            let uv = (y / 2) * WIDTH as usize + x;
            uv_plane[uv] = limited_u(red / 4, green / 4, blue / 4);
            uv_plane[uv + 1] = limited_v(red / 4, green / 4, blue / 4);
        }
    }
}

struct StreamState {
    parent: Option<IMFMediaSource>,
    state: MF_STREAM_STATE,
    shutdown: bool,
}

#[derive(Clone, Copy)]
pub(super) struct StreamCheckpoint {
    state: MF_STREAM_STATE,
}

struct Nv12Cache {
    converted: Option<(Arc<Frame>, Arc<[u8]>)>,
    pool: FrameBufferPool,
    #[cfg(test)]
    conversions: u64,
}

impl Default for Nv12Cache {
    fn default() -> Self {
        Self {
            converted: None,
            pool: FrameBufferPool::new(NV12_FRAME_BYTES as usize, CAPTURE_FRAME_POOL_CAPACITY),
            #[cfg(test)]
            conversions: 0,
        }
    }
}

#[implement(IMFMediaStream2)]
pub(super) struct MediaStream {
    events: IMFMediaEventQueue,
    descriptor: IMFStreamDescriptor,
    attributes: IMFAttributes,
    frames: Arc<Mutex<SharedFrameCache>>,
    off_bgra: Arc<[u8]>,
    off_nv12: Arc<[u8]>,
    nv12_cache: Mutex<Nv12Cache>,
    sample_allocator: Mutex<SampleAllocatorState>,
    pipe_reader: Mutex<Option<PipeReader>>,
    state: Mutex<StreamState>,
    request_samples: AtomicU64,
    queued_samples: AtomicU64,
    sample_failures: AtomicU64,
}

impl MediaStream {
    pub(super) fn new(pipe_name: String) -> windows_core::Result<Self> {
        // SAFETY: Media Foundation constructors return owned COM interfaces.
        let events = unsafe { MFCreateEventQueue()? };
        let rgb32_type = create_video_type(&MFVideoFormat_RGB32, STRIDE, FRAME_BYTES, 32)?;
        let nv12_type = create_video_type(&MFVideoFormat_NV12, WIDTH, NV12_FRAME_BYTES, 12)?;
        // SAFETY: both media types remain alive for the descriptor construction call.
        let descriptor =
            unsafe { MFCreateStreamDescriptor(0, &[Some(rgb32_type.clone()), Some(nv12_type)])? };
        // SAFETY: descriptor owns its handler and advertised media types.
        unsafe {
            descriptor
                .GetMediaTypeHandler()?
                .SetCurrentMediaType(&rgb32_type)?
        };
        let mut attributes = None;
        // SAFETY: MFCreateAttributes initializes the provided COM out slot.
        unsafe { MFCreateAttributes(&mut attributes, 4)? };
        let attributes = attributes.ok_or_else(|| Error::from_hresult(E_POINTER))?;
        let descriptor_attributes: IMFAttributes = descriptor.cast()?;
        for store in [&attributes, &descriptor_attributes] {
            // SAFETY: both stores are exclusively initialized before publication.
            unsafe {
                store.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
                store.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
                store.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
                store.SetUINT32(
                    &MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES,
                    MFFrameSourceTypes_Color.0 as u32,
                )?;
            }
        }
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        let frames = Arc::new(Mutex::new(SharedFrameCache::default()));
        let pipe_reader = PipeReader::start(pipe_name, Arc::clone(&frames));
        let off_bgra = off_frame_pixels();
        let mut off_nv12 = vec![0; NV12_FRAME_BYTES as usize];
        bgra_to_nv12(&mut off_nv12, &off_bgra);
        Ok(Self {
            events,
            descriptor,
            attributes,
            frames,
            off_bgra,
            off_nv12: off_nv12.into(),
            nv12_cache: Mutex::new(Nv12Cache::default()),
            sample_allocator: Mutex::new(SampleAllocatorState::default()),
            pipe_reader: Mutex::new(Some(pipe_reader)),
            state: Mutex::new(StreamState {
                parent: None,
                state: MF_STREAM_STATE_STOPPED,
                shutdown: false,
            }),
            request_samples: AtomicU64::new(0),
            queued_samples: AtomicU64::new(0),
            sample_failures: AtomicU64::new(0),
        })
    }

    pub(super) fn attach_parent(&self, parent: IMFMediaSource) {
        self.state
            .lock()
            .expect("stream state lock poisoned")
            .parent = Some(parent);
    }

    pub(super) fn descriptor(&self) -> IMFStreamDescriptor {
        self.descriptor.clone()
    }
    pub(super) fn attributes(&self) -> IMFAttributes {
        self.attributes.clone()
    }
    pub(super) fn set_media_type(
        &self,
        media_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
    ) -> windows_core::Result<()> {
        let handler = unsafe { self.descriptor.GetMediaTypeHandler()? };
        let result = unsafe {
            handler.IsMediaTypeSupported(media_type, None)?;
            handler.SetCurrentMediaType(media_type)
        };
        if result.is_ok() {
            self.nv12_cache
                .lock()
                .expect("NV12 cache lock poisoned")
                .converted = None;
            self.initialize_sample_allocator(media_type)?;
        }
        result
    }

    fn initialize_sample_allocator(
        &self,
        media_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
    ) -> windows_core::Result<()> {
        let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE)? };
        {
            let state = self.state.lock().expect("stream state lock poisoned");
            if state.shutdown {
                return Err(Error::from_hresult(MF_E_SHUTDOWN));
            }
        }
        let mut state = self
            .sample_allocator
            .lock()
            .expect("sample allocator state poisoned");
        if state.subtype == Some(subtype) && state.allocator.is_some() {
            return Ok(());
        }
        if let Some(allocator) = state.allocator.take() {
            let _ = unsafe { allocator.UninitializeSampleAllocator() };
        }
        let mut raw = core::ptr::null_mut();
        unsafe {
            MFCreateVideoSampleAllocatorEx(&IMFVideoSampleAllocatorEx::IID, &mut raw)?;
        }
        if raw.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        let allocator = unsafe { IMFVideoSampleAllocatorEx::from_raw(raw) };
        unsafe {
            allocator.InitializeSampleAllocator(SAMPLE_ALLOCATOR_CAPACITY, media_type)?;
        }
        diagnostics::always(format!(
            "sample allocator initialized subtype={subtype:?} capacity={SAMPLE_ALLOCATOR_CAPACITY}"
        ));
        state.allocator = Some(allocator);
        state.subtype = Some(subtype);
        Ok(())
    }

    fn allocate_sample(&self) -> windows_core::Result<(IMFSample, Option<i32>)> {
        let allocator = self
            .sample_allocator
            .lock()
            .expect("sample allocator state poisoned")
            .allocator
            .clone()
            .ok_or_else(|| Error::from_hresult(MF_E_INVALIDREQUEST))?;
        let sample = unsafe { allocator.AllocateSample()? };
        let free_samples = allocator
            .cast::<IMFVideoSampleAllocatorCallback>()
            .ok()
            .and_then(|callback| unsafe { callback.GetFreeSampleCount() }.ok());
        Ok((sample, free_samples))
    }

    /// Keys conversion by the immutable frame object, including across pipe reconnects.
    fn nv12_frame(&self, frame: &Arc<Frame>) -> Arc<[u8]> {
        let mut cache = self.nv12_cache.lock().expect("NV12 cache lock poisoned");
        if let Some((cached_frame, bytes)) = &cache.converted
            && Arc::ptr_eq(cached_frame, frame)
        {
            return Arc::clone(bytes);
        }
        cache.converted = None;
        let bytes = cache
            .pool
            .write_with_fallback(|bytes| {
                bgra_to_nv12(bytes, frame.pixels());
                Ok::<(), std::convert::Infallible>(())
            })
            .unwrap();
        cache.converted = Some((Arc::clone(frame), Arc::clone(&bytes)));
        #[cfg(test)]
        {
            cache.conversions = cache.conversions.saturating_add(1);
        }
        bytes
    }
    pub(super) fn start(&self) -> windows_core::Result<i64> {
        let mut state = self.state.lock().expect("stream state lock poisoned");
        if state.shutdown {
            return Err(Error::from_hresult(MF_E_SHUTDOWN));
        }
        state.state = MF_STREAM_STATE_RUNNING;
        // SAFETY: MFGetSystemTime returns the current monotonic MF clock value.
        Ok(unsafe { MFGetSystemTime() })
    }

    pub(super) fn queue_started(
        &self,
        start_position: *const PROPVARIANT,
    ) -> windows_core::Result<()> {
        // SAFETY: queue and GUID pointer remain valid for the call.
        unsafe {
            self.events.QueueEventParamVar(
                MEStreamStarted.0 as u32,
                &GUID::zeroed(),
                S_OK,
                start_position,
            )
        }
    }

    pub(super) fn checkpoint(&self) -> windows_core::Result<StreamCheckpoint> {
        let state = self.state.lock().expect("stream state lock poisoned");
        if state.shutdown {
            return Err(Error::from_hresult(MF_E_SHUTDOWN));
        }
        Ok(StreamCheckpoint { state: state.state })
    }

    pub(super) fn restore(&self, checkpoint: StreamCheckpoint) {
        let mut state = self.state.lock().expect("stream state lock poisoned");
        if !state.shutdown {
            state.state = checkpoint.state;
        }
    }

    pub(super) fn stop(&self, send_event: bool) -> windows_core::Result<()> {
        let mut state = self.state.lock().expect("stream state lock poisoned");
        if state.shutdown {
            return Err(Error::from_hresult(MF_E_SHUTDOWN));
        }
        state.state = MF_STREAM_STATE_STOPPED;
        drop(state);
        if send_event {
            // SAFETY: queue and GUID pointer remain valid for the call.
            unsafe {
                self.events.QueueEventParamVar(
                    MEStreamStopped.0 as u32,
                    &GUID::zeroed(),
                    S_OK,
                    core::ptr::null(),
                )
            }
        } else {
            Ok(())
        }
    }

    pub(super) fn shutdown(&self) {
        let mut state = self.state.lock().expect("stream state lock poisoned");
        if state.shutdown {
            return;
        }
        state.shutdown = true;
        state.state = MF_STREAM_STATE_STOPPED;
        state.parent = None;
        drop(state);
        if let Ok(mut pipe_reader) = self.pipe_reader.lock()
            && let Some(mut pipe_reader) = pipe_reader.take()
        {
            pipe_reader.stop();
        }
        if let Ok(mut frames) = self.frames.lock() {
            frames.invalidate();
        }
        if let Ok(mut cache) = self.nv12_cache.lock() {
            *cache = Nv12Cache::default();
        }
        if let Ok(mut allocator) = self.sample_allocator.lock()
            && let Some(allocator) = allocator.allocator.take()
        {
            let _ = unsafe { allocator.UninitializeSampleAllocator() };
        }
        // SAFETY: shutting down an owned event queue is idempotent for our state machine.
        let _ = unsafe { self.events.Shutdown() };
    }

    fn make_output_sample(
        &self,
        token: Option<IUnknown>,
    ) -> windows_core::Result<(IMFSample, i64, Option<i32>)> {
        {
            let state = self.state.lock().expect("stream state lock poisoned");
            if state.shutdown {
                return Err(Error::from_hresult(MF_E_SHUTDOWN));
            }
            if state.state != MF_STREAM_STATE_RUNNING {
                return Err(Error::from_hresult(MF_E_INVALIDREQUEST));
            }
        }
        let live_frame = self
            .frames
            .lock()
            .expect("shared frame cache lock poisoned")
            .latest(std::time::Instant::now())
            .filter(|frame| frame.size == PIPELINE_SIZE && frame.stride == STRIDE);
        // Consumers select one of the advertised stream types on the descriptor.
        let media_type = unsafe {
            self.descriptor
                .GetMediaTypeHandler()?
                .GetCurrentMediaType()?
        };
        let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE)? };
        let (format, frame_bytes) = if subtype == MFVideoFormat_NV12 {
            (BufferFormat::Nv12, NV12_FRAME_BYTES)
        } else if subtype == MFVideoFormat_RGB32 {
            (BufferFormat::Rgb32, FRAME_BYTES)
        } else {
            return Err(Error::from_hresult(MF_E_INVALIDREQUEST));
        };
        let converted_nv12 = if format == BufferFormat::Nv12 {
            live_frame.as_ref().map(|frame| self.nv12_frame(frame))
        } else {
            None
        };
        if live_frame.is_none() {
            self.nv12_cache
                .lock()
                .expect("NV12 cache lock poisoned")
                .converted = None;
        }
        let (sample, free_samples) = self.allocate_sample()?;
        let buffer = unsafe { sample.GetBufferByIndex(0)? };
        let output: &[u8] = if format == BufferFormat::Nv12 {
            converted_nv12.as_deref().unwrap_or(self.off_nv12.as_ref())
        } else {
            live_frame
                .as_ref()
                .map_or(self.off_bgra.as_ref(), |frame| frame.pixels())
        };
        if output.len() != frame_bytes as usize {
            return Err(Error::from_hresult(E_POINTER));
        }
        let buffer_2d: IMF2DBuffer2 = buffer.cast()?;
        // SAFETY: the 2D buffer is owned by the Media Foundation allocator and the source
        // slice exactly matches the selected media type's contiguous layout.
        // ContiguousCopyFrom copies into the native surface pitch, including
        // the plane layout used by NV12.
        unsafe {
            buffer_2d.ContiguousCopyFrom(output)?;
        }
        {
            let state = self.state.lock().expect("stream state lock poisoned");
            if state.shutdown {
                return Err(Error::from_hresult(MF_E_SHUTDOWN));
            }
            if state.state != MF_STREAM_STATE_RUNNING {
                return Err(Error::from_hresult(MF_E_INVALIDREQUEST));
            }
        }
        // SAFETY: MFGetSystemTime is QPC-correlated and the allocator returned a valid sample.
        let timestamp = unsafe { MFGetSystemTime() };
        unsafe {
            sample.SetSampleTime(timestamp)?;
            sample.SetSampleDuration(FRAME_DURATION_100NS)?;
            if let Some(token) = token {
                sample.SetUnknown(
                    &windows::Win32::Media::MediaFoundation::MFSampleExtension_Token,
                    &token,
                )?;
            }
        }
        Ok((sample, timestamp, free_samples))
    }
}

impl Drop for MediaStream {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Release);
    }
}

impl IMFMediaEventGenerator_Impl for MediaStream_Impl {
    fn GetEvent(
        &self,
        flags: windows::Win32::Media::MediaFoundation::MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS,
    ) -> windows_core::Result<IMFMediaEvent> {
        // SAFETY: delegates to the owned event queue.
        unsafe { self.events.GetEvent(flags.0) }
    }
    fn BeginGetEvent(
        &self,
        callback: Ref<IMFAsyncCallback>,
        state: Ref<IUnknown>,
    ) -> windows_core::Result<()> {
        // SAFETY: borrowed interfaces stay alive for the delegated call.
        unsafe { self.events.BeginGetEvent(callback.as_ref(), state.as_ref()) }
    }
    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> windows_core::Result<IMFMediaEvent> {
        // SAFETY: borrowed result stays alive for the delegated call.
        unsafe { self.events.EndGetEvent(result.as_ref()) }
    }
    fn QueueEvent(
        &self,
        event_type: u32,
        extended_type: *const GUID,
        status: HRESULT,
        value: *const PROPVARIANT,
    ) -> windows_core::Result<()> {
        // SAFETY: forwards the exact Media Foundation event arguments.
        unsafe {
            self.events
                .QueueEventParamVar(event_type, extended_type, status, value)
        }
    }
}

impl IMFMediaStream_Impl for MediaStream_Impl {
    fn GetMediaSource(&self) -> windows_core::Result<IMFMediaSource> {
        let state = self.state.lock().expect("stream state lock poisoned");
        if state.shutdown {
            return Err(Error::from_hresult(MF_E_SHUTDOWN));
        }
        state
            .parent
            .clone()
            .ok_or_else(|| Error::from_hresult(MF_E_SHUTDOWN))
    }
    fn GetStreamDescriptor(&self) -> windows_core::Result<IMFStreamDescriptor> {
        if self
            .state
            .lock()
            .expect("stream state lock poisoned")
            .shutdown
        {
            return Err(Error::from_hresult(MF_E_SHUTDOWN));
        }
        Ok(self.descriptor.clone())
    }
    fn RequestSample(&self, token: Ref<IUnknown>) -> windows_core::Result<()> {
        let request_count = self.request_samples.fetch_add(1, Ordering::Relaxed) + 1;
        diagnostics::rate_limited(
            "sample-requests",
            Duration::from_secs(5),
            format!("sample requests count={request_count}"),
        );
        let (sample, timestamp, free_samples) = match self.make_output_sample(token.cloned()) {
            Ok(sample) => sample,
            Err(error) => {
                let failure_count = self.sample_failures.fetch_add(1, Ordering::Relaxed) + 1;
                let allocator_free = self
                    .sample_allocator
                    .lock()
                    .ok()
                    .and_then(|state| state.allocator.clone())
                    .and_then(|allocator| allocator.cast::<IMFVideoSampleAllocatorCallback>().ok())
                    .and_then(|callback| unsafe { callback.GetFreeSampleCount() }.ok());
                diagnostics::rate_limited(
                    "sample-creation-failures",
                    Duration::from_secs(5),
                    format!(
                        "sample creation failed count={failure_count} allocator_free={allocator_free:?} error={error}"
                    ),
                );
                return Err(error);
            }
        };
        // SAFETY: the sample remains alive for the queue call and is AddRef'd by the event.
        let result = unsafe {
            self.events
                .QueueEventParamUnk(MEMediaSample.0 as u32, &GUID::zeroed(), S_OK, &sample)
        };
        match result {
            Ok(()) => {
                let queued_count = self.queued_samples.fetch_add(1, Ordering::Relaxed) + 1;
                diagnostics::rate_limited(
                    "sample-queued",
                    Duration::from_secs(5),
                    format!(
                        "samples queued count={queued_count} timestamp_100ns={timestamp} allocator_free={free_samples:?}"
                    ),
                );
                Ok(())
            }
            Err(error) => {
                let failure_count = self.sample_failures.fetch_add(1, Ordering::Relaxed) + 1;
                diagnostics::rate_limited(
                    "sample-queue-failures",
                    Duration::from_secs(5),
                    format!("sample queue failed count={failure_count} error={error}"),
                );
                Err(error)
            }
        }
    }
}

impl IMFMediaStream2_Impl for MediaStream_Impl {
    fn SetStreamState(&self, value: MF_STREAM_STATE) -> windows_core::Result<()> {
        if value != MF_STREAM_STATE_STOPPED
            && value != MF_STREAM_STATE_RUNNING
            && value != MF_STREAM_STATE_PAUSED
        {
            return Err(Error::from_hresult(MF_E_INVALID_STATE_TRANSITION));
        }
        {
            let state = self.state.lock().expect("stream state lock poisoned");
            if state.shutdown {
                return Err(Error::from_hresult(MF_E_SHUTDOWN));
            }
            if value == MF_STREAM_STATE_PAUSED && state.state != MF_STREAM_STATE_RUNNING {
                return Err(Error::from_hresult(MF_E_INVALID_STATE_TRANSITION));
            }
        }
        if value == MF_STREAM_STATE_RUNNING {
            let handler = unsafe { self.descriptor.GetMediaTypeHandler()? };
            let media_type = unsafe { handler.GetCurrentMediaType()? };
            self.initialize_sample_allocator(&media_type)?;
        }
        let mut state = self.state.lock().expect("stream state lock poisoned");
        if state.shutdown {
            return Err(Error::from_hresult(MF_E_SHUTDOWN));
        }
        state.state = value;
        Ok(())
    }
    fn GetStreamState(&self) -> windows_core::Result<MF_STREAM_STATE> {
        let state = self.state.lock().expect("stream state lock poisoned");
        if state.shutdown {
            return Err(Error::from_hresult(MF_E_SHUTDOWN));
        }
        Ok(state.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::MediaFoundation::{
        MF_E_SAMPLEALLOCATOR_EMPTY, MF_VERSION, MFSTARTUP_FULL, MFShutdown, MFStartup,
    };

    struct MediaFoundation;

    impl Drop for MediaFoundation {
        fn drop(&mut self) {
            // SAFETY: balances the successful MFStartup in this test.
            let _ = unsafe { MFShutdown() };
        }
    }

    #[test]
    fn native_rgb32_is_default_and_nv12_remains_selectable() -> windows_core::Result<()> {
        let _test_lock = super::super::TEST_LOCK
            .lock()
            .expect("media-source test lock poisoned");
        // SAFETY: initializes Media Foundation for this test process.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        let stream = MediaStream::new(r"\\.\pipe\StageSwap.Nonexistent.Test".into())?;
        let handler = unsafe { stream.descriptor.GetMediaTypeHandler()? };
        assert_eq!(unsafe { handler.GetMediaTypeCount()? }, 2);
        let current = unsafe { handler.GetCurrentMediaType()? };
        assert_eq!(
            unsafe { current.GetGUID(&MF_MT_SUBTYPE)? },
            MFVideoFormat_RGB32
        );
        stream.set_media_type(&current)?;
        stream.start()?;
        let (first, first_timestamp, _) = stream.make_output_sample(None)?;
        // SAFETY: the sample owns one contiguous RGB32 buffer.
        let buffer = unsafe { first.ConvertToContiguousBuffer()? };
        let mut bytes = core::ptr::null_mut();
        let mut length = 0;
        // SAFETY: Lock initializes bytes and length for this live buffer.
        unsafe { buffer.Lock(&mut bytes, None, Some(&mut length))? };
        assert_eq!(length, FRAME_BYTES);
        let pixels = unsafe { core::slice::from_raw_parts(bytes, length as usize) };
        assert_eq!(pixels, off_frame_pixels().as_ref());
        // SAFETY: balances the successful Lock.
        unsafe { buffer.Unlock()? };

        let nv12 = unsafe { handler.GetMediaTypeByIndex(1)? };
        assert_eq!(unsafe { nv12.GetGUID(&MF_MT_SUBTYPE)? }, MFVideoFormat_NV12);
        assert_eq!(
            unsafe { nv12.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE)? },
            MFNominalRange_16_235.0 as u32
        );
        assert_eq!(
            unsafe { nv12.GetUINT32(&MF_MT_YUV_MATRIX)? },
            MFVideoTransferMatrix_BT601.0 as u32
        );
        stream.set_media_type(&nv12)?;
        let (second, second_timestamp, _) = stream.make_output_sample(None)?;
        assert_eq!(unsafe { first.GetSampleTime()? }, first_timestamp);
        assert_eq!(unsafe { second.GetSampleTime()? }, second_timestamp);
        assert!(second_timestamp >= first_timestamp);
        assert_eq!(unsafe { second.GetSampleDuration()? }, FRAME_DURATION_100NS);
        let buffer = unsafe { second.ConvertToContiguousBuffer()? };
        unsafe { buffer.Lock(&mut bytes, None, Some(&mut length))? };
        assert_eq!(length, NV12_FRAME_BYTES);
        let pixels = unsafe { core::slice::from_raw_parts(bytes, length as usize) };
        let mut expected = vec![0; NV12_FRAME_BYTES as usize];
        bgra_to_nv12(&mut expected, &off_frame_pixels());
        assert_eq!(pixels, expected);
        unsafe { buffer.Unlock()? };
        stream.stop(false)?;
        stream.shutdown();
        Ok(())
    }

    #[test]
    fn native_sample_allocator_backpressure_and_reuse_after_release() -> windows_core::Result<()> {
        let _test_lock = super::super::TEST_LOCK
            .lock()
            .expect("media-source test lock poisoned");
        // SAFETY: initializes Media Foundation for this test process.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        let stream = MediaStream::new(r"\\.\pipe\StageSwap.Nonexistent.Pool.Test".into())?;
        let handler = unsafe { stream.descriptor.GetMediaTypeHandler()? };
        for media_type_index in 0..2 {
            let media_type = unsafe { handler.GetMediaTypeByIndex(media_type_index)? };
            stream.set_media_type(&media_type)?;
            stream.start()?;
            let mut samples = Vec::new();
            let mut buffer_pointers = Vec::new();
            for _ in 0..SAMPLE_ALLOCATOR_CAPACITY {
                let (sample, _, _) = stream.make_output_sample(None)?;
                let buffer = unsafe { sample.GetBufferByIndex(0)? };
                buffer_pointers.push(buffer.as_raw());
                samples.push(sample);
            }
            assert_eq!(
                stream.make_output_sample(None).unwrap_err().code(),
                MF_E_SAMPLEALLOCATOR_EMPTY
            );
            let released = samples.remove(0);
            drop(released);
            let (reused, _, _) = stream.make_output_sample(None)?;
            let reused_buffer = unsafe { reused.GetBufferByIndex(0)? };
            assert_eq!(reused_buffer.as_raw(), buffer_pointers[0]);
            assert!(buffer_pointers.iter().enumerate().all(|(index, pointer)| {
                buffer_pointers[..index]
                    .iter()
                    .all(|other| other != pointer)
            }));
            drop(reused_buffer);
            drop(reused);
            drop(samples);
            stream.stop(false)?;
        }

        stream.shutdown();
        Ok(())
    }

    #[test]
    fn native_allocator_sample_survives_source_shutdown_without_pinning_dll()
    -> windows_core::Result<()> {
        let _test_lock = super::super::TEST_LOCK
            .lock()
            .expect("media-source test lock poisoned");
        // SAFETY: initializes Media Foundation for this test process.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        let stream = MediaStream::new(r"\\.\pipe\StageSwap.Nonexistent.Lease.Test".into())?;
        let media_type = unsafe {
            stream
                .descriptor
                .GetMediaTypeHandler()?
                .GetCurrentMediaType()?
        };
        stream.set_media_type(&media_type)?;
        stream.start()?;
        let (sample, _, _) = stream.make_output_sample(None)?;
        stream.stop(false)?;
        stream.shutdown();
        drop(stream);

        assert_eq!(super::super::DllCanUnloadNow(), S_OK);
        drop(sample);
        assert_eq!(super::super::DllCanUnloadNow(), S_OK);
        Ok(())
    }

    #[test]
    fn native_stream_shutdown_is_idempotent_and_joins_the_pipe_worker_while_alive()
    -> windows_core::Result<()> {
        let _test_lock = super::super::TEST_LOCK
            .lock()
            .expect("media-source test lock poisoned");
        // SAFETY: initializes Media Foundation for this test process.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        let stream = MediaStream::new(r"\\.\pipe\StageSwap.Nonexistent.Shutdown.Test".into())?;

        stream.shutdown();
        assert!(stream.pipe_reader.lock().unwrap().is_none());
        assert!(
            stream
                .frames
                .lock()
                .unwrap()
                .latest(std::time::Instant::now())
                .is_none()
        );
        stream.shutdown();
        assert!(stream.pipe_reader.lock().unwrap().is_none());
        Ok(())
    }

    #[test]
    fn native_limited_bt601_conversion_matches_black_white_and_primary_bars() {
        assert_eq!(limited_y(0, 0, 0), 16);
        assert_eq!(limited_y(255, 255, 255), 235);
        assert_eq!(limited_y(255, 0, 0), 82);
        assert_eq!(limited_y(0, 255, 0), 144);
        assert_eq!(limited_y(0, 0, 255), 41);
        assert_eq!(
            (limited_u(128, 128, 128), limited_v(128, 128, 128)),
            (128, 128)
        );
    }

    #[test]
    fn native_nv12_cache_converts_once_per_source_sequence() -> windows_core::Result<()> {
        let _test_lock = super::super::TEST_LOCK
            .lock()
            .expect("media-source test lock poisoned");
        // SAFETY: initializes Media Foundation for this test process.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        let _foundation = MediaFoundation;
        let stream = MediaStream::new(r"\\.\pipe\StageSwap.Nonexistent.CacheTest".into())?;
        let frame = Arc::new(Frame::placeholder(
            PIPELINE_SIZE,
            0xff000000,
            7,
            0,
            std::time::Instant::now(),
        ));
        let first = stream.nv12_frame(&frame);
        let second = stream.nv12_frame(&frame);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            stream
                .nv12_cache
                .lock()
                .expect("NV12 cache lock poisoned")
                .conversions,
            1
        );
        // A new connection may reuse sequence 7 for different pixels.
        let replacement = Arc::new(Frame::placeholder(
            PIPELINE_SIZE,
            0xffffffff,
            7,
            0,
            std::time::Instant::now(),
        ));
        let third = stream.nv12_frame(&replacement);
        assert_ne!(first.as_ref(), third.as_ref());
        let pointer = first.as_ptr();
        drop(first);
        drop(second);
        let next = stream.nv12_frame(&frame);
        assert_eq!(next.as_ptr(), pointer);
        assert_eq!(stream.nv12_cache.lock().unwrap().pool.allocated_slots(), 2);
        assert!(!Arc::ptr_eq(&next, &third));
        assert_eq!(
            stream
                .nv12_cache
                .lock()
                .expect("NV12 cache lock poisoned")
                .conversions,
            3
        );
        Ok(())
    }
}
