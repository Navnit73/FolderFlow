//! Real-time automation pipeline (`P5-T04`; `FR-025`, `NFR-021`).
//!
//! Connects stabilized watcher/reconcile events (`MonitorObserver::on_stable`) to
//! rule evaluation and transactional execution via [`Organizer`], behind a bounded
//! queue with non-blocking backpressure: when a massive burst overflows the queue,
//! overflow events are dropped without blocking the watcher and the folder is marked
//! `dirty` for background reconciliation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use tokio::sync::{mpsc, Mutex, Notify};

use crate::database::repo_operations::OperationView;
use crate::database::repo_schedules::{Schedule, ScheduleType};
use crate::database::{repo_files, repo_folders, repo_rules, repo_schedules, Db};
use crate::engine::classifier::{Classifier, HEADER_BYTES};
use crate::engine::conflicts::ConflictPolicy;
use crate::engine::fs_view::{EntryKind, ReadOnlyFs, RealFs};
use crate::engine::monitor::{FolderRuntime, MonitorObserver};
use crate::engine::operations::{BatchTrigger, OpKind};
use crate::engine::organizer::Organizer;
use crate::engine::paths::{nfc, PathAuthority};
use crate::engine::planner::{
    build_execution_plan, execute_plan, PlanCandidate, PlanContext, PlanEnv, PlanScope,
};
use crate::engine::reconcile::FolderScanSpec;
use crate::engine::scanner::{scan, ScanOptions, ScannedFile};
use crate::engine::scheduler::is_in_quiet_hours;
use crate::error::{AppError, AppResult};
use crate::settings::SettingsService;

pub const DEFAULT_PIPELINE_QUEUE_CAPACITY: usize = 4096;

#[derive(Debug, Clone)]
pub struct PipelineEvent {
    pub folder_id: String,
    pub file: ScannedFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueResult {
    Queued,
    OverflowMarkedDirty,
}

#[derive(Debug, Clone)]
pub enum PipelineStepOutcome {
    Organized(Vec<OperationView>),
    QueuedForReview(u32),
    NoMatch,
    SkippedPaused,
    SkippedStale,
}

#[derive(Clone)]
pub struct AutomationPipeline {
    db: Db,
    organizer: Organizer,
    classifier: Arc<Classifier>,
    authority: Arc<RwLock<PathAuthority>>,
    conflict_policy: ConflictPolicy,
    paused: Arc<AtomicBool>,
    tx: mpsc::Sender<PipelineEvent>,
    rx: Arc<Mutex<mpsc::Receiver<PipelineEvent>>>,
    dirty_folders: Arc<std::sync::Mutex<HashSet<String>>>,
    dropped_overflow: Arc<AtomicU64>,
    notify: Arc<Notify>,
}

impl AutomationPipeline {
    pub fn new(
        db: Db,
        organizer: Organizer,
        classifier: Arc<Classifier>,
        authority: PathAuthority,
        capacity: usize,
    ) -> Self {
        let cap = capacity.max(1);
        let (tx, rx) = mpsc::channel(cap);
        Self {
            db,
            organizer,
            classifier,
            authority: Arc::new(RwLock::new(authority)),
            conflict_policy: ConflictPolicy::AutoRename,
            paused: Arc::new(AtomicBool::new(false)),
            tx,
            rx: Arc::new(Mutex::new(rx)),
            dirty_folders: Arc::new(std::sync::Mutex::new(HashSet::new())),
            dropped_overflow: Arc::new(AtomicU64::new(0)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub async fn notified(&self) {
        self.notify.notified().await;
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn set_authority(&self, authority: PathAuthority) {
        let mut guard = self.authority.write().unwrap_or_else(|e| e.into_inner());
        *guard = authority;
    }

    pub fn dropped_overflow_count(&self) -> u64 {
        self.dropped_overflow.load(Ordering::SeqCst)
    }

    pub fn is_folder_dirty(&self, folder_id: &str) -> bool {
        let guard = self.dirty_folders.lock().unwrap_or_else(|e| e.into_inner());
        guard.contains(folder_id)
    }

    /// Non-blocking enqueue from `MonitorObserver::on_stable`.
    /// When the bounded channel is full (`FR-025`, `NFR-021`), drops the event without
    /// blocking and marks `folder_id` dirty for reconciliation.
    pub fn enqueue(&self, folder_id: &str, file: ScannedFile) -> EnqueueResult {
        let ev = PipelineEvent {
            folder_id: folder_id.to_owned(),
            file,
        };
        let res = match self.tx.try_send(ev) {
            Ok(()) => EnqueueResult::Queued,
            Err(_) => {
                self.dropped_overflow.fetch_add(1, Ordering::SeqCst);
                let mut guard = self.dirty_folders.lock().unwrap_or_else(|e| e.into_inner());
                guard.insert(folder_id.to_owned());
                EnqueueResult::OverflowMarkedDirty
            }
        };
        self.notify.notify_one();
        res
    }

    /// Processes a single stabilized file immediately against enabled rules.
    pub async fn process_file(
        &self,
        folder_id: &str,
        file: &ScannedFile,
        trigger: BatchTrigger,
    ) -> AppResult<PipelineStepOutcome> {
        if self.is_paused() {
            return Ok(PipelineStepOutcome::SkippedPaused);
        }

        let Some(folder) = repo_folders::get(&self.db, folder_id).await? else {
            return Ok(PipelineStepOutcome::SkippedPaused);
        };
        if !folder.enabled {
            return Ok(PipelineStepOutcome::SkippedPaused);
        }

        let schedules = repo_schedules::list(&self.db).await?;
        let now_ms = self.organizer.clock().now_ms();
        let clock = self.organizer.clock();
        let is_active_realtime = |s: &&Schedule| {
            s.enabled
                && s.schedule_type == ScheduleType::Realtime
                && !s
                    .config
                    .quiet_hours
                    .as_ref()
                    .is_some_and(|qh| is_in_quiet_hours(now_ms, qh, clock.as_ref()))
        };

        let folder_scheds: Vec<_> = schedules
            .iter()
            .filter(|s| s.folder_id.as_deref() == Some(folder_id) && s.rule_id.is_none())
            .collect();
        if !folder_scheds.is_empty() && !folder_scheds.iter().any(is_active_realtime) {
            return Ok(PipelineStepOutcome::NoMatch);
        }

        let rules: Vec<_> = repo_rules::list(&self.db)
            .await?
            .into_iter()
            .filter(|r| {
                if !r.enabled {
                    return false;
                }
                let rule_scheds: Vec<_> = schedules
                    .iter()
                    .filter(|s| {
                        s.rule_id.as_deref() == Some(r.id.as_str())
                            && (s.folder_id.is_none() || s.folder_id.as_deref() == Some(folder_id))
                    })
                    .collect();
                rule_scheds.is_empty() || rule_scheds.iter().any(is_active_realtime)
            })
            .collect();
        if rules.is_empty() {
            return Ok(PipelineStepOutcome::NoMatch);
        }

        let authority = self
            .authority
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let cand = PlanCandidate {
            folder_id: folder.id.clone(),
            folder_path: PathBuf::from(&folder.path),
            folder_name: folder.name.clone(),
            path: file.facts.path.clone(),
            category_hint: Some(file.category.clone()),
            mime_hint: file.mime.clone(),
            content_hash: None,
            is_duplicate: false,
        };

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &authority,
                classifier: &self.classifier,
                clock: self.organizer.clock().as_ref(),
            },
            &rules,
            &[cand],
            &PlanScope {
                folder_id: Some(folder.id.clone()),
                rule_id: None,
                file_paths: None,
            },
            &PlanEnv::default(),
        );

        if plan.items.is_empty() {
            return Ok(PipelineStepOutcome::NoMatch);
        }

        let rep = execute_plan(
            &plan,
            None,
            self.conflict_policy,
            &self.organizer,
            &authority,
            trigger,
        )
        .await?;

        let folder_root_nfc = nfc(Path::new(&folder.path));
        for op in &rep.completed {
            if matches!(op.operation_type, OpKind::Move | OpKind::Rename) {
                let _ = repo_files::delete_path(&self.db, &op.source_path).await?;
            }
            let dst_path = Path::new(&op.destination_path);
            if nfc(dst_path).starts_with(&folder_root_nfc) {
                if let Ok(meta) = RealFs.stat(dst_path) {
                    let is_pkg = meta.kind == EntryKind::Package;
                    let header = if is_pkg {
                        None
                    } else {
                        RealFs.read_header(dst_path, HEADER_BYTES).ok()
                    };
                    let cls = self
                        .classifier
                        .classify(&meta.name, header.as_deref(), is_pkg);
                    let new_sf = ScannedFile {
                        facts: meta,
                        category: cls.category,
                        mime: cls.mime,
                        confidence: cls.confidence,
                    };
                    let now_ms = self.organizer.clock().now_ms();
                    let _ = repo_files::upsert_batch(
                        &self.db,
                        &folder.id,
                        std::slice::from_ref(&new_sf),
                        now_ms,
                    )
                    .await?;
                }
            }
        }

        if !rep.completed.is_empty() {
            Ok(PipelineStepOutcome::Organized(rep.completed))
        } else if rep.queued_for_review > 0 {
            Ok(PipelineStepOutcome::QueuedForReview(rep.queued_for_review))
        } else if !rep.skipped_stale.is_empty() {
            Ok(PipelineStepOutcome::SkippedStale)
        } else {
            Ok(PipelineStepOutcome::NoMatch)
        }
    }

    /// Drains all currently queued events and reconciles any folders marked dirty by overflow.
    pub async fn drain_and_reconcile(&self) -> AppResult<Vec<PipelineStepOutcome>> {
        let mut drained = Vec::new();
        {
            let mut rx = self.rx.lock().await;
            while let Ok(ev) = rx.try_recv() {
                drained.push(ev);
            }
        }

        let mut outcomes = Vec::with_capacity(drained.len());
        for ev in drained {
            let outcome = self
                .process_file(&ev.folder_id, &ev.file, BatchTrigger::Realtime)
                .await?;
            outcomes.push(outcome);
        }

        if self.is_paused() {
            return Ok(outcomes);
        }

        let dirty_ids: Vec<String> = {
            let mut guard = self.dirty_folders.lock().unwrap_or_else(|e| e.into_inner());
            guard.drain().collect()
        };

        if !dirty_ids.is_empty() {
            let monitoring = SettingsService::new(self.db.clone())
                .get()
                .await?
                .monitoring;
            for folder_id in dirty_ids {
                let Some(folder) = repo_folders::get(&self.db, &folder_id).await? else {
                    continue;
                };
                if !folder.enabled {
                    continue;
                }
                let spec = FolderScanSpec::from_folder(&folder, &monitoring);
                let cls = self.classifier.clone();
                let scanned_files = tokio::task::spawn_blocking(move || {
                    let opts = ScanOptions {
                        recursive: spec.recursive,
                        rules: spec.rules,
                    };
                    let mut scanned_files = Vec::new();
                    let _ = scan(&spec.path, &opts, &cls, |batch| {
                        scanned_files.extend(batch);
                        Ok(())
                    });
                    scanned_files
                })
                .await
                .map_err(|e| AppError::Internal(format!("reconcile scan failed: {e}")))?;
                for sf in scanned_files {
                    let outcome = self
                        .process_file(&folder.id, &sf, BatchTrigger::Realtime)
                        .await?;
                    outcomes.push(outcome);
                }
            }
        }

        Ok(outcomes)
    }
}

impl MonitorObserver for AutomationPipeline {
    fn on_status(&self, _status: &FolderRuntime) {}

    fn on_stable(&self, folder_id: &str, file: &ScannedFile) {
        let _ = self.enqueue(folder_id, file.clone());
    }

    fn on_removed(&self, _folder_id: &str, _path: &Path) {}
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::*;
    use crate::categories::builtin_categories;
    use crate::database::{self, repo_files, repo_folders, repo_rules};
    use crate::engine::classifier::{Classifier, Confidence};
    use crate::engine::fs_view::{ReadOnlyFs, RealFs};
    use crate::engine::operations::Ledger;
    use crate::engine::paths::{ApprovedRoot, PathAuthority, RootKind};
    use crate::engine::safe_fs::SafeFs;
    use crate::rules::models::{
        Action, Condition, ConditionNode, DateSource, ExecutionMode, Rule, RuleOrigin,
        TextFieldKind, TextOp,
    };
    use crate::test_support::{tempdir, write_file};
    use crate::utils::clock::FixedClock;

    async fn insert_rule(
        db: &Db,
        id: &str,
        name: &str,
        enabled: bool,
        conditions: ConditionNode,
        actions: Vec<Action>,
    ) -> Rule {
        let rule = Rule {
            id: id.into(),
            name: name.into(),
            description: None,
            enabled,
            priority: 1,
            source_folders: Vec::new(),
            conditions,
            actions,
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
        repo_rules::insert(db, &rule).await.expect("insert rule");
        rule
    }

    async fn setup_pipeline(root: &Path, capacity: usize) -> (AutomationPipeline, Db, String) {
        let db = database::open_in_memory().await.expect("db");
        let folder_id = "folder-dl".to_owned();
        repo_folders::insert(&db, &folder_id, "Downloads", &root.to_string_lossy(), true)
            .await
            .expect("insert folder");

        let ledger = Ledger::new(db.clone());
        let fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
        let org = Organizer::new(ledger, fs, clock);
        let cls = Arc::new(Classifier::new(builtin_categories()));
        let auth = PathAuthority::new(vec![ApprovedRoot {
            path: root.to_path_buf(),
            kind: RootKind::Monitored,
        }]);
        let pipe = AutomationPipeline::new(db.clone(), org, cls, auth, capacity);
        (pipe, db, folder_id)
    }

    fn scanned_file_for(path: &Path, category: &str) -> ScannedFile {
        let facts = RealFs.stat(path).expect("stat file");
        ScannedFile {
            facts,
            category: category.into(),
            mime: None,
            confidence: Confidence::High,
        }
    }

    #[tokio::test]
    async fn r_pipe_01_stable_file_triggers_matching_enabled_rule_and_organizes_file() {
        let (_d, root) = tempdir();
        let (pipe, db, folder_id) = setup_pipeline(&root, 64).await;

        let rule = insert_rule(
            &db,
            "r-inv",
            "Move Invoices",
            true,
            ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::Contains,
                    value: "invoice".into(),
                    case_sensitive: false,
                },
            },
            vec![Action::Move {
                destination: root.join("Sorted/Invoices").to_string_lossy().into_owned(),
            }],
        )
        .await;

        let f = root.join("invoice_april.pdf");
        write_file(&f, b"%PDF-1.7 april");
        let sf = scanned_file_for(&f, "documents");

        assert_eq!(pipe.enqueue(&folder_id, sf), EnqueueResult::Queued);
        let outcomes = pipe.drain_and_reconcile().await.expect("drain");
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0], PipelineStepOutcome::Organized(_)));

        assert!(!f.exists(), "source must be moved");
        assert!(root.join("Sorted/Invoices/invoice_april.pdf").exists());

        let updated_rule = repo_rules::get(&db, &rule.id)
            .await
            .expect("get")
            .expect("rule");
        assert_eq!(updated_rule.stat_matched, 1);
        assert_eq!(updated_rule.stat_succeeded, 1);
    }

    #[tokio::test]
    async fn r_pipe_02_disabled_rules_or_paused_automation_never_mutate_files() {
        let (_d, root) = tempdir();
        let (pipe, db, folder_id) = setup_pipeline(&root, 64).await;

        // 1. Disabled rule never runs.
        let rule = insert_rule(
            &db,
            "r-dis",
            "Disabled Rule",
            false,
            ConditionNode::All {
                children: Vec::new(),
            },
            vec![Action::Move {
                destination: root.join("Moved").to_string_lossy().into_owned(),
            }],
        )
        .await;

        let f1 = root.join("keep_me.txt");
        write_file(&f1, b"stay put");
        let sf1 = scanned_file_for(&f1, "documents");
        pipe.enqueue(&folder_id, sf1.clone());
        let _ = pipe.drain_and_reconcile().await.expect("drain");
        assert!(f1.exists(), "disabled rule must not move file");

        // 2. Enable rule, but pause automation globally -> still must not mutate.
        repo_rules::set_enabled(&db, &rule.id, true)
            .await
            .expect("enable rule");
        pipe.set_paused(true);
        pipe.enqueue(&folder_id, sf1);
        let outcomes = pipe.drain_and_reconcile().await.expect("drain");
        assert!(
            outcomes
                .iter()
                .all(|o| matches!(o, PipelineStepOutcome::SkippedPaused)),
            "paused pipeline must skip without mutating"
        );
        assert!(f1.exists(), "paused pipeline must not move file");
    }

    #[tokio::test]
    async fn r_pipe_03_backpressure_drops_overflow_and_marks_folder_dirty_for_reconcile() {
        let (_d, root) = tempdir();
        // Tiny capacity of 16 to test 50,000-event flood backpressure (`FR-025`, `NFR-021`).
        let (pipe, db, folder_id) = setup_pipeline(&root, 16).await;

        insert_rule(
            &db,
            "r-flood",
            "Sort Flood",
            true,
            ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::StartsWith,
                    value: "burst_".into(),
                    case_sensitive: false,
                },
            },
            vec![Action::Move {
                destination: root.join("Sorted").to_string_lossy().into_owned(),
            }],
        )
        .await;

        // Create 5 real files on disk, and flood 50,000 events into the bounded queue.
        let mut real_files = Vec::new();
        for i in 0..5 {
            let p = root.join(format!("burst_{i}.txt"));
            write_file(&p, format!("payload-{i}").as_bytes());
            real_files.push(scanned_file_for(&p, "documents"));
        }

        for i in 0..50_000 {
            let sf = &real_files[i % real_files.len()];
            let _ = pipe.enqueue(&folder_id, sf.clone());
        }

        assert!(
            pipe.dropped_overflow_count() >= 49_980,
            "overflow events must be dropped without blocking"
        );
        assert!(
            pipe.is_folder_dirty(&folder_id),
            "overflow must mark folder dirty for reconciliation"
        );

        // Draining and reconciling processes the folder cleanly and clears the dirty flag.
        let _ = pipe
            .drain_and_reconcile()
            .await
            .expect("drain_and_reconcile");
        assert!(
            !pipe.is_folder_dirty(&folder_id),
            "reconcile must clear dirty flag"
        );
        for i in 0..5 {
            assert!(
                root.join(format!("Sorted/burst_{i}.txt")).exists(),
                "burst_{i}.txt must be organized after reconcile"
            );
        }
    }

    #[tokio::test]
    async fn r_pipe_04_updates_file_records_after_move_or_rename() {
        let (_d, root) = tempdir();
        let (pipe, db, folder_id) = setup_pipeline(&root, 64).await;

        let f = root.join("draft_note.txt");
        write_file(&f, b"hello world");
        let sf = scanned_file_for(&f, "documents");
        repo_files::upsert_batch(
            &db,
            &folder_id,
            std::slice::from_ref(&sf),
            1_700_000_000_000,
        )
        .await
        .expect("seed file_records");

        insert_rule(
            &db,
            "r-notes",
            "Move Notes",
            true,
            ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::StartsWith,
                    value: "draft_".into(),
                    case_sensitive: false,
                },
            },
            vec![
                Action::AddPrefix {
                    prefix: "FINAL_".into(),
                },
                Action::Move {
                    destination: root.join("Notes").to_string_lossy().into_owned(),
                },
            ],
        )
        .await;

        pipe.enqueue(&folder_id, sf);
        let _ = pipe.drain_and_reconcile().await.expect("drain");

        let new_path = root.join("Notes/FINAL_draft_note.txt");
        assert!(new_path.exists());

        // Old path must be removed from `file_records`, and new path must be recorded.
        let old_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM file_records WHERE path = ?1")
                .bind(f.to_string_lossy().as_ref())
                .fetch_one(&db)
                .await
                .expect("count old");
        assert_eq!(
            old_count, 0,
            "old source path must be removed from file_records"
        );

        let new_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM file_records WHERE path = ?1")
                .bind(new_path.to_string_lossy().as_ref())
                .fetch_one(&db)
                .await
                .expect("count new");
        assert_eq!(
            new_count, 1,
            "new destination path must be upserted into file_records"
        );
    }

    #[tokio::test]
    async fn bug_023_realtime_pipeline_scopes_folder_schedules_and_enforces_quiet_hours() {
        let (_d1, root1) = tempdir();
        let (_d2, root2) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        let f1 = "folder-1".to_owned();
        let f2 = "folder-2".to_owned();
        repo_folders::insert(&db, &f1, "One", &root1.to_string_lossy(), true)
            .await
            .expect("f1");
        repo_folders::insert(&db, &f2, "Two", &root2.to_string_lossy(), true)
            .await
            .expect("f2");

        // Clock is 1_700_000_000_000 ms == 2023-11-14T22:13:20Z.
        let ledger = Ledger::new(db.clone());
        let fs = Arc::new(SafeFs::with_trash_dir(root1.join(".Trash")));
        let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
        let org = Organizer::new(ledger, fs, clock);
        let cls = Arc::new(Classifier::new(builtin_categories()));
        let auth = PathAuthority::new(vec![
            ApprovedRoot {
                path: root1.clone(),
                kind: RootKind::Monitored,
            },
            ApprovedRoot {
                path: root2.clone(),
                kind: RootKind::Monitored,
            },
        ]);
        let pipe = AutomationPipeline::new(db.clone(), org, cls, auth, 64);

        let rule = insert_rule(
            &db,
            "r-shared",
            "Shared Rule",
            true,
            ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::Contains,
                    value: "scope_".into(),
                    case_sensitive: false,
                },
            },
            vec![Action::Move {
                destination: root2.join("Sorted").to_string_lossy().into_owned(),
            }],
        )
        .await;

        // Bind a Manual schedule ONLY to (folder-1, r-shared). It must NOT suppress r-shared in folder-2!
        repo_schedules::upsert(
            &db,
            &repo_schedules::Schedule {
                id: "s-manual-f1".into(),
                rule_id: Some(rule.id.clone()),
                folder_id: Some(f1.clone()),
                schedule_type: ScheduleType::Manual,
                config: repo_schedules::ScheduleConfig::default(),
                enabled: true,
                last_run_at: None,
                next_run_at: None,
            },
        )
        .await
        .expect("insert f1 manual schedule");

        let f2_file = root2.join("scope_unblocked.txt");
        write_file(&f2_file, b"hello f2");
        pipe.enqueue(&f2, scanned_file_for(&f2_file, "documents"));
        let outcomes = pipe.drain_and_reconcile().await.expect("drain f2");
        assert!(
            matches!(outcomes.as_slice(), [PipelineStepOutcome::Organized(_)]),
            "BUG-023a: manual schedule on (f1, r-shared) must not suppress f2; got {outcomes:?}"
        );
        assert!(root2.join("Sorted/scope_unblocked.txt").exists());

        // Now attach an enabled Realtime schedule on (f2, None) whose quiet_hours cover 22:13Z (22:00..23:00).
        repo_schedules::upsert(
            &db,
            &repo_schedules::Schedule {
                id: "s-rt-quiet-f2".into(),
                rule_id: None,
                folder_id: Some(f2.clone()),
                schedule_type: ScheduleType::Realtime,
                config: repo_schedules::ScheduleConfig {
                    quiet_hours: Some(repo_schedules::QuietHours {
                        start: "22:00".into(),
                        end: "23:00".into(),
                    }),
                    ..Default::default()
                },
                enabled: true,
                last_run_at: None,
                next_run_at: None,
            },
        )
        .await
        .expect("insert quiet realtime schedule");

        let quiet_file = root2.join("scope_quiet.txt");
        write_file(&quiet_file, b"quiet bytes");
        pipe.enqueue(&f2, scanned_file_for(&quiet_file, "documents"));
        let quiet_outcomes = pipe.drain_and_reconcile().await.expect("drain quiet");
        assert!(
            quiet_file.exists()
                && !matches!(
                    quiet_outcomes.as_slice(),
                    [PipelineStepOutcome::Organized(_)]
                ),
            "BUG-023b: realtime pipeline must pause during quiet_hours; got {quiet_outcomes:?}"
        );
    }
}
