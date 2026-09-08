use super::*;

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy)]
#[repr(usize)]
pub(super) enum WarningSource {
    DeviceWorker = 0,
    PublisherSink = 1,
    PublisherController = 2,
    VirtualCamera = 3,
    WebcamCapture = 4,
    ScreenCapture = 5,
    Hdr = 6,
    Reference = 7,
    Command = 8,
}

impl WarningSource {
    pub(super) const fn alert_source(self) -> RuntimeAlertSource {
        match self {
            Self::DeviceWorker => RuntimeAlertSource::DeviceWorker,
            Self::PublisherSink | Self::PublisherController => RuntimeAlertSource::Publisher,
            Self::VirtualCamera => RuntimeAlertSource::VirtualCamera,
            Self::WebcamCapture => RuntimeAlertSource::Webcam,
            Self::ScreenCapture => RuntimeAlertSource::Screen,
            Self::Hdr => RuntimeAlertSource::Matching,
            Self::Reference => RuntimeAlertSource::Reference,
            Self::Command => RuntimeAlertSource::Command,
        }
    }
}

pub(super) const WARNING_SOURCE_COUNT: usize = 9;

#[derive(Clone)]
pub(super) struct WarningRegistry {
    pub(super) entries: [Option<String>; WARNING_SOURCE_COUNT],
}

impl Default for WarningRegistry {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
        }
    }
}

impl WarningRegistry {
    pub(super) fn set(&mut self, source: WarningSource, message: impl Into<String>) -> bool {
        let message = message.into();
        let entry = &mut self.entries[source as usize];
        if entry.as_ref() == Some(&message) {
            return false;
        }
        *entry = Some(message);
        true
    }

    pub(super) fn clear(&mut self, source: WarningSource) -> bool {
        self.entries[source as usize].take().is_some()
    }

    pub(super) fn top(&self) -> Option<String> {
        self.entries.iter().find_map(Clone::clone)
    }

    pub(super) fn active_alerts(&self) -> Vec<RuntimeWarning> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                message.as_ref().map(|message| RuntimeWarning {
                    source: warning_source_from_index(index).alert_source(),
                    message: message.clone(),
                })
            })
            .collect()
    }

    #[cfg(windows)]
    pub(super) fn copy_device_sources_from(
        &mut self,
        source: &Self,
    ) -> Vec<(WarningSource, String)> {
        let mut changed = Vec::new();
        for warning_source in [
            WarningSource::PublisherController,
            WarningSource::VirtualCamera,
            WarningSource::WebcamCapture,
            WarningSource::ScreenCapture,
            WarningSource::Hdr,
        ] {
            let source_entry = &source.entries[warning_source as usize];
            let destination = &mut self.entries[warning_source as usize];
            if destination != source_entry {
                destination.clone_from(source_entry);
                if let Some(message) = source_entry {
                    changed.push((warning_source, message.clone()));
                }
            }
        }
        changed
    }
}

pub(super) fn warning_source_from_index(index: usize) -> WarningSource {
    match index {
        0 => WarningSource::DeviceWorker,
        1 => WarningSource::PublisherSink,
        2 => WarningSource::PublisherController,
        3 => WarningSource::VirtualCamera,
        4 => WarningSource::WebcamCapture,
        5 => WarningSource::ScreenCapture,
        6 => WarningSource::Hdr,
        7 => WarningSource::Reference,
        8 => WarningSource::Command,
        _ => unreachable!("warning source index is bounded by WARNING_SOURCE_COUNT"),
    }
}
