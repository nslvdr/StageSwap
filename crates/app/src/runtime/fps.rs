use super::*;

#[derive(Default)]
pub(super) struct SourceFpsTracker {
    pub(super) samples: VecDeque<(Instant, u64)>,
    pub(super) last_sequence: Option<u64>,
    pub(super) last_received_at: Option<Instant>,
    pub(super) displayed: Option<u32>,
}

impl SourceFpsTracker {
    pub(super) fn observe(&mut self, frame: Option<&Frame>, now: Instant) -> Option<u32> {
        if let Some(frame) = frame
            && self.last_sequence != Some(frame.sequence)
        {
            if self
                .last_sequence
                .is_some_and(|sequence| frame.sequence <= sequence)
            {
                self.samples.clear();
                self.displayed = None;
            }
            self.last_sequence = Some(frame.sequence);
            self.last_received_at = Some(frame.received_at);
            self.samples.push_back((frame.received_at, frame.sequence));
            while self.samples.front().is_some_and(|(received_at, _)| {
                frame.received_at.saturating_duration_since(*received_at) > FPS_TRACKING_WINDOW
            }) {
                self.samples.pop_front();
            }
            if let (Some((first_time, first_sequence)), Some((last_time, last_sequence))) =
                (self.samples.front(), self.samples.back())
            {
                let elapsed = last_time.saturating_duration_since(*first_time);
                let frames = last_sequence.saturating_sub(*first_sequence);
                if frames > 0 && !elapsed.is_zero() {
                    self.displayed =
                        Some(((frames as f64 / elapsed.as_secs_f64()).round() as u32).min(999));
                }
            }
        }

        if self
            .last_received_at
            .is_some_and(|received| now.saturating_duration_since(received) >= FPS_TRACKING_WINDOW)
        {
            Some(0)
        } else {
            self.displayed
        }
    }
}

#[derive(Default)]
pub(super) struct OutputFpsTracker {
    pub(super) samples: VecDeque<Instant>,
}

impl OutputFpsTracker {
    pub(super) fn observe(&mut self, now: Instant) -> Option<u32> {
        self.samples.push_back(now);
        while self
            .samples
            .front()
            .is_some_and(|sample| now.saturating_duration_since(*sample) > FPS_TRACKING_WINDOW)
        {
            self.samples.pop_front();
        }
        let first = self.samples.front()?;
        let last = self.samples.back()?;
        let elapsed = last.saturating_duration_since(*first);
        let frames = self.samples.len().saturating_sub(1);
        (frames > 0 && !elapsed.is_zero())
            .then(|| ((frames as f64 / elapsed.as_secs_f64()).round() as u32).min(999))
    }
}
