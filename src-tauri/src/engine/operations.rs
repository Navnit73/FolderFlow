//! Operation state machine (`P4-T03`) and write-ahead ledger (`P4-T04`).
//!
//! Every mutating filesystem operation must record its intent in the SQLite
//! ledger (`PRAGMA synchronous = FULL`) and transition through `VALIDATING -> IN_PROGRESS`
//! to obtain a [`LedgerToken`] before calling any [`crate::engine::safe_fs::Fs`] mutation.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::database::repo_operations::{
    self, NewOperationRow, OperationBatchRow, OperationRow, OperationTransitionPatch,
};
use crate::database::Db;
use crate::error::{AppError, AppResult};

/// Operation status values matching `operations.status` in `0001_init.sql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OpStatus {
    Pending,
    Validating,
    InProgress,
    Verifying,
    Completed,
    Failed,
    RollbackRequired,
    RolledBack,
}

impl OpStatus {
    pub const ALL: [Self; 8] = [
        Self::Pending,
        Self::Validating,
        Self::InProgress,
        Self::Verifying,
        Self::Completed,
        Self::Failed,
        Self::RollbackRequired,
        Self::RolledBack,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Validating => "VALIDATING",
            Self::InProgress => "IN_PROGRESS",
            Self::Verifying => "VERIFYING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::RollbackRequired => "ROLLBACK_REQUIRED",
            Self::RolledBack => "ROLLED_BACK",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::RolledBack)
    }

    /// Returns `true` iff `self -> next` is a legal state transition (`P4-T03`).
    pub fn can_transition_to(self, next: Self) -> bool {
        match (self, next) {
            (Self::Pending, Self::Validating | Self::Failed | Self::RolledBack) => true,
            (Self::Validating, Self::InProgress | Self::Failed | Self::RolledBack) => true,
            (
                Self::InProgress,
                Self::Verifying
                | Self::Completed
                | Self::RollbackRequired
                | Self::RolledBack
                | Self::Failed,
            ) => true,
            (Self::Verifying, Self::Completed | Self::RollbackRequired | Self::Failed) => true,
            (Self::RollbackRequired, Self::RolledBack | Self::Failed) => true,
            (Self::Completed | Self::Failed | Self::RolledBack, _) => false,
            _ => false,
        }
    }

    /// Validates `self -> next`, returning `AppError::Validation` if illegal.
    pub fn validate_transition(self, next: Self) -> AppResult<()> {
        if self.can_transition_to(next) {
            Ok(())
        } else {
            Err(AppError::validation(
                "status",
                format!("illegal operation status transition: {self} -> {next}"),
            ))
        }
    }
}

impl fmt::Display for OpStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OpStatus {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PENDING" => Ok(Self::Pending),
            "VALIDATING" => Ok(Self::Validating),
            "IN_PROGRESS" => Ok(Self::InProgress),
            "VERIFYING" => Ok(Self::Verifying),
            "COMPLETED" => Ok(Self::Completed),
            "FAILED" => Ok(Self::Failed),
            "ROLLBACK_REQUIRED" => Ok(Self::RollbackRequired),
            "ROLLED_BACK" => Ok(Self::RolledBack),
            other => Err(AppError::validation(
                "status",
                format!("unknown operation status: {other}"),
            )),
        }
    }
}

/// Operation types matching `operations.operation_type` in `0001_init.sql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Move,
    Copy,
    Rename,
    Trash,
    UndoMove,
    UndoCopy,
    UndoRename,
}

impl OpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Copy => "copy",
            Self::Rename => "rename",
            Self::Trash => "trash",
            Self::UndoMove => "undo_move",
            Self::UndoCopy => "undo_copy",
            Self::UndoRename => "undo_rename",
        }
    }

    pub fn is_undo(self) -> bool {
        matches!(self, Self::UndoMove | Self::UndoCopy | Self::UndoRename)
    }
}

impl fmt::Display for OpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OpKind {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "move" => Ok(Self::Move),
            "copy" => Ok(Self::Copy),
            "rename" => Ok(Self::Rename),
            "trash" => Ok(Self::Trash),
            "undo_move" => Ok(Self::UndoMove),
            "undo_copy" => Ok(Self::UndoCopy),
            "undo_rename" => Ok(Self::UndoRename),
            other => Err(AppError::validation(
                "operationType",
                format!("unknown operation type: {other}"),
            )),
        }
    }
}

/// Batch triggers matching `operation_batches.trigger` in `0001_init.sql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BatchTrigger {
    Realtime,
    Schedule,
    Manual,
    Preview,
    Undo,
    Recovery,
}

impl BatchTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Realtime => "realtime",
            Self::Schedule => "schedule",
            Self::Manual => "manual",
            Self::Preview => "preview",
            Self::Undo => "undo",
            Self::Recovery => "recovery",
        }
    }
}

impl fmt::Display for BatchTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BatchTrigger {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "realtime" => Ok(Self::Realtime),
            "schedule" => Ok(Self::Schedule),
            "manual" => Ok(Self::Manual),
            "preview" => Ok(Self::Preview),
            "undo" => Ok(Self::Undo),
            "recovery" => Ok(Self::Recovery),
            other => Err(AppError::validation(
                "trigger",
                format!("unknown batch trigger: {other}"),
            )),
        }
    }
}

/// Unforgeable write-ahead proof that an operation has transitioned to `IN_PROGRESS`
/// in SQLite (`PRAGMA synchronous = FULL`) before any filesystem mutation (`SAFETY_MODEL.md` S4).
#[derive(Debug)]
pub struct LedgerToken {
    op_id: String,
}

impl LedgerToken {
    pub fn op_id(&self) -> &str {
        &self.op_id
    }
}

/// Write-ahead operation ledger (`P4-T04`).
#[derive(Debug, Clone)]
pub struct Ledger {
    db: Db,
}

impl Ledger {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub async fn create_batch(
        &self,
        id: &str,
        trigger: BatchTrigger,
        plan_id: Option<&str>,
        total: u32,
        started_at: i64,
    ) -> AppResult<OperationBatchRow> {
        repo_operations::create_batch(&self.db, id, trigger, plan_id, total, started_at).await
    }

    pub async fn complete_batch(&self, id: &str, completed_at: i64) -> AppResult<()> {
        repo_operations::complete_batch(&self.db, id, completed_at).await
    }

    pub async fn get_batch(&self, id: &str) -> AppResult<Option<OperationBatchRow>> {
        repo_operations::get_batch(&self.db, id).await
    }

    /// Writes a `PENDING` operation intent row with `synchronous = FULL`.
    pub async fn record_intent(&self, op: &NewOperationRow) -> AppResult<OperationRow> {
        repo_operations::insert_operation_intent(&self.db, op).await
    }

    /// Transitions an operation from `from` to `to` after checking `from.validate_transition(to)`.
    pub async fn transition(
        &self,
        id: &str,
        from: OpStatus,
        to: OpStatus,
        patch: OperationTransitionPatch,
    ) -> AppResult<OperationRow> {
        repo_operations::transition_operation(&self.db, id, from, to, &patch).await
    }

    /// Transitions `VALIDATING -> IN_PROGRESS` and mints a [`LedgerToken`] authorizing FS mutation.
    pub async fn begin_in_progress(
        &self,
        id: &str,
        patch: OperationTransitionPatch,
    ) -> AppResult<(OperationRow, LedgerToken)> {
        let row = self
            .transition(id, OpStatus::Validating, OpStatus::InProgress, patch)
            .await?;
        Ok((
            row,
            LedgerToken {
                op_id: id.to_owned(),
            },
        ))
    }

    pub async fn get(&self, id: &str) -> AppResult<Option<OperationRow>> {
        repo_operations::get_operation(&self.db, id).await
    }

    pub async fn list_by_batch(&self, batch_id: &str) -> AppResult<Vec<OperationRow>> {
        repo_operations::list_by_batch(&self.db, batch_id).await
    }

    pub async fn list_non_terminal(&self) -> AppResult<Vec<OperationRow>> {
        repo_operations::list_non_terminal(&self.db).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use proptest::prelude::*;

    use super::*;
    use crate::database;
    use crate::engine::safe_fs::fault::{FaultyFs, FsOpKind};
    use crate::engine::safe_fs::{Fs, SafeFs};
    use crate::test_support::{tempdir, write_file};

    fn sample_new_op(id: &str, batch_id: Option<&str>) -> NewOperationRow {
        NewOperationRow {
            id: id.to_owned(),
            batch_id: batch_id.map(str::to_owned),
            file_record_id: None,
            operation_type: OpKind::Move,
            source_path: "/tmp/src/a.pdf".to_owned(),
            destination_path: "/tmp/dst/a.pdf".to_owned(),
            staging_path: None,
            original_name: "a.pdf".to_owned(),
            new_name: "a.pdf".to_owned(),
            dev: Some(1),
            ino: Some(100),
            size: Some(1024),
            mtime_ns: Some(1_700_000_000_000_000_000),
            content_hash: None,
            cross_volume: false,
            rule_id: None,
            reversible: true,
            started_at: 1_700_000_000_000,
        }
    }

    #[test]
    fn r_ops_01_happy_path_transitions_succeed() {
        assert!(OpStatus::Pending.can_transition_to(OpStatus::Validating));
        assert!(OpStatus::Validating.can_transition_to(OpStatus::InProgress));
        assert!(OpStatus::InProgress.can_transition_to(OpStatus::Verifying));
        assert!(OpStatus::Verifying.can_transition_to(OpStatus::Completed));
        assert!(OpStatus::Completed.is_terminal());
    }

    #[test]
    fn r_ops_02_failure_and_rollback_transitions_succeed() {
        // Validation / pre-flight failures
        assert!(OpStatus::Pending.can_transition_to(OpStatus::Failed));
        assert!(OpStatus::Pending.can_transition_to(OpStatus::RolledBack));
        assert!(OpStatus::Validating.can_transition_to(OpStatus::Failed));
        assert!(OpStatus::Validating.can_transition_to(OpStatus::RolledBack));

        // Mid-flight failures & rollback
        assert!(OpStatus::InProgress.can_transition_to(OpStatus::RollbackRequired));
        assert!(OpStatus::InProgress.can_transition_to(OpStatus::Failed));
        assert!(OpStatus::InProgress.can_transition_to(OpStatus::RolledBack));
        assert!(OpStatus::InProgress.can_transition_to(OpStatus::Completed));

        // Verification failure -> RollbackRequired -> RolledBack
        assert!(OpStatus::Verifying.can_transition_to(OpStatus::RollbackRequired));
        assert!(OpStatus::RollbackRequired.can_transition_to(OpStatus::RolledBack));
        assert!(OpStatus::RollbackRequired.can_transition_to(OpStatus::Failed));
        assert!(OpStatus::Failed.is_terminal());
        assert!(OpStatus::RolledBack.is_terminal());
    }

    #[test]
    fn r_ops_03_illegal_transitions_and_terminal_outgoing_transitions_rejected() {
        // Terminal states cannot transition anywhere (including to themselves).
        for term in [OpStatus::Completed, OpStatus::Failed, OpStatus::RolledBack] {
            for target in OpStatus::ALL {
                assert!(
                    !term.can_transition_to(target),
                    "terminal state {term} must not transition to {target}"
                );
            }
        }
        // Cannot skip InProgress to reach Verifying or Completed from Pending/Validating.
        assert!(!OpStatus::Pending.can_transition_to(OpStatus::InProgress));
        assert!(!OpStatus::Pending.can_transition_to(OpStatus::Verifying));
        assert!(!OpStatus::Pending.can_transition_to(OpStatus::Completed));
        assert!(!OpStatus::Validating.can_transition_to(OpStatus::Verifying));
        assert!(!OpStatus::Validating.can_transition_to(OpStatus::Completed));
        // Cannot transition backwards.
        assert!(!OpStatus::InProgress.can_transition_to(OpStatus::Pending));
        assert!(!OpStatus::InProgress.can_transition_to(OpStatus::Validating));
        assert!(!OpStatus::Verifying.can_transition_to(OpStatus::InProgress));
    }

    #[tokio::test]
    async fn r_ops_04_db_rejects_illegal_state_transition_and_preserves_previous_state() {
        let db = database::open_in_memory().await.expect("open db");
        let ledger = Ledger::new(db);
        let op = ledger
            .record_intent(&sample_new_op("op-1", None))
            .await
            .expect("record intent");
        assert_eq!(op.status, OpStatus::Pending);

        // Illegal transition Pending -> Completed must be rejected.
        let err = ledger
            .transition(
                "op-1",
                OpStatus::Pending,
                OpStatus::Completed,
                OperationTransitionPatch::default(),
            )
            .await
            .expect_err("Pending -> Completed must fail");
        assert!(err
            .to_string()
            .contains("illegal operation status transition"));

        let fetched = ledger.get("op-1").await.expect("get").expect("exists");
        assert_eq!(fetched.status, OpStatus::Pending);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]
        #[test]
        fn r_ops_05_proptest_random_transition_sequences_never_reach_illegal_state(
            targets in proptest::collection::vec(0usize..8usize, 1..32)
        ) {
            prop_assert!(OpStatus::Pending.can_transition_to(OpStatus::Validating));
            let mut state = OpStatus::Pending;
            let mut visited_in_progress = false;
            let mut became_terminal: Option<OpStatus> = None;

            for idx in targets {
                let candidate = OpStatus::ALL[idx];
                if state.can_transition_to(candidate) {
                    // Once terminal, no further transition could ever be accepted.
                    prop_assert!(became_terminal.is_none(), "transitioned out of terminal state");
                    // No self-transitions allowed.
                    prop_assert_ne!(state, candidate, "self-transition accepted");
                    if candidate == OpStatus::InProgress {
                        visited_in_progress = true;
                    }
                    // Can never reach Verifying or Completed without first visiting InProgress.
                    if matches!(candidate, OpStatus::Verifying | OpStatus::Completed) {
                        prop_assert!(
                            visited_in_progress,
                            "reached {candidate} without visiting InProgress"
                        );
                    }
                    state = candidate;
                    if state.is_terminal() {
                        became_terminal = Some(state);
                    }
                } else if let Some(term) = became_terminal {
                    prop_assert_eq!(state, term);
                }
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn r_ledg_01_intent_is_committed_before_first_fs_mutation() {
        let (_d, root) = tempdir();
        let db_path = root.join("ledger.db");
        let db = database::open(&db_path).await.expect("open file db");
        let ledger = Ledger::new(db.clone());

        let src = root.join("src.txt");
        let dst = root.join("dst.txt");
        write_file(&src, b"ledger-first");

        let inner_fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let faulty = FaultyFs::new(inner_fs);
        let hook_verified = Arc::new(AtomicBool::new(false));
        let hook_verified_clone = Arc::clone(&hook_verified);
        let db_for_hook = db.clone();

        // FaultyFs hook fires synchronously BEFORE the underlying rename_excl executes.
        faulty.set_on_mutation(move |op_kind, step| {
            assert_eq!(step, 1);
            assert_eq!(op_kind, FsOpKind::RenameExcl);
            let row = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    repo_operations::get_operation(&db_for_hook, "op-ledg-1")
                        .await
                        .expect("query in hook")
                        .expect("ledger row must exist before FS mutation")
                })
            });
            assert_eq!(
                row.status,
                OpStatus::InProgress,
                "ledger row must be IN_PROGRESS before first FS mutation"
            );
            hook_verified_clone.store(true, Ordering::SeqCst);
        });

        let mut intent = sample_new_op("op-ledg-1", None);
        intent.source_path = src.to_string_lossy().into_owned();
        intent.destination_path = dst.to_string_lossy().into_owned();
        ledger.record_intent(&intent).await.expect("record intent");
        ledger
            .transition(
                "op-ledg-1",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("to validating");
        let (_row, _token) = ledger
            .begin_in_progress("op-ledg-1", OperationTransitionPatch::default())
            .await
            .expect("to in_progress");

        faulty.rename_excl(&src, &dst).expect("rename_excl");
        assert!(
            hook_verified.load(Ordering::SeqCst),
            "mutation hook must have verified ledger state"
        );
    }

    #[tokio::test]
    async fn r_ledg_02_synchronous_full_is_enforced_on_ledger_writes() {
        let (_d, root) = tempdir();
        let db = database::open(&root.join("sync.db"))
            .await
            .expect("open file db");
        let ledger = Ledger::new(db.clone());
        ledger
            .record_intent(&sample_new_op("op-sync", None))
            .await
            .expect("record intent");

        // SQLite PRAGMA synchronous returns 2 for FULL.
        let sync_val = repo_operations::query_synchronous_pragma(&db)
            .await
            .expect("query pragma");
        assert_eq!(
            sync_val, 2,
            "PRAGMA synchronous must be FULL (2) for ledger durability"
        );
    }

    #[tokio::test]
    async fn r_ledg_03_batch_groups_multiple_operations_and_tracks_summary_status() {
        let db = database::open_in_memory().await.expect("open db");
        let ledger = Ledger::new(db);

        let batch = ledger
            .create_batch("batch-1", BatchTrigger::Manual, Some("plan-9"), 2, 1000)
            .await
            .expect("create batch");
        assert_eq!(batch.total, 2);
        assert_eq!(batch.completed_at, None);

        ledger
            .record_intent(&sample_new_op("op-b1", Some("batch-1")))
            .await
            .expect("op 1");
        ledger
            .record_intent(&sample_new_op("op-b2", Some("batch-1")))
            .await
            .expect("op 2");

        let ops = ledger.list_by_batch("batch-1").await.expect("list batch");
        assert_eq!(ops.len(), 2);

        ledger
            .complete_batch("batch-1", 2500)
            .await
            .expect("complete batch");
        let updated = ledger
            .get_batch("batch-1")
            .await
            .expect("get batch")
            .expect("batch exists");
        assert_eq!(updated.completed_at, Some(2500));
    }

    #[tokio::test]
    async fn r_ledg_04_ledger_records_pre_and_post_metadata_hashes_and_error_details() {
        let db = database::open_in_memory().await.expect("open db");
        let ledger = Ledger::new(db);

        ledger
            .record_intent(&sample_new_op("op-meta", None))
            .await
            .expect("intent");
        ledger
            .transition(
                "op-meta",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("validating");
        let (_row, _token) = ledger
            .begin_in_progress(
                "op-meta",
                OperationTransitionPatch {
                    staging_path: Some(Some("/tmp/dst/.folderflow-op-meta.partial".into())),
                    cross_volume: Some(true),
                    ..Default::default()
                },
            )
            .await
            .expect("in_progress");

        let completed = ledger
            .transition(
                "op-meta",
                OpStatus::InProgress,
                OpStatus::Verifying,
                OperationTransitionPatch {
                    content_hash: Some("deadbeef".repeat(8)),
                    ..Default::default()
                },
            )
            .await
            .expect("verifying");
        assert_eq!(
            completed.staging_path.as_deref(),
            Some("/tmp/dst/.folderflow-op-meta.partial")
        );
        assert!(completed.cross_volume);
        assert_eq!(
            completed.content_hash.as_deref(),
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
        );

        // Now test failure metadata recording on a second op.
        ledger
            .record_intent(&sample_new_op("op-err", None))
            .await
            .expect("intent 2");
        let failed = ledger
            .transition(
                "op-err",
                OpStatus::Pending,
                OpStatus::Failed,
                OperationTransitionPatch {
                    error_code: Some(Some("enospc".into())),
                    error_message: Some(Some("No space left on device".into())),
                    completed_at: Some(9999),
                    ..Default::default()
                },
            )
            .await
            .expect("failed");
        assert_eq!(failed.status, OpStatus::Failed);
        assert_eq!(failed.error_code.as_deref(), Some("enospc"));
        assert_eq!(
            failed.error_message.as_deref(),
            Some("No space left on device")
        );
        assert_eq!(failed.completed_at, Some(9999));
    }
}
