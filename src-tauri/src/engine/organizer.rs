//! Transactional Move, Copy, and Rename execution pipelines (`P4-T05`, `P4-T06`, `P4-T07`).
//!
//! Enforces `SAFETY_MODEL.md` invariants S1–S10:
//! - Write-ahead ledger intent (`PENDING -> VALIDATING -> IN_PROGRESS`) before any FS mutation.
//! - Pre-flight path authorization (`PathAuthority`) and source identity verification (`S6`).
//! - Same-volume atomic move/rename via `rename_excl` (`RENAME_EXCL`).
//! - Cross-volume staged move via `.folderflow-<opid>.partial` -> `copy_excl` (`fsync`) ->
//!   `verify_copied` (BLAKE3 + size + source identity) -> `rename_excl` -> `remove_verified_source`.
//! - Any failure prior to final destination placement rolls back staging to Trash and leaves
//!   the source intact.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::database::repo_operations::{
    self, NewOperationRow, OperationRow, OperationTransitionPatch, ReviewItemView,
    ReviewProposedAction,
};
use crate::engine::conflicts::{claim_with_rename_excl, ConflictClaim, ConflictPolicy};
use crate::engine::operations::{Ledger, LedgerToken, OpKind, OpStatus};
use crate::engine::paths::{nfc, PathAuthority};
use crate::engine::safe_fs::{EntryIdentity, Fs};
use crate::error::{AppError, AppResult};
use crate::utils::clock::Clock;

/// Parameters for a single ledgered file organization operation.
#[derive(Debug, Clone)]
pub struct OrganizeRequest {
    pub op_id: Option<String>,
    pub batch_id: Option<String>,
    pub file_record_id: Option<String>,
    pub rule_id: Option<String>,
    pub kind: OpKind,
    pub source: PathBuf,
    pub destination: PathBuf,
    /// Optional snapshot from planning/discovery. When set, execution aborts in `VALIDATING`
    /// if the source file's `(dev, ino, size, mtime_ns)` changed (`SAFETY_MODEL.md` S6).
    pub expected_source: Option<EntryIdentity>,
    pub conflict_policy: ConflictPolicy,
    pub reversible: bool,
}

impl OrganizeRequest {
    pub fn new(kind: OpKind, source: PathBuf, destination: PathBuf) -> Self {
        Self {
            op_id: None,
            batch_id: None,
            file_record_id: None,
            rule_id: None,
            kind,
            source,
            destination,
            expected_source: None,
            conflict_policy: ConflictPolicy::AutoRename,
            reversible: true,
        }
    }
}

/// Outcome of executing an [`OrganizeRequest`].
#[derive(Debug, Clone)]
pub enum OrganizeOutcome {
    /// Operation completed and verified; returns the final `COMPLETED` ledger row.
    Completed(OperationRow),
    /// Destination existed and conflict policy was `Skip`; source untouched, row `ROLLED_BACK`.
    SkippedConflict(OperationRow),
    /// Destination existed and conflict policy was `Ask`; source untouched, review item queued.
    QueuedForReview {
        operation: Box<OperationRow>,
        review: ReviewItemView,
    },
    /// Source and destination resolved to the exact same path; safe no-op.
    NoOpSamePath,
}

impl OrganizeOutcome {
    pub fn into_completed(self) -> Option<OperationRow> {
        match self {
            Self::Completed(row) => Some(row),
            _ => None,
        }
    }
}

/// Executes ledgered move, copy, and rename operations over an injected [`Fs`] and [`Clock`].
#[derive(Clone)]
pub struct Organizer {
    ledger: Ledger,
    fs: Arc<dyn Fs>,
    clock: Arc<dyn Clock>,
}

impl Organizer {
    pub fn new(ledger: Ledger, fs: Arc<dyn Fs>, clock: Arc<dyn Clock>) -> Self {
        Self { ledger, fs, clock }
    }

    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    pub fn fs(&self) -> &Arc<dyn Fs> {
        &self.fs
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub async fn execute(
        &self,
        req: OrganizeRequest,
        authority: &PathAuthority,
    ) -> AppResult<OrganizeOutcome> {
        let src_nfc = nfc(&req.source);
        let dst_nfc = nfc(&req.destination);

        // Safe no-op when moving/renaming to the exact same NFC path.
        if src_nfc == dst_nfc
            && matches!(
                req.kind,
                OpKind::Move | OpKind::Rename | OpKind::UndoMove | OpKind::UndoRename
            )
        {
            let _ = authority.authorize(&src_nfc)?;
            let _ = self.fs.lstat(&src_nfc)?;
            return Ok(OrganizeOutcome::NoOpSamePath);
        }

        let op_id = req.op_id.unwrap_or_else(|| Uuid::now_v7().to_string());
        let started_at = self.clock.now_ms();
        let original_name = file_name_string(&src_nfc);
        let desired_new_name = file_name_string(&dst_nfc);

        // 1. Write-ahead intent in PENDING state (`PRAGMA synchronous = FULL`).
        let intent = NewOperationRow {
            id: op_id.clone(),
            batch_id: req.batch_id.clone(),
            file_record_id: req.file_record_id.clone(),
            operation_type: req.kind,
            source_path: src_nfc.to_string_lossy().into_owned(),
            destination_path: dst_nfc.to_string_lossy().into_owned(),
            staging_path: None,
            original_name: original_name.clone(),
            new_name: desired_new_name.clone(),
            dev: None,
            ino: None,
            size: None,
            mtime_ns: None,
            content_hash: None,
            cross_volume: false,
            rule_id: req.rule_id.clone(),
            reversible: req.reversible,
            started_at,
        };
        self.ledger.record_intent(&intent).await?;

        // 2. Enter VALIDATING state and run pre-flight checks.
        self.ledger
            .transition(
                &op_id,
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await?;

        let (src_auth, dst_auth, src_identity) =
            match self.preflight(&src_nfc, &dst_nfc, req.expected_source.as_ref(), authority) {
                Ok(validated) => validated,
                Err((code, err)) => {
                    let _ = self
                        .ledger
                        .transition(
                            &op_id,
                            OpStatus::Validating,
                            OpStatus::Failed,
                            OperationTransitionPatch {
                                error_code: Some(Some(code.into())),
                                error_message: Some(Some(err.to_string())),
                                completed_at: Some(self.clock.now_ms()),
                                ..Default::default()
                            },
                        )
                        .await;
                    return Err(err);
                }
            };

        let src_path = src_auth.path;
        let dst_path = dst_auth.path;

        // 3. Transition VALIDATING -> IN_PROGRESS (`synchronous = FULL`), minting `LedgerToken`
        //    BEFORE any mutating `Fs` call executes (`SAFETY_MODEL.md` S4).
        let (_in_prog_row, token) = self
            .ledger
            .begin_in_progress(
                &op_id,
                OperationTransitionPatch {
                    dev: i64::try_from(src_identity.dev).ok(),
                    ino: i64::try_from(src_identity.ino).ok(),
                    size: i64::try_from(src_identity.size).ok(),
                    mtime_ns: Some(src_identity.mtime_ns),
                    ..Default::default()
                },
            )
            .await?;

        match req.kind {
            OpKind::Move | OpKind::Rename | OpKind::UndoMove | OpKind::UndoRename => {
                self.execute_move_or_rename(
                    &token,
                    &req.kind,
                    req.rule_id.as_deref(),
                    req.conflict_policy,
                    &src_path,
                    &dst_path,
                    &src_identity,
                )
                .await
            }
            OpKind::Copy => {
                self.execute_staged(
                    &token,
                    &OpKind::Copy,
                    req.rule_id.as_deref(),
                    req.conflict_policy,
                    &src_path,
                    &dst_path,
                    &src_identity,
                    false,
                )
                .await
            }
            OpKind::Trash | OpKind::UndoCopy => self.execute_trash(&token, &src_path).await,
        }
    }

    fn preflight(
        &self,
        src: &Path,
        dst: &Path,
        expected_source: Option<&EntryIdentity>,
        authority: &PathAuthority,
    ) -> Result<
        (
            crate::engine::paths::AuthorizedPath,
            crate::engine::paths::AuthorizedPath,
            EntryIdentity,
        ),
        (&'static str, AppError),
    > {
        let src_auth = authority
            .authorize(src)
            .map_err(|e| ("path_unauthorized", AppError::Path(e)))?;
        let dst_auth = authority
            .authorize(dst)
            .map_err(|e| ("path_unauthorized", AppError::Path(e)))?;

        self.fs
            .ensure_no_symlink_ancestors(&dst_auth.path, &dst_auth.root)
            .map_err(|e| ("symlink_ancestor", AppError::Io(e)))?;

        let src_identity = self
            .fs
            .lstat(&src_auth.path)
            .map_err(|e| (io_error_code(&e), AppError::Io(e)))?;

        if let Some(expected) = expected_source {
            if !src_identity.same_version(expected) {
                return Err((
                    "stale_identity",
                    AppError::validation(
                        "source",
                        "source file changed since the plan was created",
                    ),
                ));
            }
        }

        Ok((src_auth, dst_auth, src_identity))
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_move_or_rename(
        &self,
        token: &LedgerToken,
        kind: &OpKind,
        rule_id: Option<&str>,
        conflict_policy: ConflictPolicy,
        src: &Path,
        dst: &Path,
        src_identity: &EntryIdentity,
    ) -> AppResult<OrganizeOutcome> {
        let op_id = token.op_id();
        let dst_parent = dst.parent().unwrap_or_else(|| Path::new("/"));

        if let Err(e) = self.fs.mkdir_p(dst_parent) {
            let _ = self
                .fail_from(
                    op_id,
                    OpStatus::InProgress,
                    io_error_code(&e),
                    &e.to_string(),
                )
                .await;
            return Err(AppError::Io(e));
        }

        // Try same-volume exclusive atomic rename first.
        match claim_with_rename_excl(self.fs.as_ref(), src, dst, conflict_policy) {
            Ok(ConflictClaim::Claimed { final_path, .. }) => {
                let final_name = file_name_string(&final_path);
                let final_str = final_path.to_string_lossy().into_owned();
                self.ledger
                    .transition(
                        op_id,
                        OpStatus::InProgress,
                        OpStatus::Verifying,
                        OperationTransitionPatch {
                            destination_path: Some(final_str.clone()),
                            new_name: Some(final_name),
                            ..Default::default()
                        },
                    )
                    .await?;

                let post_stat = match self.fs.lstat(&final_path) {
                    Ok(stat) if stat.ino == src_identity.ino && stat.size == src_identity.size => {
                        stat
                    }
                    Ok(_) => {
                        let msg = "post-move inode or size mismatch";
                        let _ = self
                            .fail_from(op_id, OpStatus::Verifying, "verify_failed", msg)
                            .await;
                        return Err(AppError::Internal(msg.into()));
                    }
                    Err(e) => {
                        let _ = self
                            .fail_from(
                                op_id,
                                OpStatus::Verifying,
                                io_error_code(&e),
                                &e.to_string(),
                            )
                            .await;
                        return Err(AppError::Io(e));
                    }
                };

                let completed = self
                    .ledger
                    .transition(
                        op_id,
                        OpStatus::Verifying,
                        OpStatus::Completed,
                        OperationTransitionPatch {
                            dev: i64::try_from(post_stat.dev).ok(),
                            ino: i64::try_from(post_stat.ino).ok(),
                            size: i64::try_from(post_stat.size).ok(),
                            mtime_ns: Some(post_stat.mtime_ns),
                            completed_at: Some(self.clock.now_ms()),
                            ..Default::default()
                        },
                    )
                    .await?;
                Ok(OrganizeOutcome::Completed(completed))
            }
            Ok(ConflictClaim::Skipped { .. }) => {
                let row = self
                    .ledger
                    .transition(
                        op_id,
                        OpStatus::InProgress,
                        OpStatus::RolledBack,
                        OperationTransitionPatch {
                            error_code: Some(Some("conflict_skipped".into())),
                            error_message: Some(Some(
                                "Destination already exists; skipped per conflict policy.".into(),
                            )),
                            completed_at: Some(self.clock.now_ms()),
                            ..Default::default()
                        },
                    )
                    .await?;
                Ok(OrganizeOutcome::SkippedConflict(row))
            }
            Ok(ConflictClaim::AskUser { .. }) => {
                self.enqueue_conflict_review(op_id, kind, rule_id, src, dst)
                    .await
            }
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Cross-volume fallback: staged copy -> fsync -> verify -> rename_excl -> remove_verified_source.
                self.execute_staged(
                    token,
                    kind,
                    rule_id,
                    conflict_policy,
                    src,
                    dst,
                    src_identity,
                    true,
                )
                .await
            }
            Err(e) => {
                let _ = self
                    .fail_from(
                        op_id,
                        OpStatus::InProgress,
                        io_error_code(&e),
                        &e.to_string(),
                    )
                    .await;
                Err(AppError::Io(e))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_staged(
        &self,
        token: &LedgerToken,
        kind: &OpKind,
        rule_id: Option<&str>,
        conflict_policy: ConflictPolicy,
        src: &Path,
        dst: &Path,
        src_identity: &EntryIdentity,
        remove_source_after_verify: bool,
    ) -> AppResult<OrganizeOutcome> {
        let op_id = token.op_id();
        let dst_parent = dst.parent().unwrap_or_else(|| Path::new("/"));

        if let Err(e) = self.fs.mkdir_p(dst_parent) {
            let _ = self
                .fail_from(
                    op_id,
                    OpStatus::InProgress,
                    io_error_code(&e),
                    &e.to_string(),
                )
                .await;
            return Err(AppError::Io(e));
        }

        // Under Skip or Ask policies, check before copying if destination is already occupied.
        if matches!(conflict_policy, ConflictPolicy::Skip | ConflictPolicy::Ask)
            && self.fs.exists_no_follow(dst)
        {
            return match conflict_policy {
                ConflictPolicy::Skip => {
                    let row = self
                        .ledger
                        .transition(
                            op_id,
                            OpStatus::InProgress,
                            OpStatus::RolledBack,
                            OperationTransitionPatch {
                                error_code: Some(Some("conflict_skipped".into())),
                                error_message: Some(Some(
                                    "Destination already exists; skipped per conflict policy."
                                        .into(),
                                )),
                                completed_at: Some(self.clock.now_ms()),
                                ..Default::default()
                            },
                        )
                        .await?;
                    Ok(OrganizeOutcome::SkippedConflict(row))
                }
                ConflictPolicy::Ask => {
                    self.enqueue_conflict_review(op_id, kind, rule_id, src, dst)
                        .await
                }
                _ => unreachable!(),
            };
        }

        // Check destination free space >= 2x source size (`SAFETY_MODEL.md` § Cross-volume move step 2).
        let required_free = src_identity.size.saturating_mul(2);
        match self.fs.stat_fs_free_bytes(dst_parent) {
            Ok(free) if free < required_free => {
                let msg = format!(
                    "insufficient free space on destination volume ({free} B available, {required_free} B required)"
                );
                let _ = self
                    .fail_from(op_id, OpStatus::InProgress, "enospc", &msg)
                    .await;
                return Err(AppError::Io(std::io::Error::from_raw_os_error(
                    libc::ENOSPC,
                )));
            }
            Ok(_) => {}
            Err(e) => {
                let _ = self
                    .fail_from(
                        op_id,
                        OpStatus::InProgress,
                        io_error_code(&e),
                        &e.to_string(),
                    )
                    .await;
                return Err(AppError::Io(e));
            }
        }

        let staging = staging_path_for(dst, op_id);
        let staging_str = staging.to_string_lossy().into_owned();

        // Stream copy to `.folderflow-<op_id>.partial` (`O_CREAT | O_EXCL`, 64 KiB, BLAKE3, fsync).
        let copied = match self.fs.copy_excl(src, &staging) {
            Ok(c) => c,
            Err(e) => {
                self.rollback_staging(
                    op_id,
                    OpStatus::InProgress,
                    &staging,
                    io_error_code(&e),
                    &e.to_string(),
                )
                .await;
                return Err(AppError::Io(e));
            }
        };

        let hash_hex = copied.blake3.to_hex();
        self.ledger
            .transition(
                op_id,
                OpStatus::InProgress,
                OpStatus::Verifying,
                OperationTransitionPatch {
                    staging_path: Some(Some(staging_str)),
                    content_hash: Some(hash_hex),
                    cross_volume: Some(remove_source_after_verify),
                    ..Default::default()
                },
            )
            .await?;

        // Re-verify destination size + BLAKE3 and confirm source identity has not drifted.
        let verified_token = match self.fs.verify_copied(src, src_identity, &staging, &copied) {
            Ok(tok) => tok,
            Err(e) => {
                self.rollback_staging(
                    op_id,
                    OpStatus::Verifying,
                    &staging,
                    "verify_failed",
                    &e.to_string(),
                )
                .await;
                return Err(AppError::Io(e));
            }
        };

        // Atomically claim the final destination path from `staging` via `rename_excl`.
        let final_path =
            match claim_with_rename_excl(self.fs.as_ref(), &staging, dst, conflict_policy) {
                Ok(ConflictClaim::Claimed { final_path, .. }) => final_path,
                Ok(ConflictClaim::Skipped { .. }) => {
                    self.rollback_staging(
                        op_id,
                        OpStatus::Verifying,
                        &staging,
                        "conflict_skipped",
                        "Destination already exists; skipped per conflict policy.",
                    )
                    .await;
                    let row = self
                        .ledger
                        .get(op_id)
                        .await?
                        .ok_or_else(|| AppError::NotFound(format!("Operation {op_id}")))?;
                    return Ok(OrganizeOutcome::SkippedConflict(row));
                }
                Ok(ConflictClaim::AskUser { .. }) => {
                    if self.fs.exists_no_follow(&staging) {
                        let _ = self.fs.move_to_trash(&staging);
                    }
                    let _ = self
                        .ledger
                        .transition(
                            op_id,
                            OpStatus::Verifying,
                            OpStatus::RollbackRequired,
                            OperationTransitionPatch::default(),
                        )
                        .await;
                    return self
                        .enqueue_conflict_review_from_state(
                            op_id,
                            OpStatus::RollbackRequired,
                            kind,
                            rule_id,
                            src,
                            dst,
                        )
                        .await;
                }
                Err(e) => {
                    self.rollback_staging(
                        op_id,
                        OpStatus::Verifying,
                        &staging,
                        io_error_code(&e),
                        &e.to_string(),
                    )
                    .await;
                    return Err(AppError::Io(e));
                }
            };

        let final_name = file_name_string(&final_path);
        let final_str = final_path.to_string_lossy().into_owned();

        // Persist the claimed `final_path` and clear `staging_path` while still in `VERIFYING`
        // BEFORE unlinking the verified source (`BUG-018`), so startup recovery inspects the
        // actual auto-renamed destination path if `remove_verified_source` faults or crashes.
        repo_operations::update_verifying_destination(
            self.ledger.db(),
            op_id,
            &final_str,
            &final_name,
        )
        .await?;

        // For cross-volume move, unlink verified source ONLY after final_path is in place!
        if remove_source_after_verify {
            if let Err(e) = self.fs.remove_verified_source(src, verified_token) {
                // Destination is already verified and in place; leave row in VERIFYING
                // (with staging_path cleared and destination_path updated) so startup
                // recovery can finish unlinking the verified source without losing data!
                return Err(AppError::Io(e));
            }
        }

        let post_stat = self.fs.lstat(&final_path).ok();

        let completed = self
            .ledger
            .transition(
                op_id,
                OpStatus::Verifying,
                OpStatus::Completed,
                OperationTransitionPatch {
                    destination_path: Some(final_str),
                    new_name: Some(final_name),
                    staging_path: Some(None),
                    dev: post_stat.as_ref().and_then(|s| i64::try_from(s.dev).ok()),
                    ino: post_stat.as_ref().and_then(|s| i64::try_from(s.ino).ok()),
                    size: post_stat.as_ref().and_then(|s| i64::try_from(s.size).ok()),
                    mtime_ns: post_stat.as_ref().map(|s| s.mtime_ns),
                    completed_at: Some(self.clock.now_ms()),
                    ..Default::default()
                },
            )
            .await?;

        Ok(OrganizeOutcome::Completed(completed))
    }

    async fn execute_trash(&self, token: &LedgerToken, src: &Path) -> AppResult<OrganizeOutcome> {
        let op_id = token.op_id();
        let trash_path = match self.fs.move_to_trash(src) {
            Ok(p) => p,
            Err(e) => {
                let _ = self
                    .fail_from(
                        op_id,
                        OpStatus::InProgress,
                        io_error_code(&e),
                        &e.to_string(),
                    )
                    .await;
                return Err(AppError::Io(e));
            }
        };

        let trash_str = trash_path.to_string_lossy().into_owned();
        let trash_name = file_name_string(&trash_path);
        self.ledger
            .transition(
                op_id,
                OpStatus::InProgress,
                OpStatus::Verifying,
                OperationTransitionPatch {
                    destination_path: Some(trash_str),
                    new_name: Some(trash_name),
                    ..Default::default()
                },
            )
            .await?;

        let completed = self
            .ledger
            .transition(
                op_id,
                OpStatus::Verifying,
                OpStatus::Completed,
                OperationTransitionPatch {
                    completed_at: Some(self.clock.now_ms()),
                    ..Default::default()
                },
            )
            .await?;
        Ok(OrganizeOutcome::Completed(completed))
    }

    async fn rollback_staging(
        &self,
        op_id: &str,
        from_status: OpStatus,
        staging: &Path,
        code: &str,
        message: &str,
    ) {
        let _ = self
            .ledger
            .transition(
                op_id,
                from_status,
                OpStatus::RollbackRequired,
                OperationTransitionPatch {
                    error_code: Some(Some(code.to_owned())),
                    error_message: Some(Some(message.to_owned())),
                    ..Default::default()
                },
            )
            .await;

        if self.fs.exists_no_follow(staging) {
            let _ = self.fs.move_to_trash(staging);
        }

        let _ = self
            .ledger
            .transition(
                op_id,
                OpStatus::RollbackRequired,
                OpStatus::RolledBack,
                OperationTransitionPatch {
                    staging_path: Some(None),
                    error_code: Some(Some(code.to_owned())),
                    error_message: Some(Some(message.to_owned())),
                    completed_at: Some(self.clock.now_ms()),
                    ..Default::default()
                },
            )
            .await;
    }

    async fn fail_from(
        &self,
        op_id: &str,
        from_status: OpStatus,
        code: &str,
        message: &str,
    ) -> AppResult<OperationRow> {
        self.ledger
            .transition(
                op_id,
                from_status,
                OpStatus::Failed,
                OperationTransitionPatch {
                    error_code: Some(Some(code.to_owned())),
                    error_message: Some(Some(message.to_owned())),
                    completed_at: Some(self.clock.now_ms()),
                    ..Default::default()
                },
            )
            .await
    }

    async fn enqueue_conflict_review(
        &self,
        op_id: &str,
        kind: &OpKind,
        rule_id: Option<&str>,
        src: &Path,
        dst: &Path,
    ) -> AppResult<OrganizeOutcome> {
        self.enqueue_conflict_review_from_state(
            op_id,
            OpStatus::InProgress,
            kind,
            rule_id,
            src,
            dst,
        )
        .await
    }

    async fn enqueue_conflict_review_from_state(
        &self,
        op_id: &str,
        from_status: OpStatus,
        kind: &OpKind,
        rule_id: Option<&str>,
        src: &Path,
        dst: &Path,
    ) -> AppResult<OrganizeOutcome> {
        let now = self.clock.now_ms();
        let proposed = ReviewProposedAction {
            operation_type: *kind,
            source_path: src.to_string_lossy().into_owned(),
            destination_path: dst.to_string_lossy().into_owned(),
            original_name: file_name_string(src),
            new_name: file_name_string(dst),
        };
        let review = repo_operations::enqueue_review(
            self.ledger.db(),
            &Uuid::now_v7().to_string(),
            &proposed.source_path,
            "destination_exists",
            rule_id,
            Some(&proposed),
            now,
        )
        .await?;

        let operation = self
            .ledger
            .transition(
                op_id,
                from_status,
                OpStatus::RolledBack,
                OperationTransitionPatch {
                    staging_path: Some(None),
                    error_code: Some(Some("review_queued".into())),
                    error_message: Some(Some(
                        "Destination already exists; queued for user review.".into(),
                    )),
                    completed_at: Some(now),
                    ..Default::default()
                },
            )
            .await?;

        Ok(OrganizeOutcome::QueuedForReview {
            operation: Box::new(operation),
            review,
        })
    }
}

/// Returns the staging file path `<dest_parent>/.folderflow-<op_id>.partial`.
pub fn staging_path_for(dest: &Path, op_id: &str) -> PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("/"));
    parent.join(format!(".folderflow-{op_id}.partial"))
}

pub(crate) fn file_name_string(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(crate) fn io_error_code(err: &std::io::Error) -> &'static str {
    match err.raw_os_error() {
        Some(libc::ENOSPC) => "enospc",
        Some(libc::EACCES | libc::EPERM) => "permission_denied",
        Some(libc::ENOENT) => "not_found",
        Some(libc::EEXIST) => "already_exists",
        Some(libc::EXDEV) => "cross_volume",
        _ => match err.kind() {
            ErrorKind::PermissionDenied => "permission_denied",
            ErrorKind::NotFound => "not_found",
            ErrorKind::AlreadyExists => "already_exists",
            _ => "io_error",
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use proptest::prelude::*;

    use super::*;
    use crate::database::{self, repo_operations};
    use crate::engine::conflicts::{candidate_path, ConflictPolicy};
    use crate::engine::operations::OpStatus;
    use crate::engine::paths::{ApprovedRoot, PathAuthority, RootKind};
    use crate::engine::safe_fs::fault::{FaultKind, FaultyFs, FsOpKind};
    use crate::engine::safe_fs::{Blake3Hash, Fs, SafeFs};
    use crate::test_support::{mkdir, tempdir, write_file};
    use crate::utils::clock::FixedClock;

    fn make_authority(roots: &[&Path]) -> PathAuthority {
        PathAuthority::new(
            roots
                .iter()
                .map(|p| ApprovedRoot {
                    path: (*p).to_path_buf(),
                    kind: RootKind::Monitored,
                })
                .collect(),
        )
    }

    async fn setup_organizer(
        root: &Path,
    ) -> (Organizer, Arc<FaultyFs>, PathAuthority, database::Db) {
        let db = database::open_in_memory().await.expect("open db");
        let ledger = Ledger::new(db.clone());
        let safe_fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let faulty = Arc::new(FaultyFs::new(safe_fs));
        let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
        let organizer = Organizer::new(ledger, faulty.clone(), clock);
        let auth = make_authority(&[root]);
        (organizer, faulty, auth, db)
    }

    // -------------------------------------------------------------------------
    // P4-T05: Move pipeline tests (R-MOVE-01..08)
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn r_move_01_same_volume_atomic_move_preserves_inode_and_records_ledger() {
        let (_d, root) = tempdir();
        let (org, fs, auth, _db) = setup_organizer(&root).await;

        let src = root.join("Downloads/invoice.pdf");
        let dst = root.join("Documents/Invoices/invoice.pdf");
        write_file(&src, b"same-volume-content");
        let pre = fs.lstat(&src).expect("lstat src");

        let outcome = org
            .execute(
                OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("move succeeds");
        let row = outcome.into_completed().expect("completed");

        assert_eq!(row.status, OpStatus::Completed);
        assert!(!row.cross_volume);
        assert!(!fs.exists_no_follow(&src));
        let post = fs.lstat(&dst).expect("lstat dst");
        assert_eq!(pre.ino, post.ino, "same-volume atomic move preserves inode");
        assert_eq!(
            std::fs::read(&dst).expect("read dst"),
            b"same-volume-content"
        );
    }

    #[tokio::test]
    async fn r_move_02_cross_volume_staged_move_verifies_blake3_and_removes_source() {
        let (_d, root) = tempdir();
        let vol_a = root.join("vol_a");
        let vol_b = root.join("vol_b");
        mkdir(&vol_a);
        mkdir(&vol_b);
        let (org, faulty, auth, _db) = setup_organizer(&root).await;
        faulty.simulate_cross_volume(vol_a.clone(), vol_b.clone());

        let src = vol_a.join("archive.zip");
        let dst = vol_b.join("Backups/archive.zip");
        let payload = vec![0xABu8; 90_000];
        write_file(&src, &payload);
        let expected_hash = Blake3Hash(*blake3::hash(&payload).as_bytes()).to_hex();

        let outcome = org
            .execute(
                OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("cross-volume move succeeds");
        let row = outcome.into_completed().expect("completed");

        assert_eq!(row.status, OpStatus::Completed);
        assert!(row.cross_volume);
        assert_eq!(row.content_hash.as_deref(), Some(expected_hash.as_str()));
        assert!(
            !faulty.exists_no_follow(&src),
            "verified source must be removed"
        );
        assert_eq!(std::fs::read(&dst).expect("read dst"), payload);
    }

    #[tokio::test]
    async fn r_move_03_cross_volume_failure_mid_copy_rolls_back_staging_and_keeps_source() {
        let (_d, root) = tempdir();
        let vol_a = root.join("vol_a");
        let vol_b = root.join("vol_b");
        mkdir(&vol_a);
        mkdir(&vol_b);
        let (org, faulty, auth, _db) = setup_organizer(&root).await;
        faulty.simulate_cross_volume(vol_a.clone(), vol_b.clone());

        let src = vol_a.join("photo.raw");
        let dst = vol_b.join("Photos/photo.raw");
        write_file(&src, &[9u8; 16_384]);

        faulty.fail_next_op(
            FsOpKind::CopyExcl,
            FaultKind::PartialCopy {
                bytes: 1024,
                errno: libc::ENOSPC,
            },
        );

        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.op_id = Some("op-mid-copy".into());
        let err = org
            .execute(req, &auth)
            .await
            .expect_err("mid-copy ENOSPC must fail");
        assert!(err.to_string().contains("space") || err.to_string().contains("io"));

        // Source must be untouched; staging file must be moved to Trash; destination absent.
        assert_eq!(std::fs::read(&src).expect("src intact"), vec![9u8; 16_384]);
        assert!(!faulty.exists_no_follow(&dst));
        let staged = staging_path_for(&dst, "op-mid-copy");
        assert!(
            !faulty.exists_no_follow(&staged),
            "staging file must be cleaned up to Trash on rollback"
        );

        let row = org
            .ledger()
            .get("op-mid-copy")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.status, OpStatus::RolledBack);
    }

    #[tokio::test]
    async fn r_move_04_cross_volume_source_modified_during_copy_aborts_and_rolls_back() {
        let (_d, root) = tempdir();
        let vol_a = root.join("vol_a");
        let vol_b = root.join("vol_b");
        mkdir(&vol_a);
        mkdir(&vol_b);
        let (org, faulty, auth, _db) = setup_organizer(&root).await;
        faulty.simulate_cross_volume(vol_a.clone(), vol_b.clone());

        let src = vol_a.join("live_doc.txt");
        let dst = vol_b.join("live_doc.txt");
        write_file(&src, b"version-1");

        // Mutate the source file right when VerifyCopied runs so verification detects source drift!
        let src_for_hook = src.clone();
        faulty.set_on_mutation(move |op, _step| {
            if op == FsOpKind::VerifyCopied {
                write_file(&src_for_hook, b"version-2-modified-while-copying");
            }
        });

        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.op_id = Some("op-src-drift".into());
        assert!(
            org.execute(req, &auth).await.is_err(),
            "must abort when source changes during cross-volume copy"
        );

        // Source must still hold version-2 (never unlinked!), and destination must not exist.
        assert_eq!(
            std::fs::read(&src).expect("read src"),
            b"version-2-modified-while-copying"
        );
        assert!(!faulty.exists_no_follow(&dst));
        let row = org
            .ledger()
            .get("op-src-drift")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.status, OpStatus::RolledBack);
    }

    #[tokio::test]
    async fn r_move_05_move_creates_missing_destination_subdirectories() {
        let (_d, root) = tempdir();
        let (org, fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("report.pdf");
        let dst = root.join("2026/11/Finance/Reports/report.pdf");
        write_file(&src, b"deep-nested");

        let outcome = org
            .execute(
                OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("move creates nested dirs");
        assert!(outcome.into_completed().is_some());
        assert!(!fs.exists_no_follow(&src));
        assert_eq!(std::fs::read(&dst).expect("read dst"), b"deep-nested");
    }

    #[tokio::test]
    async fn r_move_06_move_aborts_when_source_identity_changed_since_plan() {
        let (_d, root) = tempdir();
        let (org, fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("draft.txt");
        let dst = root.join("Archive/draft.txt");
        write_file(&src, b"planned-bytes");
        let planned_id = fs.lstat(&src).expect("lstat planned");

        // User edits the file after plan creation, before execution.
        write_file(&src, b"edited-after-plan-creation");

        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.op_id = Some("op-stale".into());
        req.expected_source = Some(planned_id);

        assert!(
            org.execute(req, &auth).await.is_err(),
            "stale source identity must abort before touching disk"
        );
        assert_eq!(
            std::fs::read(&src).expect("src untouched"),
            b"edited-after-plan-creation"
        );
        assert!(!fs.exists_no_follow(&dst));

        let row = org
            .ledger()
            .get("op-stale")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.status, OpStatus::Failed);
        assert_eq!(row.error_code.as_deref(), Some("stale_identity"));
    }

    #[tokio::test]
    async fn r_move_07_move_aborts_when_destination_volume_lacks_free_space() {
        let (_d, root) = tempdir();
        let vol_a = root.join("vol_a");
        let vol_b = root.join("vol_b");
        mkdir(&vol_a);
        mkdir(&vol_b);
        let (org, faulty, auth, _db) = setup_organizer(&root).await;
        faulty.simulate_cross_volume(vol_a.clone(), vol_b.clone());
        // Report only 100 bytes free on destination volume.
        faulty.set_free_bytes(Some(100));

        let src = vol_a.join("big.iso");
        let dst = vol_b.join("big.iso");
        write_file(&src, &[1u8; 256]);

        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.op_id = Some("op-nospace".into());
        assert!(org.execute(req, &auth).await.is_err());
        assert!(faulty.exists_no_follow(&src));
        assert!(!faulty.exists_no_follow(&dst));

        let row = org
            .ledger()
            .get("op-nospace")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.status, OpStatus::Failed);
        assert_eq!(row.error_code.as_deref(), Some("enospc"));
    }

    #[tokio::test]
    async fn r_move_08_fault_injection_at_every_step_leaves_file_at_source_or_dest_never_neither() {
        // Test every possible fault step (1..=10) across a cross-volume move.
        // At every single crash point, the original payload must exist intact at
        // `src`, `dst`, or BOTH — NEVER neither!
        let payload = b"critical-irreplaceable-user-document-2026".to_vec();
        let mut saw_completed_move = false;

        for fail_step in 1..=10 {
            let (_d, root) = tempdir();
            let vol_a = root.join("vol_a");
            let vol_b = root.join("vol_b");
            mkdir(&vol_a);
            mkdir(&vol_b);
            let (org, faulty, auth, _db) = setup_organizer(&root).await;
            faulty.simulate_cross_volume(vol_a.clone(), vol_b.clone());
            faulty.fail_at_step(fail_step, FaultKind::Errno(libc::EIO));

            let src = vol_a.join("doc.pdf");
            let dst = vol_b.join("Organized/doc.pdf");
            write_file(&src, &payload);

            let res = org
                .execute(
                    OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                    &auth,
                )
                .await;

            let src_ok = std::fs::read(&src).is_ok_and(|b| b == payload);
            let dst_ok = std::fs::read(&dst).is_ok_and(|b| b == payload);
            assert!(
                src_ok || dst_ok,
                "DATA LOSS at fail_step={fail_step}: neither src nor dst has intact payload!"
            );
            if res.is_ok() && dst_ok && !src_ok {
                saw_completed_move = true;
            }
        }
        assert!(
            saw_completed_move,
            "once fail_step exceeds the pipeline length, the move must complete"
        );
    }

    // -------------------------------------------------------------------------
    // P4-T06: Copy & Rename pipeline tests (R-COPY-01..03, R-REN-01..03)
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn r_copy_01_copy_streams_verifies_hash_and_leaves_source_untouched() {
        let (_d, root) = tempdir();
        let (org, fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("source.png");
        let dst = root.join("Backup/source.png");
        let bytes = vec![0x5Au8; 70_000];
        write_file(&src, &bytes);

        let outcome = org
            .execute(
                OrganizeRequest::new(OpKind::Copy, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("copy succeeds");
        let row = outcome.into_completed().expect("completed");

        assert_eq!(row.status, OpStatus::Completed);
        assert!(row.content_hash.is_some());
        assert!(fs.exists_no_follow(&src), "copy must keep source intact");
        assert_eq!(std::fs::read(&src).expect("read src"), bytes);
        assert_eq!(std::fs::read(&dst).expect("read dst"), bytes);
    }

    #[tokio::test]
    async fn r_copy_02_copy_failure_cleans_up_staging_file_to_trash() {
        let (_d, root) = tempdir();
        let (org, faulty, auth, _db) = setup_organizer(&root).await;
        let src = root.join("video.mov");
        let dst = root.join("Backup/video.mov");
        write_file(&src, &[3u8; 8192]);

        faulty.fail_next_op(
            FsOpKind::CopyExcl,
            FaultKind::PartialCopy {
                bytes: 2048,
                errno: libc::ENOSPC,
            },
        );

        let mut req = OrganizeRequest::new(OpKind::Copy, src.clone(), dst.clone());
        req.op_id = Some("op-copy-fail".into());
        assert!(org.execute(req, &auth).await.is_err());

        assert!(!faulty.exists_no_follow(&staging_path_for(&dst, "op-copy-fail")));
        assert!(!faulty.exists_no_follow(&dst));
        assert_eq!(std::fs::read(&src).expect("src intact"), vec![3u8; 8192]);

        let row = org
            .ledger()
            .get("op-copy-fail")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.status, OpStatus::RolledBack);
    }

    #[tokio::test]
    async fn r_copy_03_copy_respects_conflict_resolution_policies() {
        let (_d, root) = tempdir();
        let (org, _fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("notes.txt");
        let dst = root.join("Backup/notes.txt");
        write_file(&src, b"new-notes");
        write_file(&dst, b"existing-notes");

        // 1. Skip policy leaves existing destination untouched.
        let mut skip_req = OrganizeRequest::new(OpKind::Copy, src.clone(), dst.clone());
        skip_req.conflict_policy = ConflictPolicy::Skip;
        let out_skip = org.execute(skip_req, &auth).await.expect("skip copy");
        assert!(matches!(out_skip, OrganizeOutcome::SkippedConflict(_)));
        assert_eq!(std::fs::read(&dst).expect("dst"), b"existing-notes");

        // 2. AutoRename policy creates `notes (1).txt`.
        let mut ren_req = OrganizeRequest::new(OpKind::Copy, src.clone(), dst.clone());
        ren_req.conflict_policy = ConflictPolicy::AutoRename;
        let out_ren = org.execute(ren_req, &auth).await.expect("auto_rename copy");
        let row = out_ren.into_completed().expect("completed");
        let expected_dst = root.join("Backup/notes (1).txt");
        assert_eq!(PathBuf::from(&row.destination_path), expected_dst);
        assert_eq!(
            std::fs::read(&expected_dst).expect("read (1)"),
            b"new-notes"
        );
        assert_eq!(std::fs::read(&dst).expect("read orig"), b"existing-notes");
    }

    #[tokio::test]
    async fn r_ren_01_rename_in_place_updates_name_atomically() {
        let (_d, root) = tempdir();
        let (org, fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("IMG_001.JPG");
        let dst = root.join("2026-11-14_IMG_001.JPG");
        write_file(&src, b"jpeg-bytes");
        let pre = fs.lstat(&src).expect("lstat");

        let outcome = org
            .execute(
                OrganizeRequest::new(OpKind::Rename, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("rename");
        let row = outcome.into_completed().expect("completed");
        assert_eq!(row.new_name, "2026-11-14_IMG_001.JPG");
        assert!(!fs.exists_no_follow(&src));
        assert_eq!(fs.lstat(&dst).expect("lstat dst").ino, pre.ino);
    }

    #[tokio::test]
    async fn r_ren_02_rename_to_same_path_is_safe_noop() {
        let (_d, root) = tempdir();
        let (org, fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("same.txt");
        write_file(&src, b"unchanged");

        let outcome = org
            .execute(
                OrganizeRequest::new(OpKind::Rename, src.clone(), src.clone()),
                &auth,
            )
            .await
            .expect("noop rename");
        assert!(matches!(outcome, OrganizeOutcome::NoOpSamePath));
        assert!(fs.exists_no_follow(&src));
        assert_eq!(std::fs::read(&src).expect("read"), b"unchanged");
    }

    #[tokio::test]
    async fn r_ren_03_rename_conflict_auto_renames_without_overwriting() {
        let (_d, root) = tempdir();
        let (org, _fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("draft.md");
        let dst = root.join("final.md");
        write_file(&src, b"draft-body");
        write_file(&dst, b"existing-final-body");

        let outcome = org
            .execute(
                OrganizeRequest::new(OpKind::Rename, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("rename with auto_rename");
        let row = outcome.into_completed().expect("completed");
        let actual_dst = PathBuf::from(&row.destination_path);
        assert_eq!(actual_dst, root.join("final (1).md"));
        assert_eq!(
            std::fs::read(&dst).expect("orig final"),
            b"existing-final-body"
        );
        assert_eq!(
            std::fs::read(&actual_dst).expect("new final (1)"),
            b"draft-body"
        );
    }

    // -------------------------------------------------------------------------
    // P4-T07: Conflict resolver tests (R-CONF-01..05 + proptest)
    // -------------------------------------------------------------------------

    #[test]
    fn r_conf_01_auto_rename_generates_sequential_numbered_suffixes() {
        assert_eq!(
            candidate_path(Path::new("/tmp/report.pdf"), 0).expect("0"),
            PathBuf::from("/tmp/report.pdf")
        );
        assert_eq!(
            candidate_path(Path::new("/tmp/report.pdf"), 1).expect("1"),
            PathBuf::from("/tmp/report (1).pdf")
        );
        assert_eq!(
            candidate_path(Path::new("/tmp/report.pdf"), 42).expect("42"),
            PathBuf::from("/tmp/report (42).pdf")
        );
        // Compound archive extension `.tar.gz` stays intact after `(1)`.
        assert_eq!(
            candidate_path(Path::new("/tmp/backup.tar.gz"), 1).expect("tar.gz"),
            PathBuf::from("/tmp/backup (1).tar.gz")
        );
        // Extensionless and hidden files.
        assert_eq!(
            candidate_path(Path::new("/tmp/README"), 2).expect("no ext"),
            PathBuf::from("/tmp/README (2)")
        );
        assert_eq!(
            candidate_path(Path::new("/tmp/.env"), 1).expect("dotfile"),
            PathBuf::from("/tmp/.env (1)")
        );
    }

    #[tokio::test]
    async fn r_conf_02_skip_policy_leaves_source_and_existing_dest_untouched() {
        let (_d, root) = tempdir();
        let (org, _fs, auth, _db) = setup_organizer(&root).await;
        let src = root.join("incoming/a.txt");
        let dst = root.join("sorted/a.txt");
        write_file(&src, b"incoming");
        write_file(&dst, b"existing");

        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.conflict_policy = ConflictPolicy::Skip;
        let out = org.execute(req, &auth).await.expect("skip");
        assert!(matches!(out, OrganizeOutcome::SkippedConflict(_)));
        assert_eq!(std::fs::read(&src).expect("src"), b"incoming");
        assert_eq!(std::fs::read(&dst).expect("dst"), b"existing");
    }

    #[tokio::test]
    async fn r_conf_03_ask_policy_enqueues_review_item_without_mutating_files() {
        let (_d, root) = tempdir();
        let (org, _fs, auth, db) = setup_organizer(&root).await;
        let src = root.join("incoming/tax.pdf");
        let dst = root.join("Taxes/tax.pdf");
        write_file(&src, b"new-tax");
        write_file(&dst, b"old-tax");

        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.conflict_policy = ConflictPolicy::Ask;
        let out = org.execute(req, &auth).await.expect("ask");
        match out {
            OrganizeOutcome::QueuedForReview { review, .. } => {
                assert_eq!(review.path, src.to_string_lossy());
                assert_eq!(review.reason, "destination_exists");
            }
            other => panic!("expected QueuedForReview, got {other:?}"),
        }
        assert_eq!(std::fs::read(&src).expect("src"), b"new-tax");
        assert_eq!(std::fs::read(&dst).expect("dst"), b"old-tax");

        let pending = repo_operations::list_pending_reviews(&db)
            .await
            .expect("list reviews");
        assert_eq!(pending.len(), 1);
    }

    #[tokio::test]
    async fn r_conf_04_keep_both_preserves_both_files_with_distinct_contents() {
        let (_d, root) = tempdir();
        let (org, _fs, auth, _db) = setup_organizer(&root).await;
        let dst = root.join("Dest/item.txt");
        write_file(&dst, b"original-0");

        for i in 1..=3 {
            let src = root.join(format!("src_{i}.txt"));
            let content = format!("incoming-{i}");
            write_file(&src, content.as_bytes());
            let mut req = OrganizeRequest::new(OpKind::Move, src, dst.clone());
            req.conflict_policy = ConflictPolicy::KeepBoth;
            let row = org
                .execute(req, &auth)
                .await
                .expect("keep_both")
                .into_completed()
                .expect("completed");
            assert_eq!(
                PathBuf::from(&row.destination_path),
                root.join(format!("Dest/item ({i}).txt"))
            );
        }
        assert_eq!(std::fs::read(&dst).expect("0"), b"original-0");
        assert_eq!(
            std::fs::read(root.join("Dest/item (1).txt")).expect("1"),
            b"incoming-1"
        );
        assert_eq!(
            std::fs::read(root.join("Dest/item (2).txt")).expect("2"),
            b"incoming-2"
        );
        assert_eq!(
            std::fs::read(root.join("Dest/item (3).txt")).expect("3"),
            b"incoming-3"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// R-CONF-05: N conflicting moves targeting the same destination path produce N
        /// distinct files with every payload preserved and zero overwrites.
        #[test]
        fn r_conf_05_proptest_concurrent_conflicting_moves_never_overwrite_or_lose_files(
            n in 2usize..18usize
        ) {
            let (_d, root) = tempdir();
            let fs = SafeFs::with_trash_dir(root.join(".Trash"));
            let dst_dir = root.join("target");
            mkdir(&dst_dir);
            let desired_dst = dst_dir.join("invoice.pdf");

            let mut expected_payloads = BTreeSet::new();
            let mut claimed_paths = BTreeSet::new();

            for i in 0..n {
                let src = root.join(format!("src_{i}.pdf"));
                let payload = format!("unique-payload-{i}").into_bytes();
                write_file(&src, &payload);
                expected_payloads.insert(payload);

                let claim = crate::engine::conflicts::claim_with_rename_excl(
                    &fs,
                    &src,
                    &desired_dst,
                    ConflictPolicy::AutoRename,
                )
                .expect("claim_with_rename_excl must succeed");

                match claim {
                    crate::engine::conflicts::ConflictClaim::Claimed { final_path, .. } => {
                        prop_assert!(
                            claimed_paths.insert(final_path),
                            "duplicate destination path claimed!"
                        );
                    }
                    other => prop_assert!(false, "unexpected claim outcome: {other:?}"),
                }
            }

            prop_assert_eq!(claimed_paths.len(), n);
            let mut actual_payloads = BTreeSet::new();
            for p in &claimed_paths {
                let bytes = std::fs::read(p).expect("read claimed file");
                actual_payloads.insert(bytes);
            }
            prop_assert_eq!(actual_payloads, expected_payloads);
        }
    }
}
