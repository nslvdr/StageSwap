use super::diagnostics;
use stageswap_core::{
    CAPTURE_FRAME_POOL_CAPACITY, FrameBufferPool, FrameHeader, HEADER_LEN, MAX_FRAME_BYTES,
    SharedFrameCache,
};
use std::fs::OpenOptions;
use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::IO::CancelSynchronousIo;

pub(super) struct PipeReader {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PipeReader {
    pub(super) fn start(pipe_name: String, cache: Arc<Mutex<SharedFrameCache>>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        if pipe_name.is_empty() {
            diagnostics::always("pipe reader was given an empty frame-pipe name");
            return Self { stop, worker: None };
        }
        let path = if pipe_name.starts_with(r"\\.\pipe\") {
            pipe_name
        } else {
            format!(r"\\.\pipe\{pipe_name}")
        };
        let pipe_tag = diagnostics::path_tag(&path);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("stageswap-mf-pipe-reader".into())
            .spawn(move || reader_loop(&path, &worker_stop, &cache))
            .map_err(|error| {
                diagnostics::always(format!(
                    "could not start pipe reader pipe_tag={pipe_tag:016x}: {error}"
                ));
                error
            })
            .ok();
        Self { stop, worker }
    }

    pub(super) fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            // SAFETY: the raw handle belongs to the still-live worker thread and
            // is used only to cancel its synchronous named-pipe open or read.
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

impl Drop for PipeReader {
    fn drop(&mut self) {
        self.stop();
    }
}

fn reader_loop(path: &str, stop: &AtomicBool, cache: &Mutex<SharedFrameCache>) {
    let mut pool = FrameBufferPool::new(MAX_FRAME_BYTES as usize, CAPTURE_FRAME_POOL_CAPACITY);
    let pipe_tag = diagnostics::path_tag(path);
    let mut ingested_frames = 0_u64;
    let mut last_ingest_report = Instant::now() - Duration::from_secs(5);
    while !stop.load(Ordering::Acquire) {
        let mut pipe = match OpenOptions::new().read(true).open(path) {
            Ok(pipe) => {
                let Ok(mut cache) = cache.lock() else {
                    return;
                };
                cache.reset_for_new_connection();
                drop(cache);
                diagnostics::rate_limited(
                    "pipe-connected",
                    Duration::from_secs(5),
                    format!("pipe connected pipe_tag={pipe_tag:016x}"),
                );
                pipe
            }
            Err(error) => {
                diagnostics::rate_limited(
                    "pipe-open-retry",
                    Duration::from_secs(5),
                    format!("pipe open retry pipe_tag={pipe_tag:016x} error={error}"),
                );
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        };
        let mut previous_sequence = None;
        let mut connection_ingested = false;
        while !stop.load(Ordering::Acquire) {
            let mut encoded = [0; HEADER_LEN];
            if let Err(error) = pipe.read_exact(&mut encoded) {
                diagnostics::rate_limited(
                    "pipe-header-read",
                    Duration::from_secs(5),
                    format!("pipe header read ended pipe_tag={pipe_tag:016x} error={error}"),
                );
                break;
            }
            let header = match FrameHeader::decode(&encoded, previous_sequence) {
                Ok(header) => header,
                Err(error) => {
                    diagnostics::rate_limited(
                        "pipe-header-invalid",
                        Duration::from_secs(5),
                        format!("pipe header was invalid pipe_tag={pipe_tag:016x} error={error:?}"),
                    );
                    if let Ok(mut cache) = cache.lock() {
                        cache.invalidate();
                    }
                    break;
                }
            };
            previous_sequence = Some(header.sequence);
            if header.frame_bytes == 0 {
                let Ok(mut cache) = cache.lock() else {
                    return;
                };
                if cache.ingest(header, Arc::from([]), Instant::now()).is_err() {
                    diagnostics::rate_limited(
                        "pipe-invalidation-invalid",
                        Duration::from_secs(5),
                        format!(
                            "pipe invalidation was rejected pipe_tag={pipe_tag:016x} sequence={}",
                            header.sequence
                        ),
                    );
                    cache.invalidate();
                    break;
                }
                diagnostics::rate_limited(
                    "pipe-invalidation",
                    Duration::from_secs(5),
                    format!(
                        "pipe invalidation ingested pipe_tag={pipe_tag:016x} sequence={}",
                        header.sequence
                    ),
                );
                continue;
            }
            let pixels = match read_payload(&mut pipe, &mut pool, header.frame_bytes as usize) {
                Ok(Some(pixels)) => pixels,
                Ok(None) => continue,
                Err(error) => {
                    diagnostics::rate_limited(
                        "pipe-payload-read",
                        Duration::from_secs(5),
                        format!("pipe payload read failed pipe_tag={pipe_tag:016x} error={error}"),
                    );
                    break;
                }
            };
            let Ok(mut cache) = cache.lock() else {
                return;
            };
            if cache.ingest(header, pixels, Instant::now()).is_err() {
                diagnostics::rate_limited(
                    "pipe-frame-invalid",
                    Duration::from_secs(5),
                    format!(
                        "pipe frame was rejected pipe_tag={pipe_tag:016x} sequence={}",
                        header.sequence
                    ),
                );
                cache.invalidate();
                break;
            }
            ingested_frames = ingested_frames.saturating_add(1);
            let now = Instant::now();
            if !connection_ingested
                || now.saturating_duration_since(last_ingest_report) >= Duration::from_secs(5)
            {
                diagnostics::always(format!(
                    "pipe frames ingested pipe_tag={pipe_tag:016x} count={ingested_frames} sequence={}",
                    header.sequence
                ));
                connection_ingested = true;
                last_ingest_report = now;
            }
        }
        if let Ok(mut cache) = cache.lock() {
            cache.invalidate();
        }
        if !stop.load(Ordering::Acquire) {
            diagnostics::rate_limited(
                "pipe-disconnected",
                Duration::from_secs(5),
                format!("pipe disconnected pipe_tag={pipe_tag:016x}"),
            );
        }
    }
}

fn read_payload(
    reader: &mut impl Read,
    pool: &mut FrameBufferPool,
    frame_bytes: usize,
) -> std::io::Result<Option<Arc<[u8]>>> {
    match pool.try_write_sized(frame_bytes, |destination| reader.read_exact(destination))? {
        Some(pixels) => Ok(Some(pixels)),
        None => {
            discard_exact(reader, frame_bytes)?;
            Ok(None)
        }
    }
}

fn discard_exact(reader: &mut impl Read, mut remaining: usize) -> std::io::Result<()> {
    let mut scratch = [0; 8192];
    while remaining > 0 {
        let chunk = remaining.min(scratch.len());
        reader.read_exact(&mut scratch[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn native_repeated_pipe_payloads_reuse_slots_after_warmup() {
        let mut pool = FrameBufferPool::new(16, CAPTURE_FRAME_POOL_CAPACITY);
        let mut first_reader = Cursor::new(vec![1; 16]);
        let first = read_payload(&mut first_reader, &mut pool, 16)
            .unwrap()
            .unwrap();
        let first_pointer = first.as_ptr();

        let mut second_reader = Cursor::new(vec![2; 16]);
        let second = read_payload(&mut second_reader, &mut pool, 16)
            .unwrap()
            .unwrap();
        assert!(first.iter().all(|byte| *byte == 1));
        drop(first);

        let mut third_reader = Cursor::new(vec![3; 16]);
        let third = read_payload(&mut third_reader, &mut pool, 16)
            .unwrap()
            .unwrap();
        assert_eq!(third.as_ptr(), first_pointer);
        assert!(second.iter().all(|byte| *byte == 2));
        assert!(third.iter().all(|byte| *byte == 3));
        assert_eq!(pool.allocated_slots(), 2);
    }

    #[test]
    fn native_exhausted_pipe_pool_drains_dropped_payload_without_aliasing() {
        let mut pool = FrameBufferPool::new(4, 2);
        let mut reader = Cursor::new(vec![1; 4]);
        let first = read_payload(&mut reader, &mut pool, 4).unwrap().unwrap();
        let mut reader = Cursor::new(vec![2; 4]);
        let second = read_payload(&mut reader, &mut pool, 4).unwrap().unwrap();
        let mut reader = Cursor::new(vec![3; 4]);
        assert!(read_payload(&mut reader, &mut pool, 4).unwrap().is_none());
        assert_eq!(reader.position(), 4);
        assert_eq!(first.as_ref(), &[1; 4]);
        assert_eq!(second.as_ref(), &[2; 4]);
        assert_eq!(pool.exhaustion_count(), 1);
    }
}
