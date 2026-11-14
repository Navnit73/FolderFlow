use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::engine::planner::{ExecutionPlan, PlanConflictKind, PlanExecutionReport, StaleReason};
use crate::logging::redact_path;
use crate::settings::{NotificationSettings, Settings};
use crate::utils::clock::Clock;

pub const MAX_NOTIFICATION_HISTORY: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    SuccessSummary,
    LargeBatchDone,
    Failure,
    PermissionIssue,
    DestinationUnavailable,
    DuplicatesFound,
    MonitoringStopped,
}

impl NotificationKind {
    pub fn severity(self) -> NotificationSeverity {
        match self {
            Self::SuccessSummary | Self::LargeBatchDone => NotificationSeverity::Success,
            Self::DuplicatesFound => NotificationSeverity::Info,
            Self::DestinationUnavailable | Self::MonitoringStopped => NotificationSeverity::Warning,
            Self::Failure | Self::PermissionIssue => NotificationSeverity::Error,
        }
    }

    fn batch_title(self) -> &'static str {
        match self {
            Self::SuccessSummary => "Organization complete",
            Self::LargeBatchDone => "Large batch complete",
            Self::Failure => "Organization issues",
            Self::PermissionIssue => "Permission required",
            Self::DestinationUnavailable => "Destination unavailable",
            Self::DuplicatesFound => "Duplicates detected",
            Self::MonitoringStopped => "Monitoring stopped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecord {
    pub id: String,
    pub kind: NotificationKind,
    pub severity: NotificationSeverity,
    pub title: String,
    pub body: String,
    pub count: u32,
    pub batch_count: u32,
    pub folder_id: Option<String>,
    #[specta(type = f64)]
    pub created_at_ms: i64,
    pub read: bool,
}

#[derive(Debug, Clone)]
pub struct NotificationInput {
    pub kind: NotificationKind,
    pub title: String,
    pub message: String,
    pub item_count: u32,
    pub folder_id: Option<String>,
    pub path_hint: Option<PathBuf>,
}

impl NotificationInput {
    pub fn success_summary(folder_id: Option<String>, files_moved: u32) -> Self {
        Self {
            kind: NotificationKind::SuccessSummary,
            title: "Organization complete".into(),
            message: format!(
                "Organized {} {}",
                files_moved,
                if files_moved == 1 { "file" } else { "files" }
            ),
            item_count: files_moved.max(1),
            folder_id,
            path_hint: None,
        }
    }

    pub fn large_batch_done(folder_id: Option<String>, files_moved: u32) -> Self {
        Self {
            kind: NotificationKind::LargeBatchDone,
            title: "Large batch complete".into(),
            message: format!("Finished organizing {files_moved} files."),
            item_count: files_moved.max(1),
            folder_id,
            path_hint: None,
        }
    }

    pub fn failure(
        folder_id: Option<String>,
        message: impl Into<String>,
        path_hint: Option<PathBuf>,
    ) -> Self {
        Self {
            kind: NotificationKind::Failure,
            title: "Organization failed".into(),
            message: message.into(),
            item_count: 1,
            folder_id,
            path_hint,
        }
    }

    pub fn permission_issue(
        folder_id: Option<String>,
        message: impl Into<String>,
        path_hint: Option<PathBuf>,
    ) -> Self {
        Self {
            kind: NotificationKind::PermissionIssue,
            title: "Folder permission required".into(),
            message: message.into(),
            item_count: 1,
            folder_id,
            path_hint,
        }
    }

    pub fn destination_unavailable(
        folder_id: Option<String>,
        message: impl Into<String>,
        path_hint: Option<PathBuf>,
    ) -> Self {
        Self {
            kind: NotificationKind::DestinationUnavailable,
            title: "Destination folder unavailable".into(),
            message: message.into(),
            item_count: 1,
            folder_id,
            path_hint,
        }
    }

    pub fn duplicates_found(groups: u32, extra_copies: u32) -> Self {
        Self {
            kind: NotificationKind::DuplicatesFound,
            title: "Duplicates found".into(),
            message: format!(
                "Found {groups} duplicate {} ({extra_copies} extra {}).",
                if groups == 1 { "group" } else { "groups" },
                if extra_copies == 1 { "copy" } else { "copies" },
            ),
            item_count: groups.max(1),
            folder_id: None,
            path_hint: None,
        }
    }

    pub fn monitoring_stopped(
        folder_id: Option<String>,
        message: impl Into<String>,
        path_hint: Option<PathBuf>,
    ) -> Self {
        Self {
            kind: NotificationKind::MonitoringStopped,
            title: "Monitoring stopped".into(),
            message: message.into(),
            item_count: 1,
            folder_id,
            path_hint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationConfig {
    pub master_enabled: bool,
    pub redact_paths: bool,
    pub settings: NotificationSettings,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self::from_app_settings(&Settings::default())
    }
}

impl NotificationConfig {
    pub fn from_app_settings(s: &Settings) -> Self {
        Self {
            master_enabled: s.general.show_notifications && s.notifications.enabled,
            redact_paths: s.privacy.redact_paths_in_logs,
            settings: s.notifications.clone(),
        }
    }

    pub fn is_enabled(&self, kind: NotificationKind) -> bool {
        if !self.master_enabled {
            return false;
        }
        let p = &self.settings.per_type;
        match kind {
            NotificationKind::SuccessSummary => p.success_summary,
            NotificationKind::LargeBatchDone => p.large_batch_done,
            NotificationKind::Failure => p.failure,
            NotificationKind::PermissionIssue => p.permission_issue,
            NotificationKind::DestinationUnavailable => p.destination_unavailable,
            NotificationKind::DuplicatesFound => p.duplicates_found,
            NotificationKind::MonitoringStopped => p.monitoring_stopped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyResult {
    QueuedForBatch,
    Delivered(NotificationRecord),
    SuppressedDisabled,
}

pub trait NotificationSink: Send + Sync {
    fn emit(&self, record: &NotificationRecord);
}

#[derive(Debug, Default)]
pub struct NoopNotificationSink;

impl NotificationSink for NoopNotificationSink {
    fn emit(&self, _record: &NotificationRecord) {}
}

#[derive(Debug, Default, Clone)]
pub struct RecordingNotificationSink {
    records: Arc<Mutex<Vec<NotificationRecord>>>,
}

impl RecordingNotificationSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn records(&self) -> Vec<NotificationRecord> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl NotificationSink for RecordingNotificationSink {
    fn emit(&self, record: &NotificationRecord) {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(record.clone());
    }
}

#[derive(Debug, Clone)]
struct PendingBucket {
    kind: NotificationKind,
    first_seen_ms: i64,
    first_title: String,
    last_message: String,
    item_count: u32,
    event_count: u32,
    folder_id: Option<String>,
    distinct_folders: u32,
}

impl PendingBucket {
    fn new(input: NotificationInput, message: String, now_ms: i64) -> Self {
        let distinct_folders = u32::from(input.folder_id.is_some());
        Self {
            kind: input.kind,
            first_seen_ms: now_ms,
            first_title: input.title,
            last_message: message,
            item_count: input.item_count.max(1),
            event_count: 1,
            folder_id: input.folder_id,
            distinct_folders,
        }
    }

    fn merge(&mut self, input: NotificationInput, message: String) {
        self.event_count = self.event_count.saturating_add(1);
        self.item_count = self.item_count.saturating_add(input.item_count.max(1));
        self.last_message = message;
        if let Some(fid) = input.folder_id {
            if self.folder_id.as_deref() != Some(fid.as_str()) {
                self.distinct_folders = self.distinct_folders.saturating_add(1);
                self.folder_id = None;
            }
        }
    }

    fn into_record(self, now_ms: i64) -> NotificationRecord {
        let (title, body) = if self.event_count <= 1 {
            (self.first_title, self.last_message)
        } else {
            let summary = match self.kind {
                NotificationKind::SuccessSummary => format!(
                    "Organized {} files across {} batches.",
                    self.item_count, self.event_count
                ),
                NotificationKind::LargeBatchDone => format!(
                    "Completed {} large batches ({} files total).",
                    self.event_count, self.item_count
                ),
                NotificationKind::Failure => format!(
                    "{} organization errors occurred — latest: {}",
                    self.event_count, self.last_message
                ),
                NotificationKind::PermissionIssue => format!(
                    "{} permission issues need attention — latest: {}",
                    self.event_count, self.last_message
                ),
                NotificationKind::DestinationUnavailable => format!(
                    "{} destination alerts — latest: {}",
                    self.event_count, self.last_message
                ),
                NotificationKind::DuplicatesFound => format!(
                    "Duplicate scans reported {} groups ({} scans).",
                    self.item_count, self.event_count
                ),
                NotificationKind::MonitoringStopped => format!(
                    "{} monitoring stop alerts — latest: {}",
                    self.event_count, self.last_message
                ),
            };
            (self.kind.batch_title().to_owned(), summary)
        };

        NotificationRecord {
            id: Uuid::now_v7().to_string(),
            kind: self.kind,
            severity: self.kind.severity(),
            title,
            body,
            count: self.item_count,
            batch_count: self.event_count,
            folder_id: self.folder_id,
            created_at_ms: now_ms,
            read: false,
        }
    }
}

struct NotificationState {
    config: NotificationConfig,
    pending: HashMap<NotificationKind, PendingBucket>,
    last_emitted_ms: HashMap<NotificationKind, i64>,
    history: VecDeque<NotificationRecord>,
}

/// Manages batched, throttled user notifications and keeps recent notification history.
#[derive(Clone)]
pub struct NotificationService {
    clock: Arc<dyn Clock>,
    sink: Arc<dyn NotificationSink>,
    state: Arc<Mutex<NotificationState>>,
}

impl NotificationService {
    pub fn new(
        clock: Arc<dyn Clock>,
        sink: Arc<dyn NotificationSink>,
        config: NotificationConfig,
    ) -> Self {
        Self {
            clock,
            sink,
            state: Arc::new(Mutex::new(NotificationState {
                config,
                pending: HashMap::new(),
                last_emitted_ms: HashMap::new(),
                history: VecDeque::with_capacity(MAX_NOTIFICATION_HISTORY),
            })),
        }
    }

    pub fn update_config(&self, config: NotificationConfig) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.config = config;
        let cfg = st.config.clone();
        st.pending.retain(|kind, _| cfg.is_enabled(*kind));
    }

    pub fn config(&self) -> NotificationConfig {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .config
            .clone()
    }

    /// Submits a notification event. Events inside the configured batch window or cooldown
    /// period are coalesced rather than emitted individually (FR-202).
    pub fn notify(&self, input: NotificationInput) -> NotifyResult {
        let now_ms = self.clock.now_ms();
        let mut emitted: Option<NotificationRecord> = None;

        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if !st.config.is_enabled(input.kind) {
                return NotifyResult::SuppressedDisabled;
            }
            let message = format_message(&input, st.config.redact_paths);
            let kind = input.kind;
            match st.pending.get_mut(&kind) {
                Some(bucket) => bucket.merge(input, message),
                None => {
                    st.pending
                        .insert(kind, PendingBucket::new(input, message, now_ms));
                }
            }

            if is_due(&st, kind, now_ms) {
                if let Some(bucket) = st.pending.remove(&kind) {
                    let rec = bucket.into_record(now_ms);
                    record_emitted(&mut st, rec.clone(), now_ms);
                    emitted = Some(rec);
                }
            }
        }

        match emitted {
            Some(rec) => {
                self.sink.emit(&rec);
                NotifyResult::Delivered(rec)
            }
            None => NotifyResult::QueuedForBatch,
        }
    }

    /// Emits all pending batches whose batch window and cooldown have elapsed.
    pub fn flush_due(&self) -> Vec<NotificationRecord> {
        let now_ms = self.clock.now_ms();
        let due_records = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let due_kinds: Vec<NotificationKind> = st
                .pending
                .keys()
                .copied()
                .filter(|k| st.config.is_enabled(*k) && is_due(&st, *k, now_ms))
                .collect();

            let mut out = Vec::with_capacity(due_kinds.len());
            for kind in due_kinds {
                if let Some(bucket) = st.pending.remove(&kind) {
                    let rec = bucket.into_record(now_ms);
                    record_emitted(&mut st, rec.clone(), now_ms);
                    out.push(rec);
                }
            }
            out
        };

        for rec in &due_records {
            self.sink.emit(rec);
        }
        due_records
    }

    /// Sends a test notification immediately if notifications are enabled overall.
    pub fn send_test(&self) -> NotifyResult {
        let now_ms = self.clock.now_ms();
        let rec = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if !st.config.master_enabled {
                return NotifyResult::SuppressedDisabled;
            }
            let rec = NotificationRecord {
                id: Uuid::now_v7().to_string(),
                kind: NotificationKind::SuccessSummary,
                severity: NotificationSeverity::Info,
                title: "FolderFlow notifications active".into(),
                body: format!(
                    "Notifications are batching over {}s windows with {}s cooldown.",
                    st.config.settings.batch_window_secs, st.config.settings.cooldown_secs
                ),
                count: 1,
                batch_count: 1,
                folder_id: None,
                created_at_ms: now_ms,
                read: false,
            };
            if st.history.len() >= MAX_NOTIFICATION_HISTORY {
                st.history.pop_front();
            }
            st.history.push_back(rec.clone());
            rec
        };
        self.sink.emit(&rec);
        NotifyResult::Delivered(rec)
    }

    pub fn list_recent(&self) -> Vec<NotificationRecord> {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.history.iter().rev().cloned().collect()
    }

    pub fn mark_read(&self, id: Option<&str>) -> u32 {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut updated = 0_u32;
        for rec in &mut st.history {
            let target = match id {
                Some(want) => rec.id == want,
                None => true,
            };
            if target && !rec.read {
                rec.read = true;
                updated += 1;
            }
        }
        updated
    }

    pub fn clear_history(&self) -> u32 {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let n = u32::try_from(st.history.len()).unwrap_or(u32::MAX);
        st.history.clear();
        n
    }

    pub fn pending_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .len()
    }

    /// Submits a summary and/or failure alert from an organization execution report (`FR-200`).
    pub fn notify_execution_outcome(
        &self,
        folder_id: Option<String>,
        completed_count: usize,
        failed_messages: &[String],
    ) {
        if completed_count > 0 {
            let count_u32 = u32::try_from(completed_count).unwrap_or(u32::MAX);
            let threshold = self.config().settings.large_batch_threshold.max(2);
            if count_u32 >= threshold {
                self.notify(NotificationInput::large_batch_done(
                    folder_id.clone(),
                    count_u32,
                ));
            } else {
                self.notify(NotificationInput::success_summary(
                    folder_id.clone(),
                    count_u32,
                ));
            }
        }
        if !failed_messages.is_empty() {
            let first = failed_messages
                .first()
                .map(String::as_str)
                .unwrap_or("Operation failed");
            self.notify(NotificationInput::failure(folder_id, first, None));
        }
    }

    /// Submits summary, blocked-root (`DestinationUnavailable` / `PermissionIssue`), and/or failure
    /// alerts for an executed [`ExecutionPlan`] (`FR-200`, `BUG-032`).
    pub fn notify_plan_outcome(
        &self,
        folder_id: Option<String>,
        plan: &ExecutionPlan,
        report: &PlanExecutionReport,
    ) {
        self.notify_execution_outcome(folder_id.clone(), report.completed.len(), &report.failed);

        let blocked_ids: HashSet<&str> = report
            .skipped_stale
            .iter()
            .filter(|s| s.reason == StaleReason::BlockedByConflict)
            .map(|s| s.item_id.as_str())
            .collect();
        if blocked_ids.is_empty() {
            return;
        }

        let mut seen_unavail: HashSet<String> = HashSet::new();
        let mut seen_perm: HashSet<String> = HashSet::new();
        for item in plan.items.iter() {
            if !blocked_ids.contains(item.id.as_str()) {
                continue;
            }
            let fid = folder_id.clone().or_else(|| Some(item.folder_id.clone()));
            let hint = Some(PathBuf::from(&item.proposed_destination));
            for c in &item.conflicts {
                match c.kind {
                    PlanConflictKind::DestinationUnavailable
                        if seen_unavail.insert(c.message.clone()) =>
                    {
                        self.notify(NotificationInput::destination_unavailable(
                            fid.clone(),
                            c.message.clone(),
                            hint.clone(),
                        ));
                    }
                    PlanConflictKind::PermissionDenied if seen_perm.insert(c.message.clone()) => {
                        self.notify(NotificationInput::permission_issue(
                            fid.clone(),
                            c.message.clone(),
                            hint.clone(),
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
}

fn format_message(input: &NotificationInput, redact_paths: bool) -> String {
    match &input.path_hint {
        Some(path) => {
            let safe = redact_path(path, redact_paths);
            if input.message.is_empty() {
                safe
            } else {
                format!("{} ({safe})", input.message)
            }
        }
        None => input.message.clone(),
    }
}

fn is_due(st: &NotificationState, kind: NotificationKind, now_ms: i64) -> bool {
    let Some(bucket) = st.pending.get(&kind) else {
        return false;
    };
    let batch_ms = i64::from(st.config.settings.batch_window_secs).saturating_mul(1000);
    let cooldown_ms = i64::from(st.config.settings.cooldown_secs).saturating_mul(1000);

    let batch_ready_at = bucket.first_seen_ms.saturating_add(batch_ms);
    let cooldown_ready_at = st
        .last_emitted_ms
        .get(&kind)
        .map(|&last| last.saturating_add(cooldown_ms))
        .unwrap_or(i64::MIN);

    now_ms >= batch_ready_at.max(cooldown_ready_at)
}

fn record_emitted(st: &mut NotificationState, rec: NotificationRecord, now_ms: i64) {
    st.last_emitted_ms.insert(rec.kind, now_ms);
    if st.history.len() >= MAX_NOTIFICATION_HISTORY {
        st.history.pop_front();
    }
    st.history.push_back(rec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::utils::clock::{Clock, FixedClock};
    use std::sync::atomic::{AtomicI64, Ordering};

    #[derive(Debug)]
    struct MutableClock {
        now: AtomicI64,
    }

    impl MutableClock {
        fn new(now_ms: i64) -> Self {
            Self {
                now: AtomicI64::new(now_ms),
            }
        }

        fn advance_ms(&self, delta: i64) {
            self.now.fetch_add(delta, Ordering::SeqCst);
        }
    }

    impl Clock for MutableClock {
        fn now_ms(&self) -> i64 {
            self.now.load(Ordering::SeqCst)
        }

        fn utc_offset_secs(&self, epoch_ms: i64) -> i32 {
            FixedClock::utc(epoch_ms).utc_offset_secs(epoch_ms)
        }
    }

    fn make_service(
        batch_secs: u32,
        cooldown_secs: u32,
    ) -> (
        Arc<MutableClock>,
        Arc<RecordingNotificationSink>,
        NotificationService,
    ) {
        let clock = Arc::new(MutableClock::new(1_000_000));
        let sink = Arc::new(RecordingNotificationSink::new());
        let mut settings = Settings::default();
        settings.notifications.batch_window_secs = batch_secs;
        settings.notifications.cooldown_secs = cooldown_secs;
        let svc = NotificationService::new(
            clock.clone(),
            sink.clone(),
            NotificationConfig::from_app_settings(&settings),
        );
        (clock, sink, svc)
    }

    #[test]
    fn r_notif_01_per_type_and_master_preferences_filter_events() {
        let (_clock, sink, svc) = make_service(0, 0);

        let mut cfg = svc.config();
        cfg.settings.per_type.failure = false;
        svc.update_config(cfg);

        assert_eq!(
            svc.notify(NotificationInput::failure(None, "disk full", None)),
            NotifyResult::SuppressedDisabled
        );
        assert!(sink.records().is_empty());

        assert!(matches!(
            svc.notify(NotificationInput::duplicates_found(2, 4)),
            NotifyResult::Delivered(_)
        ));
        assert_eq!(sink.records().len(), 1);

        // Disabling master toggle drops enabled event types too.
        let mut cfg = svc.config();
        cfg.master_enabled = false;
        svc.update_config(cfg);
        assert_eq!(
            svc.notify(NotificationInput::duplicates_found(1, 1)),
            NotifyResult::SuppressedDisabled
        );
        assert_eq!(sink.records().len(), 1);
    }

    #[test]
    fn r_notif_02_time_window_batching_collapses_burst_into_single_summary() {
        let (clock, sink, svc) = make_service(10, 30);

        // Simulate 20 individual files stabilizing within 3 seconds.
        for i in 0..20 {
            if i == 10 {
                clock.advance_ms(3_000);
            }
            let res = svc.notify(NotificationInput::success_summary(Some("f1".into()), 1));
            assert_eq!(res, NotifyResult::QueuedForBatch);
        }
        assert!(
            sink.records().is_empty(),
            "must never emit one notification per file during batch window"
        );

        // Before batch_window (10s) elapses: nothing flushes yet.
        clock.advance_ms(5_000); // t + 8s
        assert!(svc.flush_due().is_empty());

        // Past batch_window (10s): single summary emits with summed item count.
        clock.advance_ms(2_500); // t + 10.5s
        let flushed = svc.flush_due();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].count, 20);
        assert_eq!(flushed[0].batch_count, 20);
        assert!(flushed[0].body.contains("20 files"));
        assert_eq!(sink.records().len(), 1);
    }

    #[test]
    fn r_notif_03_cooldown_throttling_suppresses_rapid_repeats_and_recovers_after_window() {
        let (clock, sink, svc) = make_service(2, 30);

        // First alert waits 2s batch window and emits at t + 2s.
        svc.notify(NotificationInput::failure(None, "err 1", None));
        clock.advance_ms(2_000);
        assert_eq!(svc.flush_due().len(), 1);
        assert_eq!(sink.records().len(), 1);

        // Rapid burst within 30s cooldown: even past 2s batch window, cooldown blocks delivery.
        clock.advance_ms(5_000); // 5s since emission
        svc.notify(NotificationInput::failure(None, "err 2", None));
        svc.notify(NotificationInput::failure(None, "err 3", None));
        clock.advance_ms(10_000); // 15s since emission (< 30s cooldown)
        assert!(svc.flush_due().is_empty());
        assert_eq!(sink.records().len(), 1);

        // Independent event type (permission_issue) is NOT blocked by Failure's cooldown!
        svc.notify(NotificationInput::permission_issue(
            Some("f1".into()),
            "Access denied",
            None,
        ));
        clock.advance_ms(2_500); // 17.5s since Failure emit; PermissionIssue batch ready
        assert_eq!(svc.flush_due().len(), 1);
        assert_eq!(sink.records().len(), 2);

        // Advance past Failure's 30s cooldown: coalesced errors emit as 1 summary.
        clock.advance_ms(13_000); // 30.5s since Failure emit
        let after = svc.flush_due();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].kind, NotificationKind::Failure);
        assert_eq!(after[0].batch_count, 2);
        assert_eq!(sink.records().len(), 3);
    }

    #[test]
    fn r_notif_04_path_redaction_and_bounded_history_and_read_clear() {
        let (_clock, _sink, svc) = make_service(0, 0);
        let secret_path = PathBuf::from("/Users/alice/Private/tax-return-2025.pdf");

        let NotifyResult::Delivered(rec) = svc.notify(NotificationInput::permission_issue(
            Some("f1".into()),
            "Permission denied",
            Some(secret_path),
        )) else {
            panic!("expected immediate delivery with 0s window/cooldown");
        };
        assert!(
            !rec.body.contains("tax-return-2025") && !rec.body.contains("alice"),
            "body must redact filename/path when redact_paths=true: {}",
            rec.body
        );
        assert!(rec.body.contains("[path:"));

        // Flood > MAX_NOTIFICATION_HISTORY events; history remains capped cleanly.
        for i in 0..(MAX_NOTIFICATION_HISTORY + 25) {
            svc.notify(NotificationInput::success_summary(None, (i as u32) + 1));
        }
        let history = svc.list_recent();
        assert_eq!(history.len(), MAX_NOTIFICATION_HISTORY);

        let marked = svc.mark_read(Some(&history[0].id));
        assert_eq!(marked, 1);
        assert!(svc.mark_read(None) >= 1);
        assert_eq!(svc.clear_history() as usize, MAX_NOTIFICATION_HISTORY);
        assert!(svc.list_recent().is_empty());
    }

    #[test]
    fn bug_032_plan_outcome_emits_destination_unavailable_and_dedups_per_root() {
        use crate::engine::planner::{
            FileIdentitySnapshot, PlanConflict, PlanItem, PlanItemStatus, PlanOpKind, PlanSummary,
            SkippedPlanItem,
        };

        let (_clock, sink, svc) = make_service(0, 0);
        let mk_item = |id: &str, kind: PlanConflictKind, msg: &str| PlanItem {
            id: id.into(),
            folder_id: "f-src".into(),
            source_path: format!("/Users/alice/Downloads/{id}.pdf"),
            current_location: "/Users/alice/Downloads".into(),
            original_name: format!("{id}.pdf"),
            proposed_destination: format!("/Volumes/Ext/{id}.pdf"),
            proposed_name: format!("{id}.pdf"),
            size: 100,
            category: "documents".into(),
            op: PlanOpKind::Move,
            rule_id: "r1".into(),
            rule_name: "Rule".into(),
            matched_conditions: vec![],
            planned_ops: vec![],
            conflicts: vec![PlanConflict {
                kind,
                message: msg.into(),
            }],
            status: PlanItemStatus::Blocked,
            cross_volume: false,
            identity: FileIdentitySnapshot {
                dev: 1,
                ino: 1,
                size: 100,
                mtime_ns: 1,
            },
        };
        let plan = ExecutionPlan {
            summary: PlanSummary {
                plan_id: "p1".into(),
                schema_version: 1,
                created_at: 1_000,
                files_evaluated: 3,
                files_matched: 3,
                files_with_conflicts: 3,
                total_bytes_affected: 300,
                cross_volume_bytes: 0,
                by_category: vec![],
                by_rule: vec![],
            },
            items: vec![
                mk_item(
                    "i1",
                    PlanConflictKind::DestinationUnavailable,
                    "Destination volume or root folder is unavailable: /Volumes/Ext",
                ),
                mk_item(
                    "i2",
                    PlanConflictKind::DestinationUnavailable,
                    "Destination volume or root folder is unavailable: /Volumes/Ext",
                ),
                mk_item(
                    "i3",
                    PlanConflictKind::PermissionDenied,
                    "Permission denied accessing destination root: /Volumes/Locked",
                ),
            ]
            .into(),
        };
        let report = PlanExecutionReport {
            plan_id: "p1".into(),
            batch_id: "b1".into(),
            completed: vec![],
            queued_for_review: 0,
            skipped_stale: vec![
                SkippedPlanItem {
                    item_id: "i1".into(),
                    source_path: "/Users/alice/Downloads/i1.pdf".into(),
                    reason: StaleReason::BlockedByConflict,
                    message: "Destination unavailable".into(),
                },
                SkippedPlanItem {
                    item_id: "i2".into(),
                    source_path: "/Users/alice/Downloads/i2.pdf".into(),
                    reason: StaleReason::BlockedByConflict,
                    message: "Destination unavailable".into(),
                },
                SkippedPlanItem {
                    item_id: "i3".into(),
                    source_path: "/Users/alice/Downloads/i3.pdf".into(),
                    reason: StaleReason::BlockedByConflict,
                    message: "Permission denied".into(),
                },
            ],
            failed: vec![],
        };

        svc.notify_plan_outcome(Some("f-src".into()), &plan, &report);

        let delivered = sink.records();
        let unavail_count = delivered
            .iter()
            .filter(|r| r.kind == NotificationKind::DestinationUnavailable)
            .count();
        let perm_count = delivered
            .iter()
            .filter(|r| r.kind == NotificationKind::PermissionIssue)
            .count();
        assert_eq!(
            unavail_count, 1,
            "identical DestinationUnavailable root alerts within one plan should deduplicate"
        );
        assert_eq!(perm_count, 1);
    }
}
