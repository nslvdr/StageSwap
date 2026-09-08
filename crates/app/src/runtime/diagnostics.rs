#[cfg(any(windows, test))]
use super::*;
#[cfg(windows)]
use stageswap_windows::FramePublisherDiagnostics;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActivityVerbosity {
    Normal,
    Verbose,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PublisherDiagnosticsSnapshot {
    pub(super) published_sequence: u64,
    pub(super) transmitted_sequence: u64,
    pub(super) connected: bool,
    pub(super) write_failures: u64,
    pub(super) disconnect_count: u64,
    pub(super) last_write_error: Option<u32>,
    pub(super) last_disconnect_error: Option<u32>,
    pub(super) connection_count: u64,
    pub(super) connection_failures: u64,
}

#[cfg(windows)]
impl From<FramePublisherDiagnostics> for PublisherDiagnosticsSnapshot {
    fn from(value: FramePublisherDiagnostics) -> Self {
        Self {
            published_sequence: value.published_sequence,
            transmitted_sequence: value.transmitted_sequence,
            connected: value.connected,
            write_failures: value.write_failures,
            disconnect_count: value.disconnect_count,
            last_write_error: value.last_write_error,
            last_disconnect_error: value.last_disconnect_error,
            connection_count: value.connection_count,
            connection_failures: value.connection_failures,
        }
    }
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub(super) struct PublisherDiagnosticsState {
    pub(super) last_published_sequence: u64,
    pub(super) last_transmitted_sequence: u64,
    pub(super) last_connected: Option<bool>,
    pub(super) last_write_failures: u64,
    pub(super) last_disconnect_count: u64,
    pub(super) last_write_error: Option<u32>,
    pub(super) last_disconnect_error: Option<u32>,
    pub(super) last_connection_count: u64,
    pub(super) last_connection_failures: u64,
    pub(super) last_reported_at: Option<Instant>,
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub(super) struct CaptureDiagnosticsState {
    pub(super) webcam_dropped: u64,
    pub(super) screen_dropped: u64,
    pub(super) webcam_fallbacks: u64,
    pub(super) screen_fallbacks: u64,
    pub(super) last_reported_at: Option<Instant>,
}

#[cfg(any(windows, test))]
impl CaptureDiagnosticsState {
    pub(super) fn observe(
        &mut self,
        webcam_dropped: u64,
        screen_dropped: u64,
        webcam_fallbacks: u64,
        screen_fallbacks: u64,
        now: Instant,
    ) -> bool {
        let changed = self.webcam_dropped != webcam_dropped
            || self.screen_dropped != screen_dropped
            || self.webcam_fallbacks != webcam_fallbacks
            || self.screen_fallbacks != screen_fallbacks;
        let heartbeat_due = self.last_reported_at.is_none_or(|reported| {
            now.saturating_duration_since(reported) >= Duration::from_secs(5)
        });
        self.webcam_dropped = webcam_dropped;
        self.screen_dropped = screen_dropped;
        self.webcam_fallbacks = webcam_fallbacks;
        self.screen_fallbacks = screen_fallbacks;
        if changed || heartbeat_due {
            self.last_reported_at = Some(now);
            true
        } else {
            false
        }
    }
}
