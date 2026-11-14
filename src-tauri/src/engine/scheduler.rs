//! Rule & folder scheduler (`P5-T05`; `FR-130..134`).
//!
//! Computes DST-aware `next_run_at` timestamps via [`Clock`], enforces quiet hours
//! and weekday filters, honors `CatchUpPolicy::{Skip, RunOnce}` (never "run all"),
//! and maintains a per-scope mutex lock so no two runs for the same scope ever overlap.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::database::repo_schedules::{
    self, CatchUpPolicy, QuietHours, Schedule, ScheduleConfig, ScheduleInput, ScheduleType, Weekday,
};
use crate::database::{repo_files, repo_folders, repo_rules, Db};
use crate::duplicates::cache::hydrate_candidates_for_rules;
use crate::engine::classifier::{Classifier, HEADER_BYTES};
use crate::engine::conflicts::ConflictPolicy;
use crate::engine::fs_view::{EntryKind, ReadOnlyFs, RealFs};
use crate::engine::operations::{BatchTrigger, OpKind};
use crate::engine::organizer::Organizer;
use crate::engine::paths::{nfc, PathAuthority};
use crate::engine::planner::{
    build_execution_plan, collect_candidates_from_spec, execute_plan, PlanContext, PlanEnv,
    PlanExecutionReport, PlanScope,
};
use crate::engine::reconcile::FolderScanSpec;
use crate::engine::scanner::ScannedFile;
use crate::error::{AppError, AppResult};
use crate::notifications::NotificationService;
use crate::rules::models::scope_rules;
use crate::settings::SettingsService;
use crate::utils::clock::Clock;

/// Grace window beyond `next_run_at` before a delayed run is treated as missed (5 minutes).
pub const MISSED_RUN_GRACE_MS: i64 = 5 * 60 * 1000;

/// Minimum allowed custom interval in seconds (60s).
pub const MIN_INTERVAL_SECS: u32 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DueDecision {
    NotDue,
    DeferredQuietHours {
        resume_at_ms: i64,
    },
    MissedSkipped {
        next_run_at: Option<i64>,
    },
    RunNow {
        is_catch_up: bool,
        next_run_at: Option<i64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleRunStatus {
    Executed,
    NoMatchingFiles,
    SkippedOverlap,
    SkippedQuietHours,
    SkippedMissed,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRunOutcome {
    pub schedule_id: String,
    pub status: ScheduleRunStatus,
    pub report: Option<PlanExecutionReport>,
}

/// Parses `"HH:MM"` (or `"H:MM"`) into a `NaiveTime`.
pub fn parse_hhmm(raw: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(raw.trim(), "%H:%M").map_err(|_| {
        AppError::validation(
            "time",
            format!("Invalid time \"{raw}\"; expected HH:MM (24-hour)."),
        )
    })
}

/// Validates and normalizes a [`ScheduleInput`], rejecting missing scope or invalid times.
pub fn validate_schedule_input(input: &ScheduleInput) -> AppResult<()> {
    let has_rule = input
        .rule_id
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_folder = input
        .folder_id
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if !has_rule && !has_folder {
        return Err(AppError::validation(
            "scope",
            "Schedule must target a monitored folder or a rule.",
        ));
    }

    match input.schedule_type {
        ScheduleType::Interval => {
            let secs = input.config.interval_secs.ok_or_else(|| {
                AppError::validation("intervalSecs", "Interval duration is required.")
            })?;
            if secs < MIN_INTERVAL_SECS {
                return Err(AppError::validation(
                    "intervalSecs",
                    format!("Interval must be at least {MIN_INTERVAL_SECS} seconds."),
                ));
            }
        }
        ScheduleType::Daily => {
            let raw = input.config.time.as_deref().ok_or_else(|| {
                AppError::validation(
                    "time",
                    "Time of day (HH:MM) is required for daily schedules.",
                )
            })?;
            let _ = parse_hhmm(raw)?;
        }
        ScheduleType::Weekly => {
            let raw = input.config.time.as_deref().ok_or_else(|| {
                AppError::validation(
                    "time",
                    "Time of day (HH:MM) is required for weekly schedules.",
                )
            })?;
            let _ = parse_hhmm(raw)?;
            if input.config.weekdays.is_empty() {
                return Err(AppError::validation(
                    "weekdays",
                    "Select at least one day of the week.",
                ));
            }
        }
        ScheduleType::Realtime | ScheduleType::OnLaunch | ScheduleType::Manual => {}
    }

    if let Some(qh) = &input.config.quiet_hours {
        let _ = parse_hhmm(&qh.start)?;
        let _ = parse_hhmm(&qh.end)?;
    }

    Ok(())
}

/// Resolves local `(date, time)` to the earliest corresponding UTC `epoch_ms` using `clock`,
/// handling both DST fall-back folds (picks first occurrence) and spring-forward gaps (lands on end of gap).
fn resolve_local_datetime(date: NaiveDate, time: NaiveTime, clock: &dyn Clock) -> i64 {
    let naive_dt = date.and_time(time);
    let naive_ms = naive_dt.and_utc().timestamp_millis();

    let mut offsets = vec![
        clock.utc_offset_secs(naive_ms - 86_400_000),
        clock.utc_offset_secs(naive_ms - 43_200_000),
        clock.utc_offset_secs(naive_ms),
        clock.utc_offset_secs(naive_ms + 43_200_000),
        clock.utc_offset_secs(naive_ms + 86_400_000),
    ];
    offsets.sort_unstable_by(|a, b| b.cmp(a)); // largest East offset first -> earliest UTC first
    offsets.dedup();

    let mut cands: Vec<i64> = offsets
        .iter()
        .map(|&off| naive_ms - i64::from(off) * 1000)
        .collect();
    cands.sort_unstable();

    for &cand in &cands {
        let local = clock.to_local(cand);
        if local.date_naive() == date && local.time() == time {
            return cand;
        }
    }

    // Spring-forward gap: scan in 1-minute increments across the gap until local wall clock reaches >= `(date, time)`.
    let start_scan = cands
        .first()
        .copied()
        .unwrap_or(naive_ms)
        .saturating_sub(2 * 3_600_000);
    for step in 0..=480 {
        let probe = start_scan.saturating_add(step * 60_000);
        let local = clock.to_local(probe);
        if (local.date_naive(), local.time()) >= (date, time) {
            return probe;
        }
    }

    cands.first().copied().unwrap_or(naive_ms)
}

/// Returns `true` if `epoch_ms` falls inside `quiet_hours` in the local time zone defined by `clock`.
pub fn is_in_quiet_hours(epoch_ms: i64, quiet_hours: &QuietHours, clock: &dyn Clock) -> bool {
    let Ok(start) = parse_hhmm(&quiet_hours.start) else {
        return false;
    };
    let Ok(end) = parse_hhmm(&quiet_hours.end) else {
        return false;
    };
    if start == end {
        return false;
    }
    let t = clock.to_local(epoch_ms).time();
    if start < end {
        t >= start && t < end
    } else {
        t >= start || t < end
    }
}

/// Computes the next UTC millisecond timestamp when `quiet_hours` ends after `epoch_ms`.
pub fn quiet_hours_end_after(epoch_ms: i64, quiet_hours: &QuietHours, clock: &dyn Clock) -> i64 {
    let Ok(start) = parse_hhmm(&quiet_hours.start) else {
        return epoch_ms;
    };
    let Ok(end) = parse_hhmm(&quiet_hours.end) else {
        return epoch_ms;
    };
    if !is_in_quiet_hours(epoch_ms, quiet_hours, clock) {
        return epoch_ms;
    }
    let local = clock.to_local(epoch_ms);
    let end_date = if start > end && local.time() >= start {
        local.date_naive() + Duration::days(1)
    } else {
        local.date_naive()
    };
    resolve_local_datetime(end_date, end, clock).max(epoch_ms.saturating_add(1))
}

/// Computes the next scheduled UTC millisecond timestamp strictly `> after_ms`.
/// Returns `None` for `Realtime`, `OnLaunch`, and `Manual`.
pub fn compute_next_run_at(
    schedule_type: ScheduleType,
    config: &ScheduleConfig,
    after_ms: i64,
    clock: &dyn Clock,
) -> AppResult<Option<i64>> {
    match schedule_type {
        ScheduleType::Realtime | ScheduleType::OnLaunch | ScheduleType::Manual => Ok(None),
        ScheduleType::Interval => {
            let secs = config.interval_secs.unwrap_or(900).max(MIN_INTERVAL_SECS);
            let mut cand = after_ms.saturating_add(i64::from(secs) * 1000);
            if let Some(qh) = &config.quiet_hours {
                if is_in_quiet_hours(cand, qh, clock) {
                    cand = quiet_hours_end_after(cand, qh, clock);
                }
            }
            Ok(Some(cand))
        }
        ScheduleType::Daily => {
            let target = parse_hhmm(config.time.as_deref().unwrap_or("09:00"))?;
            let start_date = clock.local_date(after_ms);
            for offset in 0..=400 {
                let d = start_date + Duration::days(offset);
                let mut cand = resolve_local_datetime(d, target, clock);
                if cand <= after_ms {
                    continue;
                }
                if let Some(qh) = &config.quiet_hours {
                    if is_in_quiet_hours(cand, qh, clock) {
                        cand = quiet_hours_end_after(cand, qh, clock);
                        if cand <= after_ms {
                            continue;
                        }
                    }
                }
                return Ok(Some(cand));
            }
            Ok(None)
        }
        ScheduleType::Weekly => {
            let target = parse_hhmm(config.time.as_deref().unwrap_or("09:00"))?;
            let allowed: HashSet<chrono::Weekday> = if config.weekdays.is_empty() {
                HashSet::from([clock.local_date(after_ms).weekday()])
            } else {
                config
                    .weekdays
                    .iter()
                    .map(|w| Weekday::to_chrono(*w))
                    .collect()
            };
            let start_date = clock.local_date(after_ms);
            for offset in 0..=400 {
                let d = start_date + Duration::days(offset);
                if !allowed.contains(&d.weekday()) {
                    continue;
                }
                let mut cand = resolve_local_datetime(d, target, clock);
                if cand <= after_ms {
                    continue;
                }
                if let Some(qh) = &config.quiet_hours {
                    if is_in_quiet_hours(cand, qh, clock) {
                        let shifted = quiet_hours_end_after(cand, qh, clock);
                        if shifted <= after_ms
                            || !allowed.contains(&clock.local_date(shifted).weekday())
                        {
                            continue;
                        }
                        cand = shifted;
                    }
                }
                return Ok(Some(cand));
            }
            Ok(None)
        }
    }
}

/// Decides what action to take for `schedule` at `now_ms`, applying quiet hours and catch-up policy (`FR-133`).
pub fn evaluate_due_schedule(
    schedule: &Schedule,
    now_ms: i64,
    clock: &dyn Clock,
) -> AppResult<DueDecision> {
    if !schedule.enabled {
        return Ok(DueDecision::NotDue);
    }
    let Some(due_at) = schedule.next_run_at else {
        return Ok(DueDecision::NotDue);
    };
    if due_at > now_ms {
        return Ok(DueDecision::NotDue);
    }
    if let Some(qh) = &schedule.config.quiet_hours {
        if is_in_quiet_hours(now_ms, qh, clock) {
            return Ok(DueDecision::DeferredQuietHours {
                resume_at_ms: quiet_hours_end_after(now_ms, qh, clock),
            });
        }
    }

    let next_future = compute_next_run_at(schedule.schedule_type, &schedule.config, now_ms, clock)?;
    let late_ms = now_ms.saturating_sub(due_at);
    let is_missed = late_ms > MISSED_RUN_GRACE_MS;

    if is_missed {
        match schedule.config.catch_up {
            CatchUpPolicy::Skip => Ok(DueDecision::MissedSkipped {
                next_run_at: next_future,
            }),
            CatchUpPolicy::RunOnce => Ok(DueDecision::RunNow {
                is_catch_up: true,
                next_run_at: next_future,
            }),
        }
    } else {
        Ok(DueDecision::RunNow {
            is_catch_up: false,
            next_run_at: next_future,
        })
    }
}

/// Canonical lock key identifying the execution scope (`folder` and/or `rule`) so
/// overlapping runs for the same scope are prevented (`FR-134`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeKey {
    pub folder_id: Option<String>,
    pub rule_id: Option<String>,
}

impl ScopeKey {
    pub fn from_schedule(s: &Schedule) -> Self {
        Self {
            folder_id: s.folder_id.clone(),
            rule_id: s.rule_id.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SchedulerService {
    db: Db,
    organizer: Organizer,
    classifier: Arc<RwLock<Arc<Classifier>>>,
    authority: Arc<RwLock<PathAuthority>>,
    clock: Arc<dyn Clock>,
    scope_locks: Arc<std::sync::Mutex<HashMap<ScopeKey, Arc<Mutex<()>>>>>,
    conflict_policy: Arc<RwLock<ConflictPolicy>>,
    notifier: Arc<RwLock<Option<Arc<NotificationService>>>>,
}

impl SchedulerService {
    pub fn new(
        db: Db,
        organizer: Organizer,
        classifier: Arc<Classifier>,
        authority: PathAuthority,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            db,
            organizer,
            classifier: Arc::new(RwLock::new(classifier)),
            authority: Arc::new(RwLock::new(authority)),
            clock,
            scope_locks: Arc::new(std::sync::Mutex::new(HashMap::new())),
            conflict_policy: Arc::new(RwLock::new(ConflictPolicy::AutoRename)),
            notifier: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_authority(&self, authority: PathAuthority) {
        let mut guard = self.authority.write().unwrap_or_else(|e| e.into_inner());
        *guard = authority;
    }

    pub fn set_classifier(&self, classifier: Arc<Classifier>) {
        let mut guard = self.classifier.write().unwrap_or_else(|e| e.into_inner());
        *guard = classifier;
    }

    pub fn set_conflict_policy(&self, policy: ConflictPolicy) {
        let mut guard = self
            .conflict_policy
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *guard = policy;
    }

    pub fn set_notifier(&self, notifier: Arc<NotificationService>) {
        let mut guard = self.notifier.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(notifier);
    }

    fn current_classifier(&self) -> Arc<Classifier> {
        self.classifier
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn current_conflict_policy(&self) -> ConflictPolicy {
        *self
            .conflict_policy
            .read()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn current_notifier(&self) -> Option<Arc<NotificationService>> {
        self.notifier
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn scope_lock(&self, key: &ScopeKey) -> Arc<Mutex<()>> {
        let mut guard = self.scope_locks.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn list(&self) -> AppResult<Vec<Schedule>> {
        repo_schedules::list(&self.db).await
    }

    pub async fn upsert(&self, mut input: ScheduleInput) -> AppResult<Schedule> {
        validate_schedule_input(&input)?;
        input.rule_id = input
            .rule_id
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        input.folder_id = input
            .folder_id
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());

        let existing = if let Some(id) = &input.id {
            repo_schedules::get(&self.db, id).await?
        } else {
            None
        };
        let id = existing
            .as_ref()
            .map(|s| s.id.clone())
            .or(input.id.clone())
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        let last_run_at = existing.and_then(|s| s.last_run_at);

        let now_ms = self.clock.now_ms();
        let next_run_at = if input.enabled {
            compute_next_run_at(
                input.schedule_type,
                &input.config,
                now_ms,
                self.clock.as_ref(),
            )?
        } else {
            None
        };

        let sched = Schedule {
            id,
            rule_id: input.rule_id,
            folder_id: input.folder_id,
            schedule_type: input.schedule_type,
            config: input.config,
            enabled: input.enabled,
            last_run_at,
            next_run_at,
        };
        repo_schedules::upsert(&self.db, &sched).await?;
        Ok(sched)
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<Schedule> {
        let mut sched = repo_schedules::get(&self.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Schedule".into()))?;
        let next_run_at = if enabled {
            compute_next_run_at(
                sched.schedule_type,
                &sched.config,
                self.clock.now_ms(),
                self.clock.as_ref(),
            )?
        } else {
            None
        };
        repo_schedules::set_enabled(&self.db, id, enabled, next_run_at).await?;
        sched.enabled = enabled;
        sched.next_run_at = next_run_at;
        Ok(sched)
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        if !repo_schedules::delete(&self.db, id).await? {
            return Err(AppError::NotFound("Schedule".into()));
        }
        Ok(())
    }

    /// Executes one schedule immediately, respecting the per-scope non-overlapping lock (`FR-134`).
    pub async fn run_now(&self, id: &str) -> AppResult<ScheduleRunOutcome> {
        let sched = repo_schedules::get(&self.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Schedule".into()))?;
        let now_ms = self.clock.now_ms();
        let next_run_at = if sched.enabled {
            compute_next_run_at(
                sched.schedule_type,
                &sched.config,
                now_ms,
                self.clock.as_ref(),
            )?
        } else {
            sched.next_run_at
        };
        self.execute_schedule(&sched, next_run_at).await
    }

    /// Executes all enabled `OnLaunch` schedules once (`FR-130`).
    pub async fn run_on_launch(&self) -> AppResult<Vec<ScheduleRunOutcome>> {
        let all = repo_schedules::list(&self.db).await?;
        let mut out = Vec::new();
        for sched in all {
            if !sched.enabled || sched.schedule_type != ScheduleType::OnLaunch {
                continue;
            }
            if let Some(qh) = &sched.config.quiet_hours {
                if is_in_quiet_hours(self.clock.now_ms(), qh, self.clock.as_ref()) {
                    out.push(ScheduleRunOutcome {
                        schedule_id: sched.id,
                        status: ScheduleRunStatus::SkippedQuietHours,
                        report: None,
                    });
                    continue;
                }
            }
            out.push(self.execute_schedule(&sched, None).await?);
        }
        Ok(out)
    }

    /// Evaluates all enabled schedules at current clock time and executes any that are due.
    pub async fn tick_once(&self) -> AppResult<Vec<ScheduleRunOutcome>> {
        let now_ms = self.clock.now_ms();
        let all = repo_schedules::list(&self.db).await?;
        let mut out = Vec::new();

        for sched in all {
            if !sched.enabled {
                continue;
            }
            match evaluate_due_schedule(&sched, now_ms, self.clock.as_ref())? {
                DueDecision::NotDue => {}
                DueDecision::DeferredQuietHours { resume_at_ms } => {
                    let _ = repo_schedules::record_run(
                        &self.db,
                        &sched.id,
                        sched.last_run_at,
                        Some(resume_at_ms),
                    )
                    .await?;
                    out.push(ScheduleRunOutcome {
                        schedule_id: sched.id,
                        status: ScheduleRunStatus::SkippedQuietHours,
                        report: None,
                    });
                }
                DueDecision::MissedSkipped { next_run_at } => {
                    let _ = repo_schedules::record_run(
                        &self.db,
                        &sched.id,
                        sched.last_run_at,
                        next_run_at,
                    )
                    .await?;
                    out.push(ScheduleRunOutcome {
                        schedule_id: sched.id,
                        status: ScheduleRunStatus::SkippedMissed,
                        report: None,
                    });
                }
                DueDecision::RunNow { next_run_at, .. } => {
                    out.push(self.execute_schedule(&sched, next_run_at).await?);
                }
            }
        }

        Ok(out)
    }

    async fn execute_schedule(
        &self,
        sched: &Schedule,
        next_run_at: Option<i64>,
    ) -> AppResult<ScheduleRunOutcome> {
        let lock = self.scope_lock(&ScopeKey::from_schedule(sched));
        let Ok(_guard) = lock.try_lock() else {
            return Ok(ScheduleRunOutcome {
                schedule_id: sched.id.clone(),
                status: ScheduleRunStatus::SkippedOverlap,
                report: None,
            });
        };

        let all_rules = repo_rules::list(&self.db).await?;
        let target_rules: Vec<_> = if let Some(rid) = &sched.rule_id {
            let Some(rule) = all_rules.into_iter().find(|r| &r.id == rid) else {
                return Ok(ScheduleRunOutcome {
                    schedule_id: sched.id.clone(),
                    status: ScheduleRunStatus::Disabled,
                    report: None,
                });
            };
            if !rule.enabled {
                return Ok(ScheduleRunOutcome {
                    schedule_id: sched.id.clone(),
                    status: ScheduleRunStatus::Disabled,
                    report: None,
                });
            }
            vec![rule]
        } else {
            all_rules.into_iter().filter(|r| r.enabled).collect()
        };

        if target_rules.is_empty() {
            let now_ms = self.clock.now_ms();
            let _ =
                repo_schedules::record_run(&self.db, &sched.id, Some(now_ms), next_run_at).await?;
            return Ok(ScheduleRunOutcome {
                schedule_id: sched.id.clone(),
                status: ScheduleRunStatus::NoMatchingFiles,
                report: None,
            });
        }

        let target_folders = if let Some(fid) = &sched.folder_id {
            match repo_folders::get(&self.db, fid).await? {
                Some(f) if f.enabled => vec![f],
                _ => Vec::new(),
            }
        } else {
            let allowed_ids: Option<HashSet<&str>> = target_rules
                .first()
                .filter(|r| sched.rule_id.is_some() && !r.source_folders.is_empty())
                .map(|r| r.source_folders.iter().map(String::as_str).collect());
            repo_folders::list(&self.db)
                .await?
                .into_iter()
                .filter(|f| {
                    f.enabled
                        && allowed_ids
                            .as_ref()
                            .is_none_or(|ids| ids.contains(f.id.as_str()))
                })
                .collect()
        };

        let settings = SettingsService::new(self.db.clone()).get().await?;
        let monitoring = settings.monitoring;
        let hash_on_scan = settings.performance.hash_on_scan;
        let classifier = self.current_classifier();
        let classifier_bg = classifier.clone();
        let mut folder_specs = Vec::with_capacity(target_folders.len());
        for folder in &target_folders {
            let spec = FolderScanSpec::load_for_folder(&self.db, folder, &monitoring).await?;
            folder_specs.push((folder.name.clone(), spec));
        }
        let mut candidates = tokio::task::spawn_blocking(move || {
            let mut candidates = Vec::new();
            for (folder_name, spec) in folder_specs {
                let mut cands =
                    collect_candidates_from_spec(&spec, &folder_name, &classifier_bg, None);
                candidates.append(&mut cands);
            }
            candidates
        })
        .await
        .map_err(|e| AppError::Internal(format!("scheduler scan task failed: {e}")))?;

        let scoped_rules = scope_rules(&target_rules, sched.rule_id.as_deref());
        hydrate_candidates_for_rules(&self.db, &mut candidates, &scoped_rules, hash_on_scan)
            .await?;

        let authority = self
            .authority
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let conflict_policy = self.current_conflict_policy();

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &authority,
                classifier: &classifier,
                clock: self.clock.as_ref(),
            },
            &target_rules,
            &candidates,
            &PlanScope {
                folder_id: sched.folder_id.clone(),
                rule_id: sched.rule_id.clone(),
                file_paths: None,
            },
            &PlanEnv::default(),
        );

        let now_ms = self.clock.now_ms();
        if plan.items.is_empty() {
            let _ =
                repo_schedules::record_run(&self.db, &sched.id, Some(now_ms), next_run_at).await?;
            return Ok(ScheduleRunOutcome {
                schedule_id: sched.id.clone(),
                status: ScheduleRunStatus::NoMatchingFiles,
                report: None,
            });
        }

        let report = execute_plan(
            &plan,
            None,
            conflict_policy,
            &self.organizer,
            &authority,
            BatchTrigger::Schedule,
        )
        .await?;

        if let Some(notifier) = self.current_notifier() {
            notifier.notify_plan_outcome(sched.folder_id.clone(), &plan, &report);
        }

        // Sync `file_records` for executed moves/renames inside monitored folders.
        for op in &report.completed {
            if matches!(op.operation_type, OpKind::Move | OpKind::Rename) {
                let _ = repo_files::delete_path(&self.db, &op.source_path).await?;
            }
            let dst_path = PathBuf::from(&op.destination_path);
            let dst_nfc = nfc(&dst_path);
            if let Some(owner) = target_folders
                .iter()
                .find(|f| dst_nfc.starts_with(nfc(PathBuf::from(&f.path).as_path())))
            {
                if let Ok(meta) = RealFs.stat(&dst_path) {
                    let is_pkg = meta.kind == EntryKind::Package;
                    let header = if is_pkg {
                        None
                    } else {
                        RealFs.read_header(&dst_path, HEADER_BYTES).ok()
                    };
                    let cls = classifier.classify(&meta.name, header.as_deref(), is_pkg);
                    let new_sf = ScannedFile {
                        facts: meta,
                        category: cls.category,
                        mime: cls.mime,
                        confidence: cls.confidence,
                    };
                    let _ = repo_files::upsert_batch(
                        &self.db,
                        &owner.id,
                        std::slice::from_ref(&new_sf),
                        now_ms,
                    )
                    .await?;
                }
            }
        }

        let finished_ms = self.clock.now_ms();
        let _ =
            repo_schedules::record_run(&self.db, &sched.id, Some(finished_ms), next_run_at).await?;

        Ok(ScheduleRunOutcome {
            schedule_id: sched.id.clone(),
            status: ScheduleRunStatus::Executed,
            report: Some(report),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use chrono::{Datelike, NaiveDate, TimeZone, Utc};

    use super::*;
    use crate::categories::builtin_categories;
    use crate::database::{self, repo_folders, repo_rules};
    use crate::engine::operations::Ledger;
    use crate::engine::paths::ApprovedRoot;
    use crate::engine::paths::RootKind;
    use crate::engine::safe_fs::SafeFs;
    use crate::rules::models::{
        Action, ConditionNode, DateSource, ExecutionMode, Rule, RuleOrigin,
    };
    use crate::test_support::{tempdir, write_file};
    use crate::utils::clock::FixedClock;

    fn utc_ms(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, hh, mm, 0)
            .single()
            .expect("valid utc")
            .timestamp_millis()
    }

    async fn setup_scheduler_env(
        root: &Path,
        clock: Arc<dyn Clock>,
    ) -> (SchedulerService, Db, String, Rule) {
        let db = database::open_in_memory().await.expect("db");
        let folder_id = "folder-sched".to_owned();
        repo_folders::insert(&db, &folder_id, "Downloads", &root.to_string_lossy(), true)
            .await
            .expect("insert folder");

        let rule = Rule {
            id: "rule-sched".into(),
            name: "Sort Docs".into(),
            description: None,
            enabled: true,
            priority: 1,
            source_folders: Vec::new(),
            conditions: ConditionNode::All {
                children: Vec::new(),
            },
            actions: vec![Action::Move {
                destination: root.join("Organized").to_string_lossy().into_owned(),
            }],
            date_source: DateSource::Modified,
            execution_mode: ExecutionMode::FirstMatch,
            origin: RuleOrigin::User,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_000_000,
        };
        repo_rules::insert(&db, &rule).await.expect("insert rule");

        let ledger = Ledger::new(db.clone());
        let fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let org = Organizer::new(ledger, fs, clock.clone());
        let cls = Arc::new(Classifier::new(builtin_categories()));
        let auth = PathAuthority::new(vec![ApprovedRoot {
            path: root.to_path_buf(),
            kind: RootKind::Monitored,
        }]);
        let svc = SchedulerService::new(db.clone(), org, cls, auth, clock);
        (svc, db, folder_id, rule)
    }

    #[tokio::test]
    async fn r_sched_01_sqlite_crud_enforces_scope_check_and_cascade() {
        let (_d, root) = tempdir();
        let clock = Arc::new(FixedClock::utc(utc_ms(2026, 5, 1, 10, 0)));
        let (svc, db, folder_id, rule) = setup_scheduler_env(&root, clock).await;

        // Missing both rule_id and folder_id must fail validation.
        let invalid = ScheduleInput {
            id: None,
            rule_id: None,
            folder_id: None,
            schedule_type: ScheduleType::Daily,
            config: ScheduleConfig {
                time: Some("09:00".into()),
                ..Default::default()
            },
            enabled: true,
        };
        assert!(svc.upsert(invalid).await.is_err());

        // Create schedule scoped to `rule_id`.
        let created = svc
            .upsert(ScheduleInput {
                id: None,
                rule_id: Some(rule.id.clone()),
                folder_id: None,
                schedule_type: ScheduleType::Interval,
                config: ScheduleConfig {
                    interval_secs: Some(900),
                    ..Default::default()
                },
                enabled: true,
            })
            .await
            .expect("create");
        assert!(created.next_run_at.is_some());
        assert_eq!(svc.list().await.expect("list").len(), 1);

        // Disabling clears active execution and persists.
        let toggled = svc.set_enabled(&created.id, false).await.expect("disable");
        assert!(!toggled.enabled);

        // Deleting the referenced rule cascades and deletes the schedule.
        repo_rules::delete(&db, &rule.id).await.expect("del rule");
        assert!(svc.list().await.expect("list after cascade").is_empty());

        // Folder-scoped schedule also cascades when folder deleted.
        let f_sched = svc
            .upsert(ScheduleInput {
                id: None,
                rule_id: None,
                folder_id: Some(folder_id.clone()),
                schedule_type: ScheduleType::Daily,
                config: ScheduleConfig {
                    time: Some("08:30".into()),
                    ..Default::default()
                },
                enabled: true,
            })
            .await
            .expect("folder sched");
        assert_eq!(svc.list().await.expect("list").len(), 1);
        repo_folders::delete(&db, &folder_id)
            .await
            .expect("del folder");
        assert!(repo_schedules::get(&db, &f_sched.id)
            .await
            .expect("get")
            .is_none());
    }

    #[test]
    fn r_sched_02_interval_daily_weekly_on_launch_and_manual_next_run_math() {
        let now = utc_ms(2026, 6, 15, 10, 0); // Mon 2026-06-15 10:00 UTC
        let clock = FixedClock::utc(now);

        // Interval 15 min (900s)
        let next_int = compute_next_run_at(
            ScheduleType::Interval,
            &ScheduleConfig {
                interval_secs: Some(900),
                ..Default::default()
            },
            now,
            &clock,
        )
        .expect("interval")
        .expect("some");
        assert_eq!(next_int, utc_ms(2026, 6, 15, 10, 15));

        // Daily at 14:00 (still ahead today -> today 14:00)
        let next_daily_today = compute_next_run_at(
            ScheduleType::Daily,
            &ScheduleConfig {
                time: Some("14:00".into()),
                ..Default::default()
            },
            now,
            &clock,
        )
        .expect("daily today")
        .expect("some");
        assert_eq!(next_daily_today, utc_ms(2026, 6, 15, 14, 0));

        // Daily at 09:00 (already passed today -> tomorrow 09:00)
        let next_daily_tomorrow = compute_next_run_at(
            ScheduleType::Daily,
            &ScheduleConfig {
                time: Some("09:00".into()),
                ..Default::default()
            },
            now,
            &clock,
        )
        .expect("daily tomorrow")
        .expect("some");
        assert_eq!(next_daily_tomorrow, utc_ms(2026, 6, 16, 9, 0));

        // OnLaunch / Manual / Realtime have no wall-clock `next_run_at`.
        for mode in [
            ScheduleType::OnLaunch,
            ScheduleType::Manual,
            ScheduleType::Realtime,
        ] {
            assert_eq!(
                compute_next_run_at(mode, &ScheduleConfig::default(), now, &clock).expect("mode"),
                None
            );
        }
    }

    #[test]
    fn r_sched_03_dst_spring_forward_and_fall_back_preserve_local_wall_clock() {
        // 1. Spring forward: 2026-03-08 02:00 EST (-5h) -> 03:00 EDT (-4h), transition at 07:00 UTC.
        let spring_trans = utc_ms(2026, 3, 8, 7, 0);
        let spring_now = utc_ms(2026, 3, 7, 14, 0); // 09:00 EST on Mar 7
        let spring_clock = FixedClock::with_dst(spring_now, spring_trans, -300, -240);

        // Next 09:00 local on Mar 8 should be 13:00 UTC (EDT -4h -> 09:00 local), 23 hours later!
        let cfg_0900 = ScheduleConfig {
            time: Some("09:00".into()),
            ..Default::default()
        };
        let next_spring =
            compute_next_run_at(ScheduleType::Daily, &cfg_0900, spring_now, &spring_clock)
                .expect("spring")
                .expect("some");
        assert_eq!(next_spring - spring_now, 23 * 3_600_000);
        let local_dt = spring_clock.to_local(next_spring);
        assert_eq!(
            local_dt.date_naive(),
            NaiveDate::from_ymd_opt(2026, 3, 8).expect("valid date")
        );
        assert_eq!(
            local_dt.time(),
            NaiveTime::from_hms_opt(9, 0, 0).expect("valid time")
        );

        // Spring gap: daily at 02:30 (skipped hour!) lands cleanly at 03:00 local (07:00 UTC).
        let cfg_0230 = ScheduleConfig {
            time: Some("02:30".into()),
            ..Default::default()
        };
        let before_gap = utc_ms(2026, 3, 8, 6, 0); // 01:00 EST
        let next_gap =
            compute_next_run_at(ScheduleType::Daily, &cfg_0230, before_gap, &spring_clock)
                .expect("spring gap")
                .expect("some");
        assert_eq!(next_gap, spring_trans);

        // 2. Fall back: 2026-11-01 02:00 EDT (-4h) -> 01:00 EST (-5h), transition at 06:00 UTC.
        let fall_trans = utc_ms(2026, 11, 1, 6, 0);
        let fall_now = utc_ms(2026, 10, 31, 13, 0); // 09:00 EDT on Oct 31
        let fall_clock = FixedClock::with_dst(fall_now, fall_trans, -240, -300);

        let next_fall = compute_next_run_at(ScheduleType::Daily, &cfg_0900, fall_now, &fall_clock)
            .expect("fall")
            .expect("some");
        // Oct 31 09:00 EDT -> Nov 1 09:00 EST is 25 hours!
        assert_eq!(next_fall - fall_now, 25 * 3_600_000);
        let local_fall = fall_clock.to_local(next_fall);
        assert_eq!(
            local_fall.time(),
            NaiveTime::from_hms_opt(9, 0, 0).expect("valid time")
        );

        // Repeated hour 01:30 must fire once on Nov 1 (first occurrence at 05:30 UTC), then advance to Nov 2!
        let cfg_0130 = ScheduleConfig {
            time: Some("01:30".into()),
            ..Default::default()
        };
        let first_pass = utc_ms(2026, 11, 1, 5, 30); // 01:30 EDT
        let next_after_first =
            compute_next_run_at(ScheduleType::Daily, &cfg_0130, first_pass, &fall_clock)
                .expect("after first 01:30")
                .expect("some");
        assert_eq!(
            fall_clock.to_local(next_after_first).date_naive(),
            NaiveDate::from_ymd_opt(2026, 11, 2).expect("valid date")
        );
    }

    #[test]
    fn r_sched_04_quiet_hours_same_day_and_overnight_defer_next_run() {
        let clock = FixedClock::utc(utc_ms(2026, 6, 15, 21, 45));

        // 1. Overnight quiet hours 22:00..07:00 defer a 30-min interval hitting 22:15 -> 07:00 next day.
        let overnight = QuietHours {
            start: "22:00".into(),
            end: "07:00".into(),
        };
        let next = compute_next_run_at(
            ScheduleType::Interval,
            &ScheduleConfig {
                interval_secs: Some(1800),
                quiet_hours: Some(overnight.clone()),
                ..Default::default()
            },
            utc_ms(2026, 6, 15, 21, 45),
            &clock,
        )
        .expect("overnight")
        .expect("some");
        assert_eq!(next, utc_ms(2026, 6, 16, 7, 0));
        assert!(is_in_quiet_hours(
            utc_ms(2026, 6, 15, 23, 30),
            &overnight,
            &clock
        ));
        assert!(is_in_quiet_hours(
            utc_ms(2026, 6, 16, 6, 30),
            &overnight,
            &clock
        ));
        assert!(!is_in_quiet_hours(
            utc_ms(2026, 6, 16, 7, 0),
            &overnight,
            &clock
        ));

        // 2. Same-day quiet hours 12:00..14:00 defer a 13:00 slot -> 14:00 same day.
        let daytime = QuietHours {
            start: "12:00".into(),
            end: "14:00".into(),
        };
        let next_day = compute_next_run_at(
            ScheduleType::Daily,
            &ScheduleConfig {
                time: Some("13:00".into()),
                quiet_hours: Some(daytime.clone()),
                ..Default::default()
            },
            utc_ms(2026, 6, 15, 10, 0),
            &clock,
        )
        .expect("daytime")
        .expect("some");
        assert_eq!(next_day, utc_ms(2026, 6, 15, 14, 0));
        assert!(is_in_quiet_hours(
            utc_ms(2026, 6, 15, 12, 30),
            &daytime,
            &clock
        ));
        assert!(!is_in_quiet_hours(
            utc_ms(2026, 6, 15, 14, 0),
            &daytime,
            &clock
        ));
    }

    #[test]
    fn r_sched_05_weekdays_filter_advances_to_next_allowed_day() {
        // 2026-06-15 is Monday, 10:00 UTC.
        let now = utc_ms(2026, 6, 15, 10, 0);
        let clock = FixedClock::utc(now);

        let cfg = ScheduleConfig {
            time: Some("09:00".into()),
            weekdays: vec![Weekday::Wed, Weekday::Fri],
            ..Default::default()
        };

        let next = compute_next_run_at(ScheduleType::Weekly, &cfg, now, &clock)
            .expect("weekly")
            .expect("some");
        // Next allowed after Mon Jun 15 10:00 is Wed Jun 17 09:00!
        assert_eq!(next, utc_ms(2026, 6, 17, 9, 0));
        assert_eq!(clock.to_local(next).weekday(), chrono::Weekday::Wed);

        // From Fri Jun 19 09:00 -> jumps over weekend to Wed Jun 24 09:00.
        let fri = utc_ms(2026, 6, 19, 9, 0);
        let next_after_fri = compute_next_run_at(ScheduleType::Weekly, &cfg, fri, &clock)
            .expect("weekly fri")
            .expect("some");
        assert_eq!(next_after_fri, utc_ms(2026, 6, 24, 9, 0));
    }

    #[test]
    fn r_sched_06_catch_up_skip_vs_run_once_never_runs_all_missed() {
        // Scheduled every 15 min; `next_run_at` was 10:00, system wakes at 13:05 (3+ hours / 12 runs missed).
        let scheduled = utc_ms(2026, 6, 15, 10, 0);
        let wake_now = utc_ms(2026, 6, 15, 13, 5);
        let clock = FixedClock::utc(wake_now);

        let mut s_skip = Schedule {
            id: "s-skip".into(),
            rule_id: Some("r1".into()),
            folder_id: None,
            schedule_type: ScheduleType::Interval,
            config: ScheduleConfig {
                interval_secs: Some(900),
                catch_up: CatchUpPolicy::Skip,
                ..Default::default()
            },
            enabled: true,
            last_run_at: Some(utc_ms(2026, 6, 15, 9, 45)),
            next_run_at: Some(scheduled),
        };
        let dec_skip = evaluate_due_schedule(&s_skip, wake_now, &clock).expect("skip");
        match dec_skip {
            DueDecision::MissedSkipped { next_run_at } => {
                assert!(next_run_at.expect("next_run_at") > wake_now);
            }
            other => panic!("expected MissedSkipped, got {other:?}"),
        }

        // CatchUpPolicy::RunOnce executes once (`is_catch_up: true`) and advances `next_run_at` strictly > wake_now.
        s_skip.config.catch_up = CatchUpPolicy::RunOnce;
        let dec_once = evaluate_due_schedule(&s_skip, wake_now, &clock).expect("once");
        match dec_once {
            DueDecision::RunNow {
                is_catch_up,
                next_run_at,
            } => {
                assert!(is_catch_up);
                assert_eq!(next_run_at, Some(utc_ms(2026, 6, 15, 13, 20)));
            }
            other => panic!("expected RunNow catch-up, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn r_sched_07_on_launch_and_run_now_organize_matching_files_in_scope() {
        let (_d, root) = tempdir();
        let clock = Arc::new(FixedClock::utc(utc_ms(2026, 6, 15, 10, 0)));
        let (svc, _db, folder_id, rule) = setup_scheduler_env(&root, clock).await;

        let launch_sched = svc
            .upsert(ScheduleInput {
                id: None,
                rule_id: Some(rule.id.clone()),
                folder_id: Some(folder_id.clone()),
                schedule_type: ScheduleType::OnLaunch,
                config: ScheduleConfig::default(),
                enabled: true,
            })
            .await
            .expect("launch sched");

        let manual_sched = svc
            .upsert(ScheduleInput {
                id: None,
                rule_id: Some(rule.id),
                folder_id: Some(folder_id),
                schedule_type: ScheduleType::Manual,
                config: ScheduleConfig::default(),
                enabled: true,
            })
            .await
            .expect("manual sched");

        let f1 = root.join("launch_doc.txt");
        write_file(&f1, b"launch payload");
        let launch_outcomes = svc.run_on_launch().await.expect("run_on_launch");
        assert_eq!(launch_outcomes.len(), 1);
        assert_eq!(launch_outcomes[0].schedule_id, launch_sched.id);
        assert_eq!(launch_outcomes[0].status, ScheduleRunStatus::Executed);
        assert!(!f1.exists());
        assert!(root.join("Organized/launch_doc.txt").exists());

        // Drop another file and run manual schedule via `run_now`.
        let f2 = root.join("manual_doc.txt");
        write_file(&f2, b"manual payload");
        let manual_out = svc.run_now(&manual_sched.id).await.expect("run_now");
        assert_eq!(manual_out.status, ScheduleRunStatus::Executed);
        assert!(!f2.exists());
        assert!(root.join("Organized/manual_doc.txt").exists());
    }

    #[tokio::test]
    async fn r_sched_08_per_scope_lock_prevents_overlapping_runs() {
        let (_d, root) = tempdir();
        let clock = Arc::new(FixedClock::utc(utc_ms(2026, 6, 15, 10, 0)));
        let (svc, _db, folder_id, rule) = setup_scheduler_env(&root, clock).await;

        let s = svc
            .upsert(ScheduleInput {
                id: None,
                rule_id: Some(rule.id.clone()),
                folder_id: Some(folder_id.clone()),
                schedule_type: ScheduleType::Manual,
                config: ScheduleConfig::default(),
                enabled: true,
            })
            .await
            .expect("sched");

        // Hold the scope lock explicitly to simulate an in-flight run for `(folder_id, rule.id)`.
        let scope_lock = svc.scope_lock(&ScopeKey {
            folder_id: Some(folder_id),
            rule_id: Some(rule.id),
        });
        let held = scope_lock.lock().await;

        // While locked, `run_now` for the same scope must NOT overlap -> returns SkippedOverlap immediately.
        let out = svc.run_now(&s.id).await.expect("run_now while locked");
        assert_eq!(out.status, ScheduleRunStatus::SkippedOverlap);
        drop(held);

        // Concurrency test across 4 tasks racing on `try_lock`: max simultaneous holders == 1.
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let lock_ref = scope_lock.clone();
            let active_ref = active.clone();
            let max_ref = max_active.clone();
            handles.push(tokio::spawn(async move {
                let _g = lock_ref.lock().await;
                let cur = active_ref.fetch_add(1, Ordering::SeqCst) + 1;
                max_ref.fetch_max(cur, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                active_ref.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.expect("join");
        }
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn bug_022_scheduled_run_honors_folder_exclusions_and_monitoring_settings() {
        let (_d, root) = tempdir();
        let clock = Arc::new(FixedClock::utc(utc_ms(2026, 6, 15, 10, 0)));
        let (svc, db, folder_id, rule) = setup_scheduler_env(&root, clock).await;

        let excluded_dir = root.join("SkipDir");
        std::fs::create_dir_all(&excluded_dir).expect("mkdir SkipDir");

        // Configure per-folder extension/subfolder exclusions.
        let mut folder = repo_folders::get(&db, &folder_id)
            .await
            .expect("get folder")
            .expect("exists");
        folder.settings.exclude_extensions = vec!["log".into()];
        folder.settings.excluded_subfolders = vec![excluded_dir.to_string_lossy().into_owned()];
        repo_folders::update(&db, &folder)
            .await
            .expect("update folder settings");

        // Disable `ignore_hidden_files` globally so dotfiles are processed when configured.
        let settings_svc = crate::settings::SettingsService::new(db.clone());
        let mut settings = settings_svc.get().await.expect("get settings");
        settings.monitoring.ignore_hidden_files = false;
        settings_svc
            .update(settings)
            .await
            .expect("update settings");

        let keep_visible = root.join("keep.txt");
        let skip_ext = root.join("debug.log");
        let skip_sub = excluded_dir.join("inside_skip.txt");
        let include_dotfile = root.join(".hidden_note.txt");
        write_file(&keep_visible, b"keep");
        write_file(&skip_ext, b"log");
        write_file(&skip_sub, b"sub");
        write_file(&include_dotfile, b"dot");

        let sched = svc
            .upsert(ScheduleInput {
                id: None,
                rule_id: Some(rule.id),
                folder_id: Some(folder_id),
                schedule_type: ScheduleType::Manual,
                config: ScheduleConfig::default(),
                enabled: true,
            })
            .await
            .expect("upsert");

        let outcome = svc.run_now(&sched.id).await.expect("run_now");
        assert_eq!(outcome.status, ScheduleRunStatus::Executed);

        assert!(!keep_visible.exists(), "normal file must be organized");
        assert!(root.join("Organized/keep.txt").exists());
        assert!(
            skip_ext.exists(),
            "BUG-022: excluded extension `.log` must stay untouched"
        );
        assert!(
            skip_sub.exists(),
            "BUG-022: excluded subfolder file must stay untouched"
        );
        assert!(
            !include_dotfile.exists() && root.join("Organized/.hidden_note.txt").exists(),
            "BUG-022: disabling global ignore_hidden_files must include hidden files"
        );
    }
}
