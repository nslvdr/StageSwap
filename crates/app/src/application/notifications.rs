use stageswap_core::{AppSnapshot, RuntimeAlert, RuntimeAlertSource};
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

pub(crate) const MAX_NOTIFICATION_ENTRIES: usize = 10;
pub(crate) const MAX_NOTIFICATION_TOASTS: usize = 2;
pub(crate) const NOTIFICATION_TOAST_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum NotificationSource {
    Startup,
    Configuration,
    DeviceWorker,
    Publisher,
    VirtualCamera,
    Webcam,
    Screen,
    Matching,
    Reference,
    Command,
    Updates,
    #[cfg_attr(windows, allow(dead_code))]
    Preview,
}

impl From<RuntimeAlertSource> for NotificationSource {
    fn from(source: RuntimeAlertSource) -> Self {
        match source {
            RuntimeAlertSource::DeviceWorker => Self::DeviceWorker,
            RuntimeAlertSource::Publisher => Self::Publisher,
            RuntimeAlertSource::VirtualCamera => Self::VirtualCamera,
            RuntimeAlertSource::Webcam => Self::Webcam,
            RuntimeAlertSource::Screen => Self::Screen,
            RuntimeAlertSource::Matching => Self::Matching,
            RuntimeAlertSource::Reference => Self::Reference,
            RuntimeAlertSource::Command => Self::Command,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NotificationTone {
    Critical,
    Information,
}

#[derive(Clone, Debug)]
pub(crate) struct NotificationItem {
    pub(crate) id: u64,
    pub(crate) tone: NotificationTone,
    pub(crate) source: NotificationSource,
    pub(crate) body: String,
    pub(crate) detail: Option<String>,
    pub(crate) created_at: Instant,
    pub(crate) unread: bool,
    pub(crate) dedupe_key: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NotificationToast {
    pub(crate) notification_id: u64,
    pub(crate) expires_at: Instant,
}

struct NotificationContent {
    body: String,
    detail: Option<String>,
    created_at: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct NotificationCenter {
    entries: VecDeque<NotificationItem>,
    toasts: VecDeque<NotificationToast>,
    active_runtime_keys: HashSet<String>,
    last_runtime_alert_id: u64,
    next_id: u64,
}

impl NotificationCenter {
    pub(crate) fn ingest_runtime_alerts(
        &mut self,
        snapshot: &AppSnapshot,
        enabled: bool,
        now: Instant,
    ) {
        let active_keys: HashSet<_> = snapshot
            .active_warnings
            .iter()
            .map(|warning| runtime_key(warning.source, &warning.message))
            .collect();
        self.active_runtime_keys
            .retain(|key| active_keys.contains(key));

        let first_id = snapshot.recent_alerts_first_id;
        if self.last_runtime_alert_id.saturating_add(1) < first_id {
            self.last_runtime_alert_id = first_id.saturating_sub(1);
        }
        let alerts: Vec<_> = snapshot
            .recent_alerts
            .iter()
            .filter(|alert| alert.id > self.last_runtime_alert_id)
            .cloned()
            .collect();
        for alert in alerts {
            let key = runtime_key(alert.source, &alert.message);
            if self.active_runtime_keys.insert(key.clone()) && enabled {
                self.push_runtime_alert(&alert, now, key);
            }
            self.last_runtime_alert_id = alert.id;
        }
    }

    pub(crate) fn push_critical(
        &mut self,
        source: NotificationSource,
        message: impl Into<String>,
        now: Instant,
        enabled: bool,
    ) {
        if !enabled {
            return;
        }
        let message = message.into();
        let key = format!("critical:{source:?}:{message}");
        self.push_item(
            NotificationTone::Critical,
            source,
            NotificationContent {
                body: message,
                detail: None,
                created_at: None,
            },
            now,
            key,
            true,
        );
    }

    #[cfg(any(windows, test))]
    pub(crate) fn push_critical_with_detail(
        &mut self,
        source: NotificationSource,
        body: impl Into<String>,
        detail: impl Into<String>,
        now: Instant,
        enabled: bool,
    ) {
        if !enabled {
            return;
        }
        let body = body.into();
        let detail = detail.into();
        let key = format!("critical:{source:?}:{body}:{detail}");
        self.push_item(
            NotificationTone::Critical,
            source,
            NotificationContent {
                body,
                detail: Some(detail),
                created_at: None,
            },
            now,
            key,
            true,
        );
    }

    pub(crate) fn push_update(
        &mut self,
        version: &str,
        message: impl Into<String>,
        now: Instant,
        enabled: bool,
    ) {
        if !enabled {
            return;
        }
        self.push_item(
            NotificationTone::Information,
            NotificationSource::Updates,
            NotificationContent {
                body: message.into(),
                detail: None,
                created_at: None,
            },
            now,
            format!("update:{version}"),
            true,
        );
    }

    #[cfg(not(windows))]
    pub(crate) fn push_information(
        &mut self,
        source: NotificationSource,
        message: impl Into<String>,
        now: Instant,
    ) {
        let message = message.into();
        self.push_item(
            NotificationTone::Information,
            source,
            NotificationContent {
                body: message.clone(),
                detail: None,
                created_at: None,
            },
            now,
            format!("information:{source:?}:{message}"),
            true,
        );
    }

    pub(crate) fn prune(&mut self, now: Instant) {
        self.toasts.retain(|toast| toast.expires_at > now);
    }

    pub(crate) fn next_toast_deadline(&self) -> Option<Instant> {
        self.toasts.iter().map(|toast| toast.expires_at).min()
    }

    pub(crate) fn mark_all_read(&mut self) {
        for entry in &mut self.entries {
            entry.unread = false;
        }
    }

    pub(crate) fn clear_all(&mut self) {
        self.entries.clear();
        self.toasts.clear();
    }

    pub(crate) fn unread_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.unread).count()
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = &NotificationItem> {
        self.entries.iter()
    }

    pub(crate) fn toasts(&self) -> impl Iterator<Item = &NotificationToast> {
        self.toasts.iter()
    }

    pub(crate) fn entry(&self, id: u64) -> Option<&NotificationItem> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    #[cfg(test)]
    pub(crate) fn last_runtime_alert_id(&self) -> u64 {
        self.last_runtime_alert_id
    }

    fn push_runtime_alert(&mut self, alert: &RuntimeAlert, now: Instant, key: String) {
        self.push_item(
            NotificationTone::Critical,
            alert.source.into(),
            NotificationContent {
                body: alert.message.clone(),
                detail: None,
                created_at: Some(alert.created_at),
            },
            now,
            key,
            false,
        );
    }

    fn push_item(
        &mut self,
        tone: NotificationTone,
        source: NotificationSource,
        content: NotificationContent,
        now: Instant,
        dedupe_key: String,
        dedupe_existing: bool,
    ) {
        if dedupe_existing
            && self
                .entries
                .iter()
                .any(|entry| entry.dedupe_key == dedupe_key)
        {
            return;
        }
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = self.next_id;
        self.entries.push_front(NotificationItem {
            id,
            tone,
            source,
            body: content.body,
            detail: content.detail,
            created_at: content.created_at.unwrap_or(now),
            unread: true,
            dedupe_key,
        });
        while self.entries.len() > MAX_NOTIFICATION_ENTRIES {
            self.entries.pop_back();
        }
        self.toasts.push_front(NotificationToast {
            notification_id: id,
            expires_at: now + NOTIFICATION_TOAST_DURATION,
        });
        while self.toasts.len() > MAX_NOTIFICATION_TOASTS {
            self.toasts.pop_back();
        }
    }
}

fn runtime_key(source: RuntimeAlertSource, message: &str) -> String {
    format!("runtime:{source:?}:{message}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use stageswap_core::{RuntimeAlert, RuntimeWarning};
    use std::sync::Arc;

    fn snapshot_with_alert(id: u64, source: RuntimeAlertSource, message: &str) -> AppSnapshot {
        AppSnapshot {
            active_warnings: Arc::from([RuntimeWarning {
                source,
                message: message.into(),
            }]),
            recent_alerts_first_id: id,
            recent_alerts: Arc::from([RuntimeAlert {
                id,
                source,
                message: message.into(),
                created_at: Instant::now(),
            }]),
            ..AppSnapshot::default()
        }
    }

    #[test]
    fn contract_runtime_alerts_are_deduplicated_until_cleared() {
        let now = Instant::now();
        let mut center = NotificationCenter::default();
        let active = snapshot_with_alert(1, RuntimeAlertSource::Webcam, "camera failed");
        center.ingest_runtime_alerts(&active, true, now);
        center.ingest_runtime_alerts(&active, true, now + Duration::from_secs(1));
        assert_eq!(center.entries.len(), 1);
        assert_eq!(center.last_runtime_alert_id(), 1);

        center.ingest_runtime_alerts(&AppSnapshot::default(), true, now);
        let repeated = snapshot_with_alert(2, RuntimeAlertSource::Webcam, "camera failed");
        center.ingest_runtime_alerts(&repeated, true, now + Duration::from_secs(2));
        assert_eq!(center.entries.len(), 2);
    }

    #[test]
    fn contract_notification_center_caps_history_and_toasts() {
        let now = Instant::now();
        let mut center = NotificationCenter::default();
        for index in 0..(MAX_NOTIFICATION_ENTRIES + 3) {
            center.push_critical(
                NotificationSource::Configuration,
                format!("failure-{index}"),
                now,
                true,
            );
        }
        assert_eq!(center.entries.len(), MAX_NOTIFICATION_ENTRIES);
        assert_eq!(center.toasts.len(), MAX_NOTIFICATION_TOASTS);
    }

    #[test]
    fn contract_notification_center_honors_visibility_and_toast_expiry() {
        let now = Instant::now();
        let mut center = NotificationCenter::default();
        center.push_critical(NotificationSource::Configuration, "hidden", now, false);
        assert_eq!(center.entries.len(), 0);
        center.push_update("2.0.0", "update", now, true);
        assert_eq!(center.unread_count(), 1);
        center.mark_all_read();
        assert_eq!(center.unread_count(), 0);
        center.prune(now + NOTIFICATION_TOAST_DURATION);
        assert_eq!(center.toasts.len(), 0);
    }

    #[test]
    fn contract_notification_center_clear_all_is_session_only() {
        let now = Instant::now();
        let mut center = NotificationCenter::default();
        center.push_critical(NotificationSource::Configuration, "failure", now, true);

        center.clear_all();

        assert_eq!(center.entries.len(), 0);
        assert_eq!(center.toasts.len(), 0);
    }

    #[test]
    fn contract_notification_entries_keep_optional_technical_detail() {
        let now = Instant::now();
        let mut center = NotificationCenter::default();
        center.push_critical_with_detail(
            NotificationSource::Configuration,
            "Settings need attention",
            "Could not write settings.json: access denied",
            now,
            true,
        );

        let entry = center.entries.front().unwrap();
        assert_eq!(entry.body, "Settings need attention");
        assert_eq!(
            entry.detail.as_deref(),
            Some("Could not write settings.json: access denied")
        );
    }
}
