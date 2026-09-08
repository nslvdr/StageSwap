use stageswap_core::Frame;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PREVIEW_REFRESH_EARLY_TOLERANCE: Duration = Duration::from_millis(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreviewFrameId {
    pub(crate) sequence: u64,
    received_at: Instant,
}

impl PreviewFrameId {
    fn from_frame(frame: &Frame) -> Self {
        Self {
            sequence: frame.sequence,
            received_at: frame.received_at,
        }
    }
}

struct PreviewJob {
    frame: Arc<Frame>,
    id: PreviewFrameId,
    size: [usize; 2],
    generation: u64,
}

pub(crate) struct PreparedPreview {
    pub(crate) id: PreviewFrameId,
    pub(crate) size: [usize; 2],
    pub(crate) image: eframe::egui::ColorImage,
}

#[derive(Default)]
pub(crate) struct PreviewConverterState {
    latest_request: Option<(PreviewFrameId, [usize; 2], Duration)>,
    last_submitted_at: Option<Instant>,
    pending: Option<PreviewJob>,
    pub(crate) ready: Option<PreparedPreview>,
    generation: u64,
    stopping: bool,
}

pub(crate) struct PreviewConverter {
    shared: Arc<(Mutex<PreviewConverterState>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl PreviewConverter {
    pub(crate) fn new(key: &'static str) -> Self {
        let shared = Arc::new((Mutex::new(PreviewConverterState::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name(format!("stageswap-preview-{key}"))
            .spawn(move || preview_converter_loop(&worker_shared))
            .expect("preview conversion worker can be created");
        Self {
            shared,
            worker: Some(worker),
        }
    }

    pub(crate) fn submit(
        &self,
        frame: Arc<Frame>,
        size: [usize; 2],
        minimum_interval: Duration,
        now: Instant,
    ) {
        let (state, wake) = &*self.shared;
        let mut state = state
            .lock()
            .expect("preview converter state is not poisoned");
        let id = PreviewFrameId::from_frame(&frame);
        if state.latest_request.as_ref().is_some_and(
            |(requested, requested_size, requested_interval)| {
                *requested == id
                    && *requested_size == size
                    && *requested_interval == minimum_interval
            },
        ) {
            return;
        }
        let profile_changed =
            state
                .latest_request
                .as_ref()
                .is_some_and(|(_, requested_size, requested_interval)| {
                    *requested_size != size || *requested_interval != minimum_interval
                });
        if !profile_changed
            && state.last_submitted_at.is_some_and(|submitted| {
                now.saturating_duration_since(submitted)
                    < minimum_interval.saturating_sub(PREVIEW_REFRESH_EARLY_TOLERANCE)
            })
        {
            return;
        }
        state.latest_request = Some((id, size, minimum_interval));
        state.last_submitted_at = Some(now);
        state.pending = Some(PreviewJob {
            frame,
            id,
            size,
            generation: state.generation,
        });
        wake.notify_one();
    }

    pub(crate) fn take_ready(&self) -> Option<PreparedPreview> {
        self.shared
            .0
            .lock()
            .expect("preview converter state is not poisoned")
            .ready
            .take()
    }

    pub(crate) fn suspend(&self) {
        let (state, _) = &*self.shared;
        let mut state = state
            .lock()
            .expect("preview converter state is not poisoned");
        state.generation = state.generation.wrapping_add(1);
        state.latest_request = None;
        state.last_submitted_at = None;
        state.pending = None;
        state.ready = None;
    }
}

impl Drop for PreviewConverter {
    fn drop(&mut self) {
        let (state, wake) = &*self.shared;
        if let Ok(mut state) = state.lock() {
            state.stopping = true;
            state.pending = None;
            wake.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn preview_converter_loop(shared: &Arc<(Mutex<PreviewConverterState>, Condvar)>) {
    loop {
        let job = {
            let (state, wake) = &**shared;
            let mut state = state
                .lock()
                .expect("preview converter state is not poisoned");
            while state.pending.is_none() && !state.stopping {
                state = wake
                    .wait(state)
                    .expect("preview converter state is not poisoned");
            }
            if state.stopping {
                return;
            }
            state
                .pending
                .take()
                .expect("pending preview job is present")
        };
        let prepared = PreparedPreview {
            image: super::frame_image(&job.frame, job.size),
            id: job.id,
            size: job.size,
        };
        let mut state = shared
            .0
            .lock()
            .expect("preview converter state is not poisoned");
        store_completed_preview(&mut state, job.generation, prepared);
    }
}

pub(crate) fn store_completed_preview(
    state: &mut PreviewConverterState,
    generation: u64,
    prepared: PreparedPreview,
) {
    if !state.stopping && state.generation == generation {
        state.ready = Some(prepared);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn contract_completed_frame_is_kept_when_a_newer_request_is_pending() {
        let now = Instant::now();
        let completed = Arc::new(Frame::placeholder(
            stageswap_core::Size::new(2, 2),
            0xff00_0001,
            1,
            0,
            now,
        ));
        let pending = Arc::new(Frame::placeholder(
            stageswap_core::Size::new(2, 2),
            0xff00_0002,
            2,
            0,
            now,
        ));
        let size = [2, 2];
        let pending_id = PreviewFrameId::from_frame(&pending);
        let mut state = PreviewConverterState {
            latest_request: Some((pending_id, size, Duration::ZERO)),
            pending: Some(PreviewJob {
                frame: Arc::clone(&pending),
                id: pending_id,
                size,
                generation: 0,
            }),
            ..PreviewConverterState::default()
        };

        store_completed_preview(
            &mut state,
            0,
            PreparedPreview {
                image: super::super::frame_image(&completed, size),
                id: PreviewFrameId::from_frame(&completed),
                size,
            },
        );

        assert_eq!(state.ready.as_ref().unwrap().id.sequence, 1);
        assert_eq!(state.pending.as_ref().unwrap().frame.sequence, 2);
    }

    #[test]
    fn contract_suspension_discards_in_flight_results_and_source_frames() {
        let converter = PreviewConverter::new("release-test");
        let now = Instant::now();
        let frame = Arc::new(Frame::placeholder(
            stageswap_core::Size::new(640, 360),
            0xff00_0001,
            1,
            0,
            now,
        ));
        let pixels = frame.pixels_arc();
        converter.submit(
            Arc::clone(&frame),
            [240, 135],
            Duration::from_millis(100),
            now,
        );
        converter.suspend();
        drop(frame);

        let deadline = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(&pixels) > 1 && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(Arc::strong_count(&pixels), 1);
        assert!(converter.take_ready().is_none());
    }

    #[test]
    fn contract_submission_rate_is_limited_but_profile_changes_are_immediate() {
        let converter = PreviewConverter::new("cadence-test");
        let now = Instant::now();
        let frame = |sequence| {
            Arc::new(Frame::placeholder(
                stageswap_core::Size::new(1280, 720),
                0xff00_0000 | sequence as u32,
                sequence,
                0,
                now + Duration::from_millis(sequence),
            ))
        };
        let embedded = Duration::from_millis(100);
        converter.submit(frame(1), [240, 135], embedded, now);
        converter.submit(
            frame(2),
            [240, 135],
            embedded,
            now + Duration::from_millis(50),
        );
        {
            let state = converter.shared.0.lock().unwrap();
            assert_eq!(state.latest_request.unwrap().0.sequence, 1);
        }

        let enlarged = Duration::from_nanos(1_000_000_000 / 30);
        converter.submit(
            frame(3),
            [1280, 720],
            enlarged,
            now + Duration::from_millis(50),
        );
        let state = converter.shared.0.lock().unwrap();
        assert_eq!(state.latest_request.unwrap().0.sequence, 3);
        assert_eq!(state.latest_request.unwrap().1, [1280, 720]);
    }

    #[test]
    fn contract_thirty_fps_cadence_accepts_small_repaint_jitter_without_halving() {
        let converter = PreviewConverter::new("jitter-test");
        let now = Instant::now();
        let interval = Duration::from_nanos(1_000_000_000 / 30);
        let frame = |sequence, elapsed| {
            Arc::new(Frame::placeholder(
                stageswap_core::Size::new(1280, 720),
                0xff00_0000 | sequence as u32,
                sequence,
                0,
                now + elapsed,
            ))
        };

        converter.submit(frame(1, Duration::ZERO), [240, 135], interval, now);
        converter.submit(
            frame(2, Duration::from_millis(20)),
            [240, 135],
            interval,
            now + Duration::from_millis(20),
        );
        {
            let state = converter.shared.0.lock().unwrap();
            assert_eq!(state.latest_request.unwrap().0.sequence, 1);
        }

        converter.submit(
            frame(3, Duration::from_millis(32)),
            [240, 135],
            interval,
            now + Duration::from_millis(32),
        );
        let state = converter.shared.0.lock().unwrap();
        assert_eq!(state.latest_request.unwrap().0.sequence, 3);
    }
}
