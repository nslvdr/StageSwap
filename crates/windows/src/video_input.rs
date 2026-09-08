use crate::{InputDevice, VideoInput, WebcamError};
use stageswap_core::WebcamFailureKind;
use stageswap_core::{
    CAPTURE_FRAME_POOL_CAPACITY, Frame, FrameBufferPool, FramePacer, PIPELINE_SIZE, Size,
    aspect_fill_bgra_into, aspect_fit_bgra_into,
};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFActivate, IMFMediaEvent, IMFMediaType, IMFSample, IMFSourceReader,
    IMFSourceReaderCallback, IMFSourceReaderCallback_Impl, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
    MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED, MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED,
    MF_MT_DEFAULT_STRIDE, MF_MT_FIXED_SIZE_SAMPLES, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_GEOMETRIC_APERTURE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SAMPLE_SIZE, MF_MT_SUBTYPE,
    MF_SOURCE_READER_ASYNC_CALLBACK, MF_SOURCE_READER_CURRENT_TYPE_INDEX,
    MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ERROR,
    MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED, MF_VERSION, MF2DBuffer_LockFlags_Read,
    MFCreateAttributes, MFCreateDeviceSource, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFMediaType_Video, MFSTARTUP_FULL,
    MFShutdown, MFStartup, MFVideoArea, MFVideoFormat_MJPG, MFVideoFormat_NV12,
    MFVideoFormat_RGB32, MFVideoFormat_YUY2, MFVideoInterlace_Progressive,
};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_core::{HRESULT, Interface, PCWSTR, PWSTR, Ref, implement};

const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const WEBCAM_FAILURE_THRESHOLD: u32 = 3;
const WEBCAM_FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 30);
const WEBCAM_FRAME_EARLY_TOLERANCE: Duration = Duration::from_millis(1);
const TARGET_WEBCAM_ASPECT_RATIO: f64 = 16.0 / 9.0;
const WEBCAM_ASPECT_RATIO_TOLERANCE: f64 = 0.01;

fn should_aspect_fill(enabled: bool, display_aspect_ratio: Option<f64>) -> bool {
    enabled
        && display_aspect_ratio.is_some_and(|ratio| {
            ratio.is_finite()
                && ratio > 0.0
                && ((ratio - TARGET_WEBCAM_ASPECT_RATIO) / TARGET_WEBCAM_ASPECT_RATIO).abs()
                    > WEBCAM_ASPECT_RATIO_TOLERANCE
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebcamCaptureFailure {
    pub kind: WebcamFailureKind,
    pub message: String,
    pub hresult: Option<i32>,
    pub device_tag: u64,
    pub capture_generation: u64,
}

fn device_identity_tag(device_id: &str) -> u64 {
    device_id.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn webcam_error_message(
    context: &str,
    code: HRESULT,
    device_tag: u64,
    capture_generation: u64,
) -> String {
    let details = if device_tag == 0 {
        format!(
            "HRESULT=0x{:08X}, device=unknown, capture_generation={capture_generation}",
            code.0 as u32
        )
    } else {
        format!(
            "HRESULT=0x{:08X}, device_tag={device_tag:016x}, capture_generation={capture_generation}",
            code.0 as u32,
        )
    };
    if code == E_ACCESSDENIED {
        return format!(
            "{context}: camera access was denied or camera privacy is disabled; open Windows Settings > Privacy & security > Camera ({details})"
        );
    }
    if code == MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED {
        return format!(
            "{context}: the webcam is in use; close Zoom, Teams, or another application using it, then restart the webcam ({details})"
        );
    }
    if code == MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED {
        return format!(
            "{context}: the webcam was disconnected or invalidated; StageSwap will retry the saved webcam automatically, or you can reconnect it and use Restart Webcam if recovery is exhausted ({details})"
        );
    }
    format!(
        "{context}: {} ({details})",
        windows_core::Error::from_hresult(code)
    )
}

#[derive(Clone, Copy, Debug)]
struct NegotiatedFormat {
    size: Size,
    stride: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WebcamFormatCandidate {
    size: Size,
    frame_rate_numerator: u32,
    frame_rate_denominator: u32,
    subtype: &'static str,
    subtype_rank: u8,
}

fn sort_webcam_formats(formats: &mut [WebcamFormatCandidate]) {
    formats.sort_by(|left, right| {
        let fixed_rank = |candidate: &WebcamFormatCandidate| {
            let exact_rgb32 = !(candidate.subtype_rank == 0
                && candidate.size == PIPELINE_SIZE
                && u64::from(candidate.frame_rate_numerator)
                    == 30 * u64::from(candidate.frame_rate_denominator));
            let rgb32_fallback = candidate.subtype_rank != 0;
            let aspect_error = (i64::from(candidate.size.width) * 9
                - i64::from(candidate.size.height) * 16)
                .unsigned_abs();
            let resolution_error = (i64::from(candidate.size.width)
                * i64::from(candidate.size.height)
                - i64::from(PIPELINE_SIZE.width) * i64::from(PIPELINE_SIZE.height))
            .unsigned_abs();
            (exact_rgb32, rgb32_fallback, aspect_error, resolution_error)
        };
        fixed_rank(left)
            .cmp(&fixed_rank(right))
            .then_with(|| {
                let left_error = (i64::from(left.frame_rate_numerator)
                    - 30 * i64::from(left.frame_rate_denominator))
                .unsigned_abs();
                let right_error = (i64::from(right.frame_rate_numerator)
                    - 30 * i64::from(right.frame_rate_denominator))
                .unsigned_abs();
                (u128::from(left_error) * u128::from(right.frame_rate_denominator))
                    .cmp(&(u128::from(right_error) * u128::from(left.frame_rate_denominator)))
            })
            .then_with(|| left.subtype_rank.cmp(&right.subtype_rank))
    });
}

fn ranked_native_formats(reader: &IMFSourceReader) -> Vec<WebcamFormatCandidate> {
    let mut formats = Vec::new();
    for index in 0.. {
        // SAFETY: the reader is live; enumeration ends when Media Foundation returns an error.
        let Ok(media_type) = (unsafe { reader.GetNativeMediaType(STREAM, index) }) else {
            break;
        };
        // SAFETY: native media-type attributes are scalar values owned by media_type.
        let Ok(subtype) = (unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }) else {
            continue;
        };
        let (subtype, subtype_rank) = if subtype == MFVideoFormat_RGB32 {
            ("RGB32", 0)
        } else if subtype == MFVideoFormat_NV12 {
            ("NV12", 1)
        } else if subtype == MFVideoFormat_YUY2 {
            ("YUY2", 2)
        } else if subtype == MFVideoFormat_MJPG {
            ("MJPG", 3)
        } else {
            continue;
        };
        let Ok(frame_size) = (unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }) else {
            continue;
        };
        let Ok(frame_rate) = (unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }) else {
            continue;
        };
        let interlace = unsafe { media_type.GetUINT32(&MF_MT_INTERLACE_MODE) }.ok();
        if interlace.is_some_and(|mode| mode != MFVideoInterlace_Progressive.0 as u32) {
            continue;
        }
        let candidate = WebcamFormatCandidate {
            size: Size::new((frame_size >> 32) as u32, frame_size as u32),
            frame_rate_numerator: (frame_rate >> 32) as u32,
            frame_rate_denominator: frame_rate as u32,
            subtype,
            subtype_rank,
        };
        if candidate.size.width > 0
            && candidate.size.height > 0
            && candidate.frame_rate_numerator > 0
            && candidate.frame_rate_denominator > 0
            && !formats.contains(&candidate)
        {
            formats.push(candidate);
        }
    }
    sort_webcam_formats(&mut formats);
    formats
}

fn negotiate_rgb32_output(
    reader: &IMFSourceReader,
) -> Result<(NegotiatedFormat, String, String), WebcamError> {
    let native = ranked_native_formats(reader);
    let native_description = native.first().map_or_else(
        || "no ranked native format".to_owned(),
        |format| {
            format!(
                "{} {}x{} {}/{} fps subtype-rank {}",
                format.subtype,
                format.size.width,
                format.size.height,
                format.frame_rate_numerator,
                format.frame_rate_denominator,
                format.subtype_rank
            )
        },
    );
    let exact = WebcamFormatCandidate {
        size: PIPELINE_SIZE,
        frame_rate_numerator: 30,
        frame_rate_denominator: 1,
        subtype: "RGB32",
        subtype_rank: 0,
    };
    let mut candidates = vec![exact];
    candidates.extend(native.into_iter().filter(|candidate| *candidate != exact));
    let mut last_error = "camera exposed no convertible progressive format".to_owned();
    for candidate in candidates {
        let output: IMFMediaType = unsafe { MFCreateMediaType() }.map_err(|error| {
            WebcamError::new(
                webcam_hresult_kind(error.code()),
                format!("could not create video output type: {error}"),
            )
        })?;
        let frame_size = (u64::from(candidate.size.width) << 32) | u64::from(candidate.size.height);
        let frame_rate = (u64::from(candidate.frame_rate_numerator) << 32)
            | u64::from(candidate.frame_rate_denominator);
        let Some(row_bytes) = candidate.size.width.checked_mul(4) else {
            last_error = format!(
                "{} {}x{} at {}/{} fps has an overflowing RGB32 row size",
                candidate.subtype,
                candidate.size.width,
                candidate.size.height,
                candidate.frame_rate_numerator,
                candidate.frame_rate_denominator
            );
            continue;
        };
        let Some(sample_size) = row_bytes.checked_mul(candidate.size.height) else {
            last_error = format!(
                "{} {}x{} at {}/{} fps has an overflowing RGB32 sample size",
                candidate.subtype,
                candidate.size.width,
                candidate.size.height,
                candidate.frame_rate_numerator,
                candidate.frame_rate_denominator
            );
            continue;
        };
        let configured = (|| -> windows_core::Result<()> {
            // SAFETY: reader, media type, and attributes are live.
            unsafe {
                output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
                output.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)?;
                output.SetUINT64(&MF_MT_FRAME_RATE, frame_rate)?;
                output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                output.SetUINT32(&MF_MT_DEFAULT_STRIDE, row_bytes)?;
                output.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)?;
                output.SetUINT32(&MF_MT_SAMPLE_SIZE, sample_size)?;
                reader.SetCurrentMediaType(STREAM, None, &output)?;
            }
            Ok(())
        })();
        if let Err(error) = configured {
            let kind = webcam_hresult_kind(error.code());
            if kind != WebcamFailureKind::DriverFailure {
                return Err(WebcamError::new(
                    kind,
                    format!("could not configure webcam output: {error}"),
                ));
            }
            last_error = format!(
                "{} {}x{} at {}/{} fps was rejected: {error}",
                candidate.subtype,
                candidate.size.width,
                candidate.size.height,
                candidate.frame_rate_numerator,
                candidate.frame_rate_denominator
            );
            continue;
        }
        // SAFETY: the reader is live and returns a retained media type.
        let current = unsafe { reader.GetCurrentMediaType(STREAM) }.map_err(|error| {
            WebcamError::new(
                webcam_hresult_kind(error.code()),
                format!("could not read negotiated webcam format: {error}"),
            )
        })?;
        match validate_negotiated_type(&current) {
            Ok(format) => {
                let output_description = format!(
                    "RGB32 {}x{} stride {}",
                    format.size.width, format.size.height, format.stride
                );
                return Ok((format, native_description, output_description));
            }
            Err(error) => last_error = error,
        }
    }
    Err(WebcamError::new(
        WebcamFailureKind::UnsupportedFormat,
        format!("could not negotiate a webcam format: {last_error}"),
    ))
}

fn copy_bgra_rows(
    source: &[u8],
    scanline0_offset: usize,
    stride: i32,
    size: Size,
    destination: &mut [u8],
) -> Result<(), String> {
    let row_bytes = usize::try_from(size.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "webcam row size overflowed".to_owned())?;
    let destination_length = row_bytes
        .checked_mul(size.height as usize)
        .ok_or_else(|| "webcam frame size overflowed".to_owned())?;
    if (stride.unsigned_abs() as usize) < row_bytes {
        return Err(format!(
            "webcam stride {stride} is shorter than the {row_bytes}-byte RGB32 row"
        ));
    }
    if destination.len() != destination_length {
        return Err("webcam destination has an invalid length".into());
    }
    for row in 0..size.height as usize {
        let source_offset = (scanline0_offset as i128)
            .checked_add((row as i128) * i128::from(stride))
            .filter(|offset| *offset >= 0)
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or_else(|| "webcam stride points outside the sample buffer".to_owned())?;
        let source_end = source_offset
            .checked_add(row_bytes)
            .filter(|end| *end <= source.len())
            .ok_or_else(|| {
                "webcam sample buffer is too short for its declared stride".to_owned()
            })?;
        let destination_offset = row * row_bytes;
        destination[destination_offset..destination_offset + row_bytes]
            .copy_from_slice(&source[source_offset..source_end]);
    }
    Ok(())
}

fn media_type_display_aspect_ratio(media_type: &IMFMediaType) -> Option<f64> {
    let display_size = [&MF_MT_MINIMUM_DISPLAY_APERTURE, &MF_MT_GEOMETRIC_APERTURE]
        .into_iter()
        .find_map(|attribute| {
            let mut bytes = [0u8; size_of::<MFVideoArea>()];
            // SAFETY: bytes is writable storage of exactly the expected structure size.
            unsafe { media_type.GetBlob(attribute, &mut bytes, None) }
                .ok()
                .and_then(|()| {
                    // SAFETY: Media Foundation wrote an MFVideoArea blob; read_unaligned handles
                    // the byte buffer's alignment.
                    let area =
                        unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<MFVideoArea>()) };
                    (area.Area.cx > 0 && area.Area.cy > 0)
                        .then_some((area.Area.cx as u32, area.Area.cy as u32))
                })
        })
        .or_else(|| {
            // SAFETY: media_type is a live IMFMediaType and the key stores a packed ratio.
            let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.ok()?;
            let width = (frame_size >> 32) as u32;
            let height = frame_size as u32;
            (width > 0 && height > 0).then_some((width, height))
        })?;
    // SAFETY: media_type is live; a missing pixel-aspect attribute means square pixels.
    let pixel_aspect = unsafe { media_type.GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO) }
        .ok()
        .and_then(|ratio| {
            let numerator = (ratio >> 32) as u32;
            let denominator = ratio as u32;
            (numerator > 0 && denominator > 0).then_some((numerator, denominator))
        })
        .unwrap_or((1, 1));
    let ratio = display_size.0 as f64 / display_size.1 as f64 * pixel_aspect.0 as f64
        / pixel_aspect.1 as f64;
    ratio.is_finite().then_some(ratio)
}

fn current_native_display_aspect_ratio(reader: &IMFSourceReader) -> Option<f64> {
    // SAFETY: reader is live and CURRENT_TYPE_INDEX requests a copy of its current native type.
    let media_type =
        unsafe { reader.GetNativeMediaType(STREAM, MF_SOURCE_READER_CURRENT_TYPE_INDEX.0 as u32) }
            .ok()?;
    media_type_display_aspect_ratio(&media_type)
}

pub fn enumerate_video_devices() -> Result<Vec<InputDevice>, String> {
    Ok(enumerate_all_video_devices()?
        .into_iter()
        .filter(|device| !device.is_virtual)
        .collect())
}

pub(super) fn enumerate_all_video_devices() -> Result<Vec<InputDevice>, String> {
    let mut attributes = None;
    // SAFETY: output storage is writable and Media Foundation is initialized by
    // the owning runtime thread.
    unsafe { MFCreateAttributes(&mut attributes, 1) }
        .map_err(|error| format!("could not create device attributes: {error}"))?;
    let attributes = attributes.expect("MFCreateAttributes returned no attributes");
    // SAFETY: attributes is a live IMFAttributes instance.
    unsafe {
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )
    }
    .map_err(|error| format!("could not select video capture devices: {error}"))?;
    let mut raw = core::ptr::null_mut();
    let mut count = 0;
    // SAFETY: both output pointers are writable and released below.
    unsafe { MFEnumDeviceSources(&attributes, &mut raw, &mut count) }
        .map_err(|error| format!("could not enumerate video devices: {error}"))?;
    let mut result = Vec::with_capacity(count as usize);
    if count == 0 {
        // Media Foundation may represent an empty array with a null pointer.
        unsafe { CoTaskMemFree((!raw.is_null()).then_some(raw.cast())) };
        return Ok(result);
    }
    if raw.is_null() {
        return Err("video device enumeration returned an empty array pointer".into());
    }
    // SAFETY: MFEnumDeviceSources returned an array of count COM interface slots.
    let activations = unsafe { core::slice::from_raw_parts_mut(raw, count as usize) };
    for activation in activations {
        let Some(activation) = activation.take() else {
            continue;
        };
        let name = allocated_string(&activation, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
            .unwrap_or_else(|| "Unnamed video device".into());
        let id = allocated_string(
            &activation,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        )
        .unwrap_or_default();
        let is_virtual = is_managed_virtual_camera(&name);
        if !id.is_empty() {
            result.push(InputDevice {
                id,
                name,
                is_virtual,
            });
        }
    }
    // SAFETY: Media Foundation allocated the interface-array block with CoTaskMemAlloc.
    unsafe { CoTaskMemFree(Some(raw.cast())) };
    Ok(result)
}

fn is_managed_virtual_camera(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("stageswap") || name.contains("automatic screen camera")
}

fn allocated_string(activation: &IMFActivate, key: &windows_core::GUID) -> Option<String> {
    let mut value = PWSTR::null();
    let mut length = 0;
    // SAFETY: both output pointers are writable and value is freed below.
    if unsafe { activation.GetAllocatedString(key, &mut value, &mut length) }.is_err() {
        return None;
    }
    if length == 0 {
        unsafe { CoTaskMemFree((!value.is_null()).then_some(value.0.cast())) };
        return Some(String::new());
    }
    if value.is_null() {
        return None;
    }
    // SAFETY: GetAllocatedString returned `length` UTF-16 code units.
    let result =
        unsafe { String::from_utf16(core::slice::from_raw_parts(value.0, length as usize)) }.ok();
    // SAFETY: Media Foundation allocated this string with CoTaskMemAlloc.
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    result
}

fn resolve_negotiated_layout(
    size: Size,
    default_stride: Option<u32>,
    sample_size: Option<u32>,
) -> Result<NegotiatedFormat, String> {
    let row_bytes = size
        .width
        .checked_mul(4)
        .ok_or_else(|| "negotiated webcam row size overflowed".to_owned())?;
    let row_bytes_i32 = i32::try_from(row_bytes)
        .map_err(|_| "negotiated webcam row size exceeds the supported stride range".to_owned())?;
    // MF_MT_DEFAULT_STRIDE is a UINT32 attribute carrying a signed stride. It is a
    // default contiguous stride, not the pitch of every individual surface. When
    // the attribute is absent, RGB32's checked tightly packed row size is the
    // documented default; IMF2DBuffer2 supplies the actual pitch at copy time.
    let stride = default_stride
        .map(|value| i32::from_ne_bytes(value.to_ne_bytes()))
        .unwrap_or(row_bytes_i32);
    if stride == 0 || stride.unsigned_abs() < row_bytes {
        return Err(format!(
            "negotiated webcam stride {stride} is too short for {row_bytes}-byte rows"
        ));
    }
    let required = stride
        .unsigned_abs()
        .checked_mul(size.height)
        .ok_or_else(|| "negotiated webcam sample size overflowed".to_owned())?;
    let sample_size = sample_size.unwrap_or(required);
    if sample_size < required {
        return Err(format!(
            "negotiated webcam sample size {sample_size} is smaller than {required} bytes"
        ));
    }
    Ok(NegotiatedFormat { size, stride })
}

fn validate_negotiated_type(media_type: &IMFMediaType) -> Result<NegotiatedFormat, String> {
    // SAFETY: the media type is live and these attributes have scalar values.
    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }
        .map_err(|error| format!("negotiated webcam format has no subtype: {error}"))?;
    if subtype != MFVideoFormat_RGB32 {
        return Err(format!(
            "negotiated webcam subtype {subtype:?} is not convertible RGB32"
        ));
    }
    let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }
        .map_err(|error| format!("negotiated webcam format has no frame size: {error}"))?;
    let size = Size::new((frame_size >> 32) as u32, frame_size as u32);
    if size.width == 0 || size.height == 0 {
        return Err("negotiated webcam format has empty dimensions".into());
    }
    let frame_rate = unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }
        .map_err(|error| format!("negotiated webcam format has no frame rate: {error}"))?;
    if (frame_rate >> 32) == 0 || frame_rate as u32 == 0 {
        return Err("negotiated webcam format has an invalid frame rate".into());
    }
    let interlace = unsafe { media_type.GetUINT32(&MF_MT_INTERLACE_MODE) }
        .map_err(|error| format!("negotiated webcam format has no interlace mode: {error}"))?;
    if interlace != MFVideoInterlace_Progressive.0 as u32 {
        return Err("interlaced webcam formats are unsupported".into());
    }
    let fixed_size = unsafe { media_type.GetUINT32(&MF_MT_FIXED_SIZE_SAMPLES) }
        .map_err(|error| format!("negotiated webcam format has no fixed-size flag: {error}"))?;
    if fixed_size == 0 {
        return Err("negotiated webcam format does not use fixed-size samples".into());
    }
    let default_stride = unsafe { media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.ok();
    let sample_size = unsafe { media_type.GetUINT32(&MF_MT_SAMPLE_SIZE) }.ok();
    resolve_negotiated_layout(size, default_stride, sample_size)
}

struct CaptureState {
    reader: Mutex<Option<IMFSourceReader>>,
    latest: Mutex<Option<Arc<Frame>>>,
    format: Mutex<Size>,
    native_display_aspect_ratio: Mutex<Option<f64>>,
    crop_to_16_9: AtomicBool,
    pacer: Mutex<Option<FramePacer>>,
    stride: AtomicI32,
    scratch: Mutex<Vec<u8>>,
    pool: Mutex<FrameBufferPool>,
    failure: Mutex<Option<WebcamCaptureFailure>>,
    running: AtomicBool,
    consecutive_failures: AtomicU32,
    generation: AtomicU64,
    sequence: AtomicU64,
    dropped_frames: AtomicU64,
    pool_fallbacks: AtomicU64,
    device_tag: AtomicU64,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            reader: Mutex::new(None),
            latest: Mutex::new(None),
            format: Mutex::new(Size::default()),
            native_display_aspect_ratio: Mutex::new(None),
            crop_to_16_9: AtomicBool::new(true),
            pacer: Mutex::new(None),
            stride: AtomicI32::new(0),
            scratch: Mutex::new(Vec::new()),
            pool: Mutex::new(FrameBufferPool::new(
                (PIPELINE_SIZE.width * PIPELINE_SIZE.height * 4) as usize,
                CAPTURE_FRAME_POOL_CAPACITY,
            )),
            failure: Mutex::new(None),
            running: AtomicBool::new(false),
            consecutive_failures: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            pool_fallbacks: AtomicU64::new(0),
            device_tag: AtomicU64::new(0),
        }
    }
}

// SAFETY: IMFSourceReader documents asynchronous callbacks from Media
// Foundation worker threads; this state exposes it only through a mutex.
unsafe impl Send for CaptureState {}
// SAFETY: every mutable field is atomic or mutex-protected.
unsafe impl Sync for CaptureState {}

impl CaptureState {
    /// A replacement owns independent callback state; late callbacks can only
    /// modify the retired session. Preserve user settings and diagnostic counters.
    fn next_capture(&self) -> Self {
        Self {
            crop_to_16_9: AtomicBool::new(self.crop_to_16_9.load(Ordering::Acquire)),
            generation: AtomicU64::new(self.generation.load(Ordering::Acquire)),
            sequence: AtomicU64::new(self.sequence.load(Ordering::Acquire)),
            dropped_frames: AtomicU64::new(self.dropped_frames.load(Ordering::Acquire)),
            pool_fallbacks: AtomicU64::new(self.pool_fallbacks.load(Ordering::Acquire)),
            ..Self::default()
        }
    }

    fn is_current(&self, expected_generation: u64) -> bool {
        self.running.load(Ordering::Acquire)
            && self.generation.load(Ordering::Acquire) == expected_generation
    }

    fn request_next(&self, expected_generation: u64) -> Result<(), WebcamError> {
        if !self.is_current(expected_generation) {
            return Ok(());
        }
        let reader = self.reader.lock().ok().and_then(|reader| reader.clone());
        if let Some(reader) = reader {
            // SAFETY: asynchronous mode requires all output pointers to be null.
            if let Err(error) = unsafe { reader.ReadSample(STREAM, 0, None, None, None, None) } {
                let device_tag = self.device_tag.load(Ordering::Acquire);
                let capture_generation = self.generation.load(Ordering::Acquire);
                let failure = WebcamCaptureFailure {
                    kind: webcam_hresult_kind(error.code()),
                    message: webcam_error_message(
                        "webcam sample request failed",
                        error.code(),
                        device_tag,
                        capture_generation,
                    ),
                    hresult: Some(error.code().0),
                    device_tag,
                    capture_generation,
                };
                let error = WebcamError::new(failure.kind, failure.message.clone());
                self.set_failure(failure, true);
                return Err(error);
            }
        }
        Ok(())
    }

    fn admit_frame(&self, now: Instant) -> bool {
        let Ok(mut pacer) = self.pacer.lock() else {
            return false;
        };
        let pacer = pacer.get_or_insert_with(|| FramePacer::new(now, WEBCAM_FRAME_INTERVAL));
        if !pacer.is_due(now, WEBCAM_FRAME_EARLY_TOLERANCE) {
            return false;
        }
        pacer.advance(now);
        true
    }

    fn set_failure(&self, details: WebcamCaptureFailure, terminal: bool) {
        if let Ok(mut failure) = self.failure.lock() {
            *failure = Some(details);
        }
        let failures = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
        if terminal || failures >= WEBCAM_FAILURE_THRESHOLD {
            self.running.store(false, Ordering::Release);
            if let Ok(mut latest) = self.latest.lock() {
                *latest = None;
            }
        }
    }

    fn clear_failure(&self) {
        self.consecutive_failures.store(0, Ordering::Release);
        if let Ok(mut failure) = self.failure.lock() {
            *failure = None;
        }
    }

    /// Normalizes locked row-aware input, copying native 720p directly into the pool.
    fn normalize_sample(
        &self,
        source: &[u8],
        offset: usize,
        stride: i32,
        size: Size,
    ) -> Result<Arc<[u8]>, String> {
        let aspect_ratio = *self
            .native_display_aspect_ratio
            .lock()
            .map_err(|_| "webcam aspect-ratio state is poisoned")?;
        let fill = should_aspect_fill(self.crop_to_16_9.load(Ordering::Acquire), aspect_ratio);
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| "webcam frame pool is poisoned")?;
        let previous_exhaustions = pool.exhaustion_count();
        let pixels = pool.write_with_fallback(|destination| {
            if size == PIPELINE_SIZE && !fill {
                return copy_bgra_rows(source, offset, stride, size, destination);
            }
            let mut tight = self
                .scratch
                .lock()
                .map_err(|_| "webcam row-copy state is poisoned")?;
            let length = (size.width as usize)
                .checked_mul(size.height as usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or("webcam frame size overflowed")?;
            tight.resize(length, 0);
            copy_bgra_rows(source, offset, stride, size, &mut tight)?;
            let result = if fill {
                aspect_fill_bgra_into(
                    &tight,
                    size,
                    size.width * 4,
                    aspect_ratio.expect("aspect fill requires a known ratio"),
                    destination,
                    PIPELINE_SIZE,
                )
            } else {
                aspect_fit_bgra_into(&tight, size, size.width * 4, destination, PIPELINE_SIZE)
            };
            result.map_err(|error| format!("could not normalize webcam frame: {error:?}"))
        })?;
        if pool.exhaustion_count() > previous_exhaustions {
            self.pool_fallbacks.fetch_add(1, Ordering::Relaxed);
        }
        Ok(pixels)
    }

    fn update_current_format(&self) -> Result<NegotiatedFormat, String> {
        let reader = self
            .reader
            .lock()
            .map_err(|_| "webcam reader state is poisoned")?
            .clone()
            .ok_or_else(|| "webcam reader is unavailable".to_owned())?;
        // SAFETY: the reader is live and returns a retained media type.
        let current = unsafe { reader.GetCurrentMediaType(STREAM) }
            .map_err(|error| format!("could not read changed webcam media type: {error}"))?;
        let format = validate_negotiated_type(&current)?;
        *self
            .format
            .lock()
            .map_err(|_| "webcam format state is poisoned")? = format.size;
        *self
            .native_display_aspect_ratio
            .lock()
            .map_err(|_| "webcam aspect-ratio state is poisoned")? =
            current_native_display_aspect_ratio(&reader);
        self.stride.store(format.stride, Ordering::Release);
        Ok(format)
    }
}

/// Maps native status codes before formatting diagnostics or making recovery decisions.
fn webcam_hresult_kind(code: HRESULT) -> WebcamFailureKind {
    if code == E_ACCESSDENIED {
        WebcamFailureKind::PrivacyDisabled
    } else if code == MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED {
        WebcamFailureKind::DeviceBusy
    } else if code == MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED {
        WebcamFailureKind::DeviceInvalidated
    } else {
        WebcamFailureKind::DriverFailure
    }
}

fn webcam_failure_without_hresult(
    state: &CaptureState,
    message: impl Into<String>,
    kind: WebcamFailureKind,
) -> WebcamCaptureFailure {
    let device_tag = state.device_tag.load(Ordering::Acquire);
    let capture_generation = state.generation.load(Ordering::Acquire);
    let identity = if device_tag == 0 {
        format!("device=unknown, capture_generation={capture_generation}")
    } else {
        format!("device_tag={device_tag:016x}, capture_generation={capture_generation}")
    };
    WebcamCaptureFailure {
        kind,
        message: format!("{} (HRESULT=unavailable, {identity})", message.into()),
        hresult: None,
        device_tag,
        capture_generation,
    }
}

#[implement(IMFSourceReaderCallback)]
struct ReaderCallback {
    state: Arc<CaptureState>,
    expected_generation: u64,
}

impl IMFSourceReaderCallback_Impl for ReaderCallback_Impl {
    fn OnReadSample(
        &self,
        status: HRESULT,
        _stream: u32,
        flags: u32,
        timestamp: i64,
        sample: Ref<IMFSample>,
    ) -> windows_core::Result<()> {
        if !self.state.is_current(self.expected_generation) {
            return Ok(());
        }
        if status.is_err() {
            let device_tag = self.state.device_tag.load(Ordering::Acquire);
            let capture_generation = self.state.generation.load(Ordering::Acquire);
            self.state.set_failure(
                WebcamCaptureFailure {
                    kind: webcam_hresult_kind(status),
                    message: webcam_error_message(
                        "webcam sample failed",
                        status,
                        device_tag,
                        capture_generation,
                    ),
                    hresult: Some(status.0),
                    device_tag,
                    capture_generation,
                },
                true,
            );
            return Ok(());
        } else if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            self.state.set_failure(
                webcam_failure_without_hresult(
                    &self.state,
                    "webcam source reader reported a capture error",
                    WebcamFailureKind::DriverFailure,
                ),
                true,
            );
            return Ok(());
        }
        if flags
            & (MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32
                | MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED.0 as u32)
            != 0
            && let Err(error) = self.state.update_current_format()
        {
            self.state.set_failure(
                webcam_failure_without_hresult(
                    &self.state,
                    format!("webcam media-type change is incompatible: {error}"),
                    WebcamFailureKind::IncompatibleTypeChange,
                ),
                true,
            );
            return Ok(());
        }
        let now = Instant::now();
        if !self.state.admit_frame(now) {
            let _ = self.state.request_next(self.expected_generation);
            return Ok(());
        }
        if let Some(sample) = sample.as_ref() {
            // SAFETY: the sample and selected buffer remain live for this callback. Taking the
            // original first buffer preserves IMF2DBuffer2 row metadata when the driver exposes it.
            if let Ok(buffer) = unsafe { sample.GetBufferByIndex(0) }
                .or_else(|_| unsafe { sample.ConvertToContiguousBuffer() })
            {
                let size = self
                    .state
                    .format
                    .lock()
                    .map_or(PIPELINE_SIZE, |format| *format);
                let stride = self.state.stride.load(Ordering::Acquire);
                let copy_result = (|| {
                    let pixels = if let Ok(buffer_2d) = buffer.cast::<IMF2DBuffer2>() {
                        let mut scanline = core::ptr::null_mut();
                        let mut pitch = 0;
                        let mut buffer_start = core::ptr::null_mut();
                        let mut buffer_length = 0;
                        // SAFETY: all output pointers are writable and Unlock2D balances a
                        // successful lock before the callback returns.
                        unsafe {
                            buffer_2d.Lock2DSize(
                                MF2DBuffer_LockFlags_Read,
                                &mut scanline,
                                &mut pitch,
                                &mut buffer_start,
                                &mut buffer_length,
                            )
                        }
                        .map_err(|error| format!("could not lock webcam 2D buffer: {error}"))?;
                        // SAFETY: Lock2DSize returns pointers into the same locked allocation.
                        let offset = unsafe { scanline.offset_from(buffer_start) };
                        let result = if offset < 0 {
                            Err("webcam scanline starts before its 2D buffer".into())
                        } else {
                            // SAFETY: buffer_start addresses buffer_length locked bytes.
                            let source = unsafe {
                                core::slice::from_raw_parts(buffer_start, buffer_length as usize)
                            };
                            self.state
                                .normalize_sample(source, offset as usize, pitch, size)
                        };
                        // SAFETY: balances the successful Lock2DSize call above.
                        let _ = unsafe { buffer_2d.Unlock2D() };
                        result?
                    } else {
                        if stride < 0 {
                            return Err(
                                "negative webcam stride requires IMF2DBuffer2 access".into()
                            );
                        }
                        let mut bytes = core::ptr::null_mut();
                        let mut length = 0;
                        // SAFETY: all output pointers are writable; Unlock balances Lock.
                        unsafe { buffer.Lock(&mut bytes, None, Some(&mut length)) }
                            .map_err(|error| format!("could not lock webcam buffer: {error}"))?;
                        // SAFETY: Lock returned length readable bytes at bytes.
                        let source = unsafe { core::slice::from_raw_parts(bytes, length as usize) };
                        let result = self.state.normalize_sample(source, 0, stride, size);
                        // SAFETY: balances the successful Lock call above.
                        let _ = unsafe { buffer.Unlock() };
                        result?
                    };
                    let sequence = self.state.sequence.fetch_add(1, Ordering::Relaxed) + 1;
                    let frame = Frame::new(
                        pixels,
                        PIPELINE_SIZE,
                        PIPELINE_SIZE.width * 4,
                        sequence,
                        timestamp,
                        now,
                    )
                    .map_err(|error| format!("invalid webcam frame: {error:?}"))?;
                    if self.state.is_current(self.expected_generation) {
                        *self
                            .state
                            .latest
                            .lock()
                            .map_err(|_| "webcam frame state is poisoned".to_owned())? =
                            Some(Arc::new(frame));
                        self.state.clear_failure();
                    }
                    Ok::<(), String>(())
                })();
                if let Err(error) = copy_result {
                    self.state.dropped_frames.fetch_add(1, Ordering::Relaxed);
                    self.state.set_failure(
                        webcam_failure_without_hresult(
                            &self.state,
                            error,
                            WebcamFailureKind::DriverFailure,
                        ),
                        false,
                    );
                }
            } else {
                self.state.dropped_frames.fetch_add(1, Ordering::Relaxed);
                self.state.set_failure(
                    webcam_failure_without_hresult(
                        &self.state,
                        "could not obtain a contiguous webcam buffer",
                        WebcamFailureKind::DriverFailure,
                    ),
                    false,
                );
            }
        }
        let _ = self.state.request_next(self.expected_generation);
        Ok(())
    }

    fn OnFlush(&self, _stream: u32) -> windows_core::Result<()> {
        Ok(())
    }

    fn OnEvent(&self, _stream: u32, _event: Ref<IMFMediaEvent>) -> windows_core::Result<()> {
        Ok(())
    }
}

pub struct MediaFoundationVideoInput {
    state: Arc<CaptureState>,
    callback: Option<IMFSourceReaderCallback>,
    selected_native_format: Option<String>,
    selected_output_format: Option<String>,
    initialization_error: Option<String>,
    mf_started: bool,
    com_initialized: bool,
}

// SAFETY: the source reader runs in asynchronous mode. Interface access and
// frame state that cross callback threads are guarded by mutexes.
unsafe impl Send for MediaFoundationVideoInput {}

impl Default for MediaFoundationVideoInput {
    fn default() -> Self {
        // SAFETY: this adapter is created, used, and dropped on its owning runtime thread.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok();
        let com_initialized = com.is_ok();
        let (mf_started, initialization_error) = match com {
            Ok(()) => {
                // SAFETY: balanced by MFShutdown in Drop.
                match unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
                    Ok(()) => (true, None),
                    Err(error) => (
                        false,
                        Some(format!("could not initialize Media Foundation: {error}")),
                    ),
                }
            }
            Err(error) => (false, Some(format!("could not initialize COM: {error}"))),
        };
        Self {
            state: Arc::new(CaptureState::default()),
            callback: None,
            selected_native_format: None,
            selected_output_format: None,
            initialization_error,
            mf_started,
            com_initialized,
        }
    }
}

impl VideoInput for MediaFoundationVideoInput {
    fn enumerate(&self) -> Result<Vec<InputDevice>, String> {
        if let Some(error) = &self.initialization_error {
            return Err(error.clone());
        }
        enumerate_video_devices()
    }

    fn start(&mut self, device_id: &str) -> Result<(), WebcamError> {
        self.selected_native_format = None;
        self.selected_output_format = None;
        if let Some(error) = &self.initialization_error {
            return Err(error.clone().into());
        }
        if device_id.is_empty() {
            return Err("video source identifier is empty".into());
        }
        self.stop();
        self.state = Arc::new(self.state.next_capture());
        let mut source_attributes = None;
        let mut reader_attributes = None;
        (|| -> windows_core::Result<()> {
            // SAFETY: all outputs are writable and the runtime initialized MF.
            unsafe {
                MFCreateAttributes(&mut source_attributes, 2)?;
                MFCreateAttributes(&mut reader_attributes, 2)?;
            }
            Ok(())
        })()
        .map_err(|error| {
            WebcamError::new(
                webcam_hresult_kind(error.code()),
                format!("could not create video capture attributes: {error}"),
            )
        })?;
        let source_attributes = source_attributes.expect("attributes were created");
        let reader_attributes = reader_attributes.expect("attributes were created");
        let id: Vec<u16> = device_id.encode_utf16().chain([0]).collect();
        (|| -> windows_core::Result<()> {
            // SAFETY: all attributes and the terminated identifier are valid.
            unsafe {
                source_attributes.SetGUID(
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                )?;
                source_attributes.SetString(
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                    PCWSTR(id.as_ptr()),
                )?;
            }
            Ok(())
        })()
        .map_err(|error| {
            WebcamError::new(
                webcam_hresult_kind(error.code()),
                format!("could not configure video source: {error}"),
            )
        })?;
        let device_tag = device_identity_tag(device_id);
        let capture_generation = self.state.generation.load(Ordering::Acquire);
        let source = unsafe { MFCreateDeviceSource(&source_attributes) }.map_err(|error| {
            WebcamError::new(
                webcam_hresult_kind(error.code()),
                webcam_error_message(
                    "could not open video source",
                    error.code(),
                    device_tag,
                    capture_generation,
                ),
            )
        })?;
        let expected_generation = self.state.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let callback: IMFSourceReaderCallback = ReaderCallback {
            state: Arc::clone(&self.state),
            expected_generation,
        }
        .into();
        (|| -> windows_core::Result<()> {
            // SAFETY: reader attributes and callback remain live.
            unsafe {
                reader_attributes.SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, &callback)?;
                reader_attributes
                    .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
            }
            Ok(())
        })()
        .map_err(|error| {
            WebcamError::new(
                webcam_hresult_kind(error.code()),
                format!("could not configure video reader: {error}"),
            )
        })?;
        let reader = unsafe { MFCreateSourceReaderFromMediaSource(&source, &reader_attributes) }
            .map_err(|error| {
                WebcamError::new(
                    webcam_hresult_kind(error.code()),
                    format!("could not create video reader: {error}"),
                )
            })?;
        let (format, native_description, output_description) = negotiate_rgb32_output(&reader)?;
        self.selected_native_format = Some(native_description);
        self.selected_output_format = Some(output_description);
        *self
            .state
            .native_display_aspect_ratio
            .lock()
            .map_err(|_| "webcam aspect-ratio state is poisoned")? =
            current_native_display_aspect_ratio(&reader);
        *self
            .state
            .format
            .lock()
            .map_err(|_| "webcam format state is poisoned")? = format.size;
        self.state.stride.store(format.stride, Ordering::Release);
        *self
            .state
            .pacer
            .lock()
            .map_err(|_| "webcam pacing state is poisoned")? = None;
        self.state.device_tag.store(device_tag, Ordering::Release);
        self.state.clear_failure();
        *self
            .state
            .reader
            .lock()
            .map_err(|_| "video reader state is poisoned")? = Some(reader);
        self.state.running.store(true, Ordering::Release);
        self.callback = Some(callback);
        self.state.request_next(expected_generation)?;
        Ok(())
    }

    fn stop(&mut self) {
        self.state.generation.fetch_add(1, Ordering::AcqRel);
        self.state.running.store(false, Ordering::Release);
        if let Ok(mut reader) = self.state.reader.lock()
            && let Some(reader) = reader.take()
        {
            // SAFETY: this reader was created by this adapter.
            let _ = unsafe { reader.Flush(STREAM) };
        }
        self.callback = None;
        if let Ok(mut aspect_ratio) = self.state.native_display_aspect_ratio.lock() {
            *aspect_ratio = None;
        }
        self.state.stride.store(0, Ordering::Release);
        self.state.device_tag.store(0, Ordering::Release);
        if let Ok(mut format) = self.state.format.lock() {
            *format = Size::default();
        }
        if let Ok(mut pacer) = self.state.pacer.lock() {
            *pacer = None;
        }
        if let Ok(mut latest) = self.state.latest.lock() {
            *latest = None;
        }
        self.state.clear_failure();
    }

    fn latest_frame(&self) -> Option<Arc<Frame>> {
        if !self.state.running.load(Ordering::Acquire) {
            return None;
        }
        self.state
            .latest
            .lock()
            .ok()
            .and_then(|frame| frame.clone())
    }
}

impl MediaFoundationVideoInput {
    pub fn set_crop_to_16_9(&self, enabled: bool) {
        self.state.crop_to_16_9.store(enabled, Ordering::Release);
    }

    pub fn native_display_aspect_ratio(&self) -> Option<f64> {
        self.state
            .native_display_aspect_ratio
            .lock()
            .ok()
            .and_then(|ratio| *ratio)
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_failure().map(|failure| failure.message)
    }

    pub fn last_failure(&self) -> Option<WebcamCaptureFailure> {
        self.state
            .failure
            .lock()
            .ok()
            .and_then(|failure| failure.clone())
    }

    pub fn capture_generation(&self) -> u64 {
        self.state.generation.load(Ordering::Acquire)
    }

    pub fn selected_native_format(&self) -> Option<&str> {
        self.selected_native_format.as_deref()
    }

    pub fn selected_output_format(&self) -> Option<&str> {
        self.selected_output_format.as_deref()
    }

    pub fn dropped_frame_count(&self) -> u64 {
        self.state.dropped_frames.load(Ordering::Relaxed)
    }

    pub fn pool_fallback_count(&self) -> u64 {
        self.state.pool_fallbacks.load(Ordering::Relaxed)
    }
}

impl Drop for MediaFoundationVideoInput {
    fn drop(&mut self) {
        self.stop();
        if self.mf_started {
            // SAFETY: balances this adapter's successful MFStartup.
            let _ = unsafe { MFShutdown() };
        }
        if self.com_initialized {
            // SAFETY: balances this adapter's successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn native_missing_stride_and_sample_size_use_checked_rgb32_defaults() {
        let format = resolve_negotiated_layout(Size::new(1280, 720), None, None).unwrap();
        assert_eq!(format.stride, 1280 * 4);
    }

    #[test]
    fn native_negative_stride_is_validated_by_absolute_row_pitch() {
        let format = resolve_negotiated_layout(
            Size::new(2, 2),
            Some(u32::from_ne_bytes((-12i32).to_ne_bytes())),
            None,
        )
        .unwrap();
        assert_eq!(format.stride, -12);
    }

    #[test]
    fn native_undersized_declared_sample_is_rejected() {
        let error = resolve_negotiated_layout(Size::new(2, 2), Some(12), Some(23)).unwrap_err();
        assert!(error.contains("smaller than 24"));
    }

    #[test]
    fn native_row_copy_accepts_padding_and_negative_stride() {
        let size = Size::new(2, 2);
        let top = [1, 2, 3, 4, 5, 6, 7, 8];
        let bottom = [11, 12, 13, 14, 15, 16, 17, 18];
        let mut padded = Vec::new();
        padded.extend_from_slice(&top);
        padded.extend_from_slice(&[0; 4]);
        padded.extend_from_slice(&bottom);
        padded.extend_from_slice(&[0; 4]);
        let mut destination = vec![0; 16];
        copy_bgra_rows(&padded, 0, 12, size, &mut destination).unwrap();
        assert_eq!(destination, [top, bottom].concat());

        let mut bottom_up = Vec::new();
        bottom_up.extend_from_slice(&bottom);
        bottom_up.extend_from_slice(&[0; 4]);
        bottom_up.extend_from_slice(&top);
        bottom_up.extend_from_slice(&[0; 4]);
        copy_bgra_rows(&bottom_up, 12, -12, size, &mut destination).unwrap();
        assert_eq!(destination, [top, bottom].concat());
    }

    #[test]
    fn native_row_copy_rejects_an_undersized_buffer() {
        let size = Size::new(2, 2);
        let mut destination = vec![0; 16];
        let error = copy_bgra_rows(&[0; 20], 0, 12, size, &mut destination).unwrap_err();
        assert!(error.contains("too short"));
    }

    #[test]
    fn native_repeated_callback_failures_open_the_circuit() {
        let state = CaptureState::default();
        state.running.store(true, Ordering::Release);
        for _ in 0..WEBCAM_FAILURE_THRESHOLD {
            state.set_failure(
                webcam_failure_without_hresult(
                    &state,
                    "scripted callback failure",
                    WebcamFailureKind::DriverFailure,
                ),
                false,
            );
        }
        assert!(!state.running.load(Ordering::Acquire));
        assert!(state.latest.lock().unwrap().is_none());
    }

    #[test]
    fn contract_rational_frame_rate_ranking_prefers_2997_over_29() {
        let mut formats = [
            WebcamFormatCandidate {
                size: Size::new(1920, 1080),
                frame_rate_numerator: 29,
                frame_rate_denominator: 1,
                subtype: "RGB32",
                subtype_rank: 0,
            },
            WebcamFormatCandidate {
                size: Size::new(1920, 1080),
                frame_rate_numerator: 30_000,
                frame_rate_denominator: 1_001,
                subtype: "RGB32",
                subtype_rank: 0,
            },
        ];
        sort_webcam_formats(&mut formats);
        assert_eq!(formats[0].frame_rate_numerator, 30_000);
        assert_eq!(formats[0].frame_rate_denominator, 1_001);
    }

    #[test]
    fn contract_webcam_pacing_admits_common_rates_at_the_expected_ceiling() {
        let expected = [
            ((30_u64, 1_u64), 300),
            ((60, 1), 150),
            ((60_000, 1_001), 150),
            ((120, 1), 75),
        ];
        for ((numerator, denominator), expected_count) in expected {
            let state = CaptureState::default();
            let start = Instant::now();
            let input_interval = Duration::from_nanos(1_000_000_000_u64 * denominator / numerator);
            let admitted = (0..300)
                .filter(|index| state.admit_frame(start + input_interval * *index))
                .count();
            assert_eq!(admitted, expected_count, "rate {numerator}/{denominator}");
        }
    }

    #[test]
    fn contract_intentional_webcam_decimation_does_not_change_failure_counters() {
        let state = CaptureState::default();
        state.sequence.store(9, Ordering::Release);
        state.dropped_frames.store(4, Ordering::Release);
        state.consecutive_failures.store(2, Ordering::Release);
        let start = Instant::now();
        let input_interval = Duration::from_nanos(1_000_000_000 / 120);
        assert!((0..120).any(|index| !state.admit_frame(start + input_interval * index)));
        assert_eq!(state.sequence.load(Ordering::Acquire), 9);
        assert_eq!(state.dropped_frames.load(Ordering::Acquire), 4);
        assert_eq!(state.consecutive_failures.load(Ordering::Acquire), 2);
    }

    #[test]
    fn contract_webcam_crop_decision_handles_toggle_tolerance_and_unknown_aspect() {
        assert!(should_aspect_fill(true, Some(4.0 / 3.0)));
        assert!(should_aspect_fill(true, Some(21.0 / 9.0)));
        assert!(!should_aspect_fill(false, Some(4.0 / 3.0)));
        assert!(!should_aspect_fill(true, None));
        assert!(!should_aspect_fill(true, Some(16.0 / 9.0)));
        assert!(!should_aspect_fill(
            true,
            Some(TARGET_WEBCAM_ASPECT_RATIO * 1.005)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_capture_failure_categories_do_not_depend_on_diagnostic_words() {
        for (code, expected) in [
            (E_ACCESSDENIED, WebcamFailureKind::PrivacyDisabled),
            (
                MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED,
                WebcamFailureKind::DeviceBusy,
            ),
            (
                MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED,
                WebcamFailureKind::DeviceInvalidated,
            ),
            (
                HRESULT(0x80004005_u32 as i32),
                WebcamFailureKind::DriverFailure,
            ),
        ] {
            assert_eq!(webcam_hresult_kind(code), expected);
        }
        let state = CaptureState::default();
        let failure = webcam_failure_without_hresult(
            &state,
            "privacy format unavailable",
            WebcamFailureKind::DriverFailure,
        );
        assert_eq!(failure.kind, WebcamFailureKind::DriverFailure);
        let failure = webcam_failure_without_hresult(
            &state,
            "diagnostic wording changed",
            WebcamFailureKind::IncompatibleTypeChange,
        );
        assert_eq!(failure.kind, WebcamFailureKind::IncompatibleTypeChange);
    }

    #[test]
    fn native_capture_generation_rejects_callbacks_from_stopped_reader() {
        let state = CaptureState::default();
        let generation = state.generation.fetch_add(1, Ordering::AcqRel) + 1;
        state.running.store(true, Ordering::Release);
        assert!(state.is_current(generation));
        state.generation.fetch_add(1, Ordering::AcqRel);
        assert!(!state.is_current(generation));
    }

    #[test]
    fn native_retired_callback_cannot_fail_a_replacement_capture() {
        let old = CaptureState::default();
        old.generation.store(1, Ordering::Release);
        old.running.store(true, Ordering::Release);
        assert!(old.is_current(1)); // Callback admitted before a restart.
        old.running.store(false, Ordering::Release);
        old.generation.fetch_add(1, Ordering::AcqRel);
        let replacement = old.next_capture();
        replacement.running.store(true, Ordering::Release);
        let frame = Arc::new(Frame::placeholder(
            PIPELINE_SIZE,
            0xff00_ff00,
            2,
            0,
            Instant::now(),
        ));
        *replacement.latest.lock().unwrap() = Some(Arc::clone(&frame));
        old.set_failure(
            webcam_failure_without_hresult(
                &old,
                "late retired callback",
                WebcamFailureKind::DriverFailure,
            ),
            true,
        );
        assert!(replacement.running.load(Ordering::Acquire));
        assert!(replacement.failure.lock().unwrap().is_none());
        assert!(Arc::ptr_eq(
            replacement.latest.lock().unwrap().as_ref().unwrap(),
            &frame
        ));
    }

    #[test]
    fn native_720p_normalization_copies_rows_directly_and_reuses_pool_slots() {
        let state = CaptureState::default();
        let row_bytes = PIPELINE_SIZE.width as usize * 4;
        let pitch = row_bytes + 16;
        let mut source = vec![0; pitch * PIPELINE_SIZE.height as usize];
        for row in 0..PIPELINE_SIZE.height as usize {
            source[row * pitch..row * pitch + row_bytes].fill((row % 251) as u8);
        }
        let first = state
            .normalize_sample(&source, 0, pitch as i32, PIPELINE_SIZE)
            .unwrap();
        let pointer = first.as_ptr();
        assert!(state.scratch.lock().unwrap().is_empty());
        assert!(
            first[row_bytes..row_bytes * 2]
                .iter()
                .all(|byte| *byte == 1)
        );
        let second = state
            .normalize_sample(
                &source,
                pitch * (PIPELINE_SIZE.height as usize - 1),
                -(pitch as i32),
                PIPELINE_SIZE,
            )
            .unwrap();
        assert!(
            second[..row_bytes]
                .iter()
                .all(|byte| *byte == ((PIPELINE_SIZE.height - 1) % 251) as u8)
        );
        drop(first);
        let third = state
            .normalize_sample(&source, 0, pitch as i32, PIPELINE_SIZE)
            .unwrap();
        assert_eq!(third.as_ptr(), pointer);
        assert!(state.scratch.lock().unwrap().is_empty());
        assert!(
            state
                .normalize_sample(&source[..10], 0, pitch as i32, PIPELINE_SIZE)
                .is_err()
        );
    }

    #[test]
    fn native_product_virtual_cameras_are_never_selectable_webcams() {
        assert!(is_managed_virtual_camera("StageSwap"));
        assert!(is_managed_virtual_camera("Automatic Screen Camera"));
        assert!(is_managed_virtual_camera("stageswap"));
        assert!(is_managed_virtual_camera("StageSwap Virtual Camera"));
        assert!(is_managed_virtual_camera("Stageswap Camera"));
        assert!(!is_managed_virtual_camera("USB Webcam"));
    }
}
