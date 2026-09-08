use stageswap_core::AppConfig;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    pending: Option<(u64, AppConfig)>,
    result: Option<(u64, Result<(), String>)>,
    generation: u64,
    active: bool,
    stopping: bool,
}

/// Serializes settings writes with one last-wins pending value and one result slot.
pub(super) struct SettingsPersistence {
    shared: Arc<(Mutex<State>, Condvar)>,
    done: mpsc::Receiver<()>,
    worker: Option<JoinHandle<()>>,
}

impl SettingsPersistence {
    pub(super) fn start(
        mut save: impl FnMut(&AppConfig) -> Result<(), String> + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let (done_tx, done) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("stageswap-settings-io".into())
            .spawn(move || {
                let (lock, changed) = &*worker_shared;
                loop {
                    let (generation, config) = {
                        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                        while state.pending.is_none() && !state.stopping {
                            state = changed
                                .wait(state)
                                .unwrap_or_else(|error| error.into_inner());
                        }
                        let Some(job) = state.pending.take() else {
                            break;
                        };
                        state.active = true;
                        job
                    };
                    let result = save(&config);
                    let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                    state.active = false;
                    state.result = Some((generation, result));
                    changed.notify_all();
                }
                let _ = done_tx.send(());
            })
            .map_err(|error| format!("Could not start settings writer: {error}"))?;
        Ok(Self {
            shared,
            done,
            worker: Some(worker),
        })
    }

    pub(super) fn submit(&self, config: AppConfig) {
        let (lock, changed) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        state.generation = state.generation.wrapping_add(1);
        state.pending = Some((state.generation, config));
        changed.notify_one();
    }

    pub(super) fn poll(&self) -> Option<Result<(), String>> {
        let mut state = self
            .shared
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state
            .result
            .take()
            .and_then(|(generation, result)| (generation == state.generation).then_some(result))
    }

    /// Barriers are reserved for explicit restoration and shutdown, never UI refreshes.
    pub(super) fn drain(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (lock, changed) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        while state.active || state.pending.is_some() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            state = changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        true
    }
}

impl Drop for SettingsPersistence {
    fn drop(&mut self) {
        let (lock, changed) = &*self.shared;
        {
            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
            state.stopping = true;
            changed.notify_all();
        }
        // The worker drains the newest settings before exiting. A stuck filesystem
        // must not keep the process alive indefinitely or start a competing writer.
        if self.done.recv_timeout(Duration::from_secs(2)).is_ok()
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_settings_writer_coalesces_during_io_and_reports_only_latest_result() {
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let saved = Arc::new(Mutex::new(Vec::new()));
        let worker_saved = Arc::clone(&saved);
        let worker = SettingsPersistence::start(move |config| {
            if config.selected_video_device_id == "first" {
                started_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
                return Err("obsolete failure".into());
            }
            worker_saved
                .lock()
                .unwrap()
                .push(config.selected_video_device_id.clone());
            Ok(())
        })
        .unwrap();
        let config = |id: &str| AppConfig {
            selected_video_device_id: id.into(),
            ..AppConfig::default()
        };
        worker.submit(config("first"));
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.submit(config("second"));
        worker.submit(config("latest"));
        assert!(!worker.drain(Duration::ZERO));
        resume_tx.send(()).unwrap();
        assert!(worker.drain(Duration::from_secs(2)));
        assert_eq!(*saved.lock().unwrap(), ["latest"]);
        assert_eq!(worker.poll(), Some(Ok(())));
    }

    #[test]
    fn flow_settings_writer_shutdown_drains_the_final_value_and_reports_errors() {
        let saved = Arc::new(Mutex::new(false));
        let worker_saved = Arc::clone(&saved);
        let worker = SettingsPersistence::start(move |_| {
            *worker_saved.lock().unwrap() = true;
            Err("disk full".into())
        })
        .unwrap();
        worker.submit(AppConfig::default());
        assert!(worker.drain(Duration::from_secs(2)));
        assert_eq!(worker.poll(), Some(Err("disk full".into())));
        *saved.lock().unwrap() = false;
        worker.submit(AppConfig::default());
        drop(worker);
        assert!(*saved.lock().unwrap());
    }
}
