//! Reference decoding, persistence, cancellation, and completion delivery.

use super::*;

pub(super) struct ReferenceData {
    pub(super) detector: GrayImage,
    pub(super) preview: Arc<Frame>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReferenceAction {
    Load,
    LegacyCapture { frame_sequence: u64 },
    ConfirmedCapture { frame_sequence: u64 },
    Import,
}

pub(super) enum ReferenceJobKind {
    Load {
        path: std::path::PathBuf,
    },
    Persist {
        frame: Arc<Frame>,
        path: std::path::PathBuf,
        confirmed: bool,
    },
    Import {
        source: std::path::PathBuf,
        destination: std::path::PathBuf,
    },
}

/// Cancellation wins until the worker claims the final atomic replacement.
/// Once claimed, completion must be reconciled with runtime state even if the UI moves on.
#[derive(Default)]
pub(super) struct ReferenceCommit(std::sync::atomic::AtomicU8);

impl ReferenceCommit {
    const PENDING: u8 = 0;
    const CANCELLED: u8 = 1;
    const COMMITTING: u8 = 2;

    pub(super) fn cancel(&self) {
        let _ = self.0.compare_exchange(
            Self::PENDING,
            Self::CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire) == Self::CANCELLED
    }

    pub(super) fn claim(&self) -> Result<(), String> {
        match self.0.compare_exchange(
            Self::PENDING,
            Self::COMMITTING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) | Err(Self::COMMITTING) => Ok(()),
            Err(_) => Err("Reference save was cancelled before replacement".into()),
        }
    }
}

pub(super) struct ReferenceJob {
    pub(super) generation: u64,
    pub(super) commit: Option<Arc<ReferenceCommit>>,
    pub(super) kind: ReferenceJobKind,
}

pub(super) struct ReferenceResult {
    pub(super) generation: u64,
    pub(super) persisted_path: Option<std::path::PathBuf>,
    pub(super) action: ReferenceAction,
    pub(super) result: Result<ReferenceData, String>,
}

pub(super) struct ReferenceWorker {
    pub(super) jobs: Option<SyncSender<ReferenceJob>>,
    pub(super) results: Receiver<ReferenceResult>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) done: Receiver<()>,
    pub(super) worker: Option<JoinHandle<()>>,
}

impl ReferenceWorker {
    pub(super) fn start() -> Result<Self, String> {
        let (job_sender, job_receiver) = mpsc::sync_channel::<ReferenceJob>(REFERENCE_JOB_CAPACITY);
        let (result_sender, result_receiver) =
            mpsc::sync_channel::<ReferenceResult>(REFERENCE_JOB_CAPACITY);
        let (done_sender, done_receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("stageswap-reference-io".into())
            .spawn(move || {
                while let Ok(job) = job_receiver.recv() {
                    if worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    let mut result = execute_reference_job(job);
                    loop {
                        if worker_stop.load(Ordering::Acquire) {
                            break;
                        }
                        match result_sender.try_send(result) {
                            Ok(()) => break,
                            Err(mpsc::TrySendError::Disconnected(_)) => return,
                            Err(mpsc::TrySendError::Full(pending)) => {
                                result = pending;
                                // Retain the result under backpressure without
                                // blocking shutdown or growing the bounded queue.
                                thread::park_timeout(Duration::from_millis(10));
                            }
                        }
                    }
                }
                let _ = done_sender.try_send(());
            })
            .map_err(|error| format!("could not start reference worker: {error}"))?;
        Ok(Self {
            jobs: Some(job_sender),
            results: result_receiver,
            stop,
            done: done_receiver,
            worker: Some(worker),
        })
    }

    pub(super) fn submit(&self, job: ReferenceJob) -> Result<(), String> {
        self.jobs
            .as_ref()
            .ok_or_else(|| "reference worker is stopped".to_owned())?
            .try_send(job)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "reference worker is busy".to_owned(),
                mpsc::TrySendError::Disconnected(_) => "reference worker is unavailable".to_owned(),
            })
    }

    pub(super) fn poll(&self) -> Option<ReferenceResult> {
        self.results.try_recv().ok()
    }

    pub(super) fn signal_shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.jobs.take();
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }

    pub(super) fn finish_shutdown(&mut self, timeout: Duration) -> bool {
        finish_worker_shutdown(&self.done, &mut self.worker, timeout)
    }
}

impl Drop for ReferenceWorker {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.signal_shutdown();
            let _ = self.finish_shutdown(WORKER_SHUTDOWN_TIMEOUT);
        }
    }
}

pub(super) fn execute_reference_job(job: ReferenceJob) -> ReferenceResult {
    let persisted_path = match &job.kind {
        ReferenceJobKind::Load { .. } => None,
        ReferenceJobKind::Persist { path, .. } => Some(path.clone()),
        ReferenceJobKind::Import { destination, .. } => Some(destination.clone()),
    };
    let (action, result) = match job.kind {
        ReferenceJobKind::Load { path } => {
            cleanup_pending_reference(&path);
            (ReferenceAction::Load, decode_reference(&path))
        }
        ReferenceJobKind::Persist {
            frame,
            path,
            confirmed,
        } => {
            let action = if confirmed {
                ReferenceAction::ConfirmedCapture {
                    frame_sequence: frame.sequence,
                }
            } else {
                ReferenceAction::LegacyCapture {
                    frame_sequence: frame.sequence,
                }
            };
            let result = reference_data_from_frame(&frame).and_then(|data| {
                persist_frame_guarded(&frame, &path, job.commit.as_deref()).map(|()| data)
            });
            (action, result)
        }
        ReferenceJobKind::Import {
            source,
            destination,
        } => {
            let result = decode_rgba_limited(&source).and_then(|rgba| {
                let data = reference_data_from_rgba(&rgba)?;
                persist_rgba_guarded(&rgba, &destination, job.commit.as_deref())?;
                Ok(data)
            });
            (ReferenceAction::Import, result)
        }
    };
    ReferenceResult {
        generation: job.generation,
        persisted_path,
        action,
        result,
    }
}

pub(super) fn decode_rgba_limited(path: &std::path::Path) -> Result<image::RgbaImage, String> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|error| format!("could not open reference image: {error}"))?
        .with_guessed_format()
        .map_err(|error| format!("could not identify reference image: {error}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(REFERENCE_MAX_DIMENSION);
    limits.max_image_height = Some(REFERENCE_MAX_DIMENSION);
    limits.max_alloc = Some(REFERENCE_MAX_ALLOCATION);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|error| format!("could not decode reference image: {error}"))
        .map(|image| image.to_rgba8())
}

pub(super) fn decode_reference(path: &std::path::Path) -> Result<ReferenceData, String> {
    if path.as_os_str().is_empty() {
        return Err("reference image path is empty".into());
    }
    let rgba = decode_rgba_limited(path)?;
    reference_data_from_rgba(&rgba)
}

pub(super) fn reference_data_from_frame(frame: &Frame) -> Result<ReferenceData, String> {
    let detector = gray_thumbnail(frame)
        .ok_or_else(|| "reference candidate is not a valid screen frame".to_owned())?;
    let preview = if frame.size.width > REFERENCE_PREVIEW_SIZE.width
        || frame.size.height > REFERENCE_PREVIEW_SIZE.height
    {
        let mut rgba = frame.pixels().to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let rgba = image::RgbaImage::from_raw(frame.size.width, frame.size.height, rgba)
            .ok_or_else(|| "reference frame layout is invalid".to_owned())?;
        reference_data_from_rgba(&rgba)?.preview
    } else {
        Arc::new(frame.clone())
    };
    Ok(ReferenceData { detector, preview })
}

pub(super) fn reference_data_from_rgba(rgba: &image::RgbaImage) -> Result<ReferenceData, String> {
    let size = Size::new(rgba.width(), rgba.height());
    let mut bgra = rgba.as_raw().clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    // Match the screen adapter's normalization, including letterboxing, before
    // sampling the reference. Stretching imports changes their detector geometry.
    let mut normalized = vec![0; (PIPELINE_SIZE.width * PIPELINE_SIZE.height * 4) as usize];
    aspect_fit_bgra_into(&bgra, size, size.width * 4, &mut normalized, PIPELINE_SIZE)
        .map_err(|error| format!("could not normalize reference image: {error:?}"))?;
    let detector = resize_bgra_to_gray(
        &normalized,
        PIPELINE_SIZE,
        PIPELINE_SIZE.width as usize * 4,
        Size::new(160, 90),
    )
    .map_err(|error| format!("could not prepare reference detector image: {error:?}"))?;
    let preview_rgba = if size.width > REFERENCE_PREVIEW_SIZE.width
        || size.height > REFERENCE_PREVIEW_SIZE.height
    {
        image::DynamicImage::ImageRgba8(rgba.clone())
            .thumbnail(REFERENCE_PREVIEW_SIZE.width, REFERENCE_PREVIEW_SIZE.height)
            .to_rgba8()
    } else {
        rgba.clone()
    };
    let preview_size = Size::new(preview_rgba.width(), preview_rgba.height());
    let mut preview_bgra = preview_rgba.into_raw();
    for pixel in preview_bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let preview = Frame::new(
        preview_bgra.into(),
        preview_size,
        preview_size.width * 4,
        0,
        0,
        Instant::now(),
    )
    .map(Arc::new)
    .map_err(|error| format!("could not prepare reference preview: {error:?}"))?;
    Ok(ReferenceData { detector, preview })
}

pub(super) fn pending_reference_path(destination: &std::path::Path) -> std::path::PathBuf {
    destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("reference.pending.png")
}

pub(super) fn cleanup_pending_reference(destination: &std::path::Path) {
    let pending = pending_reference_path(destination);
    if let Err(error) = std::fs::remove_file(&pending)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        // A later atomic write will report an actionable error if this matters.
    }
}

#[cfg(test)]
pub(super) fn persist_frame_atomic(
    frame: &Frame,
    destination: &std::path::Path,
) -> Result<(), String> {
    persist_frame_guarded(frame, destination, None)
}

pub(super) fn persist_frame_guarded(
    frame: &Frame,
    destination: &std::path::Path,
    commit: Option<&ReferenceCommit>,
) -> Result<(), String> {
    let mut rgba = frame.pixels().to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let rgba = image::RgbaImage::from_raw(frame.size.width, frame.size.height, rgba)
        .ok_or_else(|| "reference frame layout is invalid".to_owned())?;
    persist_rgba_guarded(&rgba, destination, commit)
}

#[cfg(test)]
pub(super) fn persist_rgba_atomic(
    rgba: &image::RgbaImage,
    destination: &std::path::Path,
) -> Result<(), String> {
    persist_rgba_guarded(rgba, destination, None)
}

pub(super) fn persist_rgba_guarded(
    rgba: &image::RgbaImage,
    destination: &std::path::Path,
    commit: Option<&ReferenceCommit>,
) -> Result<(), String> {
    if destination.as_os_str().is_empty() {
        return Err("reference image path is empty".into());
    }
    if let Some(parent) = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create reference directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let pending = pending_reference_path(destination);
    let mut last_error = None;
    for attempt in 0..3 {
        if commit.is_some_and(ReferenceCommit::is_cancelled) {
            cleanup_pending_reference(destination);
            return Err("Reference save was cancelled before replacement".into());
        }
        if attempt > 0 {
            let delay = Duration::from_millis(50 * (1_u64 << (attempt - 1)));
            thread::sleep(delay);
        }
        cleanup_pending_reference(destination);
        match persist_rgba_once(rgba, destination, &pending, commit) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(format!("attempt {}: {error}", attempt + 1));
            }
        }
    }
    cleanup_pending_reference(destination);
    Err(format!(
        "could not persist reference image at {} (pending {}): {}",
        destination.display(),
        pending.display(),
        last_error.unwrap_or_else(|| "unknown persistence failure".into())
    ))
}

pub(super) fn persist_rgba_once(
    rgba: &image::RgbaImage,
    destination: &std::path::Path,
    pending: &std::path::Path,
    commit: Option<&ReferenceCommit>,
) -> Result<(), String> {
    image::save_buffer(
        pending,
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
        image::ColorType::Rgba8,
    )
    .map_err(|error| {
        format!(
            "could not encode pending reference image {}: {error}",
            pending.display()
        )
    })?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(pending)
        .map_err(|error| {
            format!(
                "could not open pending reference image {} for flush: {error}",
                pending.display()
            )
        })?;
    file.sync_all().map_err(|error| {
        format!(
            "could not flush pending reference image {}: {error}",
            pending.display()
        )
    })?;
    validate_pending_reference(pending, rgba.width(), rgba.height())?;
    if let Some(commit) = commit {
        commit.claim()?;
    }
    replace_reference_atomic(pending, destination)
}

pub(super) fn validate_pending_reference(
    pending: &std::path::Path,
    expected_width: u32,
    expected_height: u32,
) -> Result<(), String> {
    use image::ImageDecoder;
    let decoder = image::codecs::png::PngDecoder::new(std::io::BufReader::new(
        std::fs::File::open(pending).map_err(|error| {
            format!(
                "could not reopen pending reference image {}: {error}",
                pending.display()
            )
        })?,
    ))
    .map_err(|error| {
        format!(
            "pending reference {} is not a valid PNG: {error}",
            pending.display()
        )
    })?;
    let dimensions = decoder.dimensions();
    if dimensions != (expected_width, expected_height) {
        return Err(format!(
            "pending reference {} dimensions changed from {expected_width}x{expected_height} to {}x{}",
            pending.display(),
            dimensions.0,
            dimensions.1
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn replace_reference_atomic(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    stageswap_windows::replace_file_atomic(source, destination).map_err(|error| {
        format!(
            "could not atomically replace reference image {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

#[cfg(not(windows))]
pub(super) fn replace_reference_atomic(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    std::fs::rename(source, destination).map_err(|error| {
        format!(
            "could not atomically replace reference image {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

#[cfg(test)]
pub(super) fn load_reference(path: &str) -> Option<(GrayImage, Arc<Frame>)> {
    let data = decode_reference(std::path::Path::new(path)).ok()?;
    Some((data.detector, data.preview))
}

#[cfg(test)]
pub(super) fn save_reference(frame: &Frame, path: &str) -> Result<(), String> {
    persist_frame_atomic(frame, std::path::Path::new(path))
}
