use super::*;

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScreenCaptureRecoveryObservation {
    Clear,
    AwaitingConfirmation,
    Restart,
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub(super) struct ScreenCaptureRecovery {
    pub(super) consecutive_failures: u8,
}

#[cfg(any(windows, test))]
impl ScreenCaptureRecovery {
    pub(super) fn reset(&mut self) {
        self.consecutive_failures = 0;
    }

    pub(super) fn observe(&mut self, frame: Option<&Frame>) -> ScreenCaptureRecoveryObservation {
        if frame.is_some_and(|frame| !is_nearly_black(frame)) {
            self.reset();
            return ScreenCaptureRecoveryObservation::Clear;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures < SCREEN_CAPTURE_FAILURE_CONFIRMATIONS {
            ScreenCaptureRecoveryObservation::AwaitingConfirmation
        } else {
            self.reset();
            ScreenCaptureRecoveryObservation::Restart
        }
    }
}

#[cfg(any(windows, test))]
pub(super) fn is_nearly_black(frame: &Frame) -> bool {
    let Some(thumbnail) = gray_thumbnail(frame) else {
        return false;
    };
    gray_image_is_nearly_black(&thumbnail)
}

pub(super) fn gray_image_is_nearly_black(thumbnail: &GrayImage) -> bool {
    let pixels = &thumbnail.pixels;
    let black_pixels = pixels
        .iter()
        .filter(|pixel| **pixel <= BLACK_LUMA_THRESHOLD)
        .count();
    black_pixels * 100 >= pixels.len() * BLACK_PIXEL_PERCENT
}

#[cfg(any(windows, test))]
pub(super) fn automatic_screen_tasks_due(
    automatic_monitor_rescans: bool,
    automatic_screen_capture_recovery: bool,
    last_monitor_scan: Instant,
    last_screen_capture_recovery_check: Instant,
    now: Instant,
) -> (bool, bool) {
    (
        automatic_monitor_rescans
            && now.duration_since(last_monitor_scan) >= AUTOMATIC_SCREEN_CHECK_INTERVAL,
        automatic_screen_capture_recovery
            && now.duration_since(last_screen_capture_recovery_check)
                >= AUTOMATIC_SCREEN_CHECK_INTERVAL,
    )
}

#[cfg(any(windows, test))]
pub(super) fn screen_restart_backoff(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(4);
    SCREEN_RESTART_BACKOFF_BASE
        .checked_mul(1_u32 << exponent)
        .unwrap_or(SCREEN_RESTART_BACKOFF_MAX)
        .min(SCREEN_RESTART_BACKOFF_MAX)
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct WebcamRecovery {
    pub(super) active: bool,
    pub(super) attempts: u8,
    pub(super) next_attempt: Option<Instant>,
    pub(super) waiting_for_first_frame: bool,
}

#[cfg(any(windows, test))]
impl WebcamRecovery {
    pub(super) fn schedule_initial(&mut self, now: Instant) -> bool {
        if self.active {
            return false;
        }
        self.active = true;
        self.attempts = 0;
        self.waiting_for_first_frame = false;
        self.next_attempt = Some(now + WEBCAM_RECOVERY_DELAYS[0]);
        true
    }

    pub(super) fn begin_due_attempt(&mut self, now: Instant) -> Option<u8> {
        if !self.active
            || self.waiting_for_first_frame
            || self.next_attempt.is_none_or(|attempt_at| now < attempt_at)
            || usize::from(self.attempts) >= WEBCAM_RECOVERY_DELAYS.len()
        {
            return None;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.next_attempt = None;
        Some(self.attempts)
    }

    pub(super) fn attempt_started(&mut self) {
        self.waiting_for_first_frame = true;
        self.next_attempt = None;
    }

    pub(super) fn attempt_failed(&mut self, now: Instant) -> bool {
        self.waiting_for_first_frame = false;
        if usize::from(self.attempts) >= WEBCAM_RECOVERY_DELAYS.len() {
            self.active = false;
            self.next_attempt = None;
            return false;
        }
        self.next_attempt = Some(now + WEBCAM_RECOVERY_DELAYS[usize::from(self.attempts)]);
        true
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn exhaust(&mut self) {
        self.active = false;
        self.waiting_for_first_frame = false;
        self.next_attempt = None;
    }
}

#[cfg(any(windows, test))]
pub(super) fn webcam_failure_is_automatically_recoverable(kind: WebcamFailureKind) -> bool {
    matches!(
        kind,
        WebcamFailureKind::DeviceInvalidated | WebcamFailureKind::DriverFailure
    )
}
