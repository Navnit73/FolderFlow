//! Undo service (`P4-T08`, `FR-100..101`, `SAFETY_MODEL.md` § Undo safety).
//!
//! Every undo is itself a ledgered operation (`undo_move`, `undo_copy`, `undo_rename`)
//! with `trigger = 'undo'`, and sets `reversed_by` on the original operation row.
//! - Never overwrites a file now occupying the original `source_path`.
//! - Copy-undo moves the copy to Trash **only** after verifying its BLAKE3 hash still
//!   matches the recorded hash and the original file still exists.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::database::repo_operations::{self, OperationRow, OperationView};
use crate::engine::conflicts::ConflictPolicy;
use crate::engine::operations::{BatchTrigger, OpKind, OpStatus};
use crate::engine::organizer::{OrganizeRequest, Organizer};
use crate::engine::paths::PathAuthority;
use crate::error::AppResult;

/// What the user wants to undo (`P4-T08`, `P4-T10`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UndoTarget {
    /// Undo the single most recent undoable operation.
    Last {
        #[serde(default)]
        auto_rename_conflicts: bool,
    },
    /// Undo a specific set of operation IDs (executed in reverse chronological order).
    Ops {
        op_ids: Vec<String>,
        #[serde(default)]
        auto_rename_conflicts: bool,
    },
    /// Undo an entire batch in reverse order, optionally pruning created empty directories.
    Batch {
        batch_id: String,
        #[serde(default = "default_true")]
        restore_structure: bool,
        #[serde(default)]
        auto_rename_conflicts: bool,
    },
}

fn default_true() -> bool {
    true
}

/// Reported when the original path is now occupied by another file (`SAFETY_MODEL.md` § Undo safety).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UndoConflict {
    pub op_id: String,
    pub original_path: String,
    pub current_path: String,
    pub reason: String,
}

/// Summary returned by an undo invocation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub undone: Vec<OperationView>,
    pub conflicts: Vec<UndoConflict>,
    pub failed: Vec<String>,
    pub pruned_dirs: u32,
}

#[derive(Clone)]
pub struct UndoService {
    organizer: Organizer,
}

impl UndoService {
    pub fn new(organizer: Organizer) -> Self {
        Self { organizer }
    }

    pub fn organizer(&self) -> &Organizer {
        &self.organizer
    }

    pub async fn undo(
        &self,
        target: UndoTarget,
        authority: &PathAuthority,
    ) -> AppResult<UndoReport> {
        let db = self.organizer.ledger().db();
        let mut report = UndoReport::default();

        match target {
            UndoTarget::Last {
                auto_rename_conflicts,
            } => {
                let Some(last) = repo_operations::get_last_undoable_operation(db).await? else {
                    return Ok(report);
                };
                let batch_id = self.create_undo_batch(1).await?;
                self.undo_one(
                    &last,
                    &batch_id,
                    auto_rename_conflicts,
                    authority,
                    &mut report,
                )
                .await?;
                let _ = self
                    .organizer
                    .ledger()
                    .complete_batch(&batch_id, self.organizer.clock().now_ms())
                    .await;
            }
            UndoTarget::Ops {
                op_ids,
                auto_rename_conflicts,
            } => {
                // Deduplicate `op_ids` while preserving caller order (`BUG-019`).
                let mut seen = BTreeSet::new();
                let unique_ids: Vec<String> = op_ids
                    .into_iter()
                    .filter(|id| seen.insert(id.clone()))
                    .collect();

                let mut rows = Vec::with_capacity(unique_ids.len());
                for id in &unique_ids {
                    match self.organizer.ledger().get(id).await? {
                        Some(r) => rows.push(r),
                        None => report.failed.push(format!("Operation {id} not found")),
                    }
                }
                // Reverse chronological order: newest first (`started_at DESC`).
                rows.sort_by_key(|a| std::cmp::Reverse(a.started_at));
                if !rows.is_empty() {
                    let batch_id = self
                        .create_undo_batch(u32::try_from(rows.len()).unwrap_or(1))
                        .await?;
                    for row in &rows {
                        self.undo_one(
                            row,
                            &batch_id,
                            auto_rename_conflicts,
                            authority,
                            &mut report,
                        )
                        .await?;
                    }
                    let _ = self
                        .organizer
                        .ledger()
                        .complete_batch(&batch_id, self.organizer.clock().now_ms())
                        .await;
                }
            }
            UndoTarget::Batch {
                batch_id,
                restore_structure,
                auto_rename_conflicts,
            } => {
                let mut rows = self.organizer.ledger().list_by_batch(&batch_id).await?;
                rows.retain(|r| {
                    r.status == OpStatus::Completed && r.reversible && r.reversed_by.is_none()
                });
                // `list_by_batch` is ordered `started_at ASC, rowid ASC`; reverse for undo!
                rows.reverse();

                if !rows.is_empty() {
                    let undo_batch_id = self
                        .create_undo_batch(u32::try_from(rows.len()).unwrap_or(1))
                        .await?;
                    let mut candidate_dirs = Vec::new();
                    for row in &rows {
                        let before_count = report.undone.len();
                        self.undo_one(
                            row,
                            &undo_batch_id,
                            auto_rename_conflicts,
                            authority,
                            &mut report,
                        )
                        .await?;
                        if restore_structure && report.undone.len() > before_count {
                            if let Some(parent) = Path::new(&row.destination_path).parent() {
                                candidate_dirs.push(parent.to_path_buf());
                            }
                        }
                    }
                    if restore_structure {
                        report.pruned_dirs = self.prune_empty_dirs(&candidate_dirs, authority);
                    }
                    let _ = self
                        .organizer
                        .ledger()
                        .complete_batch(&undo_batch_id, self.organizer.clock().now_ms())
                        .await;
                }
            }
        }

        Ok(report)
    }

    async fn create_undo_batch(&self, total: u32) -> AppResult<String> {
        let id = Uuid::now_v7().to_string();
        let now = self.organizer.clock().now_ms();
        self.organizer
            .ledger()
            .create_batch(&id, BatchTrigger::Undo, None, total, now)
            .await?;
        Ok(id)
    }

    async fn undo_one(
        &self,
        op: &OperationRow,
        undo_batch_id: &str,
        auto_rename_conflicts: bool,
        authority: &PathAuthority,
        report: &mut UndoReport,
    ) -> AppResult<()> {
        if op.status != OpStatus::Completed {
            report.failed.push(format!(
                "Operation {} is in status {} and cannot be undone",
                op.id, op.status
            ));
            return Ok(());
        }
        if !op.reversible || op.operation_type.is_undo() {
            report
                .failed
                .push(format!("Operation {} is not reversible", op.id));
            return Ok(());
        }
        if op.reversed_by.is_some() {
            report
                .failed
                .push(format!("Operation {} has already been undone", op.id));
            return Ok(());
        }

        let fs = self.organizer.fs();
        let orig_src = PathBuf::from(&op.source_path);
        let orig_dst = PathBuf::from(&op.destination_path);

        match op.operation_type {
            OpKind::Move | OpKind::Rename => {
                let undo_kind = if op.operation_type == OpKind::Move {
                    OpKind::UndoMove
                } else {
                    OpKind::UndoRename
                };

                if !fs.exists_no_follow(&orig_dst) {
                    report.failed.push(format!(
                        "Cannot undo {}: file no longer exists at {}",
                        op.id, op.destination_path
                    ));
                    return Ok(());
                }

                // Check if the original source path is now occupied by another file (`SAFETY_MODEL.md` § Undo safety).
                if fs.exists_no_follow(&orig_src) && !auto_rename_conflicts {
                    report.conflicts.push(UndoConflict {
                        op_id: op.id.clone(),
                        original_path: op.source_path.clone(),
                        current_path: op.destination_path.clone(),
                        reason: "Original location is now occupied by another file.".into(),
                    });
                    return Ok(());
                }

                let mut req = OrganizeRequest::new(undo_kind, orig_dst, orig_src);
                req.batch_id = Some(undo_batch_id.to_owned());
                req.reversible = false;
                req.conflict_policy = if auto_rename_conflicts {
                    ConflictPolicy::AutoRename
                } else {
                    ConflictPolicy::Skip
                };

                match self.organizer.execute(req, authority).await {
                    Ok(outcome) => {
                        if let Some(undo_row) = outcome.into_completed() {
                            repo_operations::mark_reversed_by(
                                self.organizer.ledger().db(),
                                &op.id,
                                &undo_row.id,
                            )
                            .await?;
                            report.undone.push(OperationView::from(undo_row));
                        } else {
                            report
                                .failed
                                .push(format!("Undo of {} did not complete", op.id));
                        }
                    }
                    Err(e) => report.failed.push(format!("Undo of {} failed: {e}", op.id)),
                }
            }
            OpKind::Copy => {
                // SAFETY_MODEL.md § Undo safety:
                // "Undoing a copy moves the copy to Trash only after verifying its BLAKE3 hash
                //  still matches the recorded hash and the original still exists."
                if !fs.exists_no_follow(&orig_src) {
                    report.failed.push(format!(
                        "Refusing to undo copy {}: original file at {} no longer exists",
                        op.id, op.source_path
                    ));
                    return Ok(());
                }
                if !fs.exists_no_follow(&orig_dst) {
                    report.failed.push(format!(
                        "Cannot undo copy {}: copied file at {} no longer exists",
                        op.id, op.destination_path
                    ));
                    return Ok(());
                }

                let Some(expected_hash) = op.content_hash.as_deref() else {
                    report.failed.push(format!(
                        "Refusing to undo copy {}: no recorded content hash",
                        op.id
                    ));
                    return Ok(());
                };

                let Ok((_sz, current_dst_hash)) = fs.hash_file(&orig_dst) else {
                    report.failed.push(format!(
                        "Refusing to undo copy {}: could not hash {}",
                        op.id, op.destination_path
                    ));
                    return Ok(());
                };

                if current_dst_hash.to_hex() != expected_hash {
                    report.failed.push(format!(
                        "Refusing to undo copy {}: copied file was modified since it was created",
                        op.id
                    ));
                    return Ok(());
                }

                let mut req = OrganizeRequest::new(OpKind::UndoCopy, orig_dst.clone(), orig_dst);
                req.batch_id = Some(undo_batch_id.to_owned());
                req.reversible = false;

                match self.organizer.execute(req, authority).await {
                    Ok(outcome) => {
                        if let Some(undo_row) = outcome.into_completed() {
                            repo_operations::mark_reversed_by(
                                self.organizer.ledger().db(),
                                &op.id,
                                &undo_row.id,
                            )
                            .await?;
                            report.undone.push(OperationView::from(undo_row));
                        } else {
                            report
                                .failed
                                .push(format!("Undo of copy {} did not complete", op.id));
                        }
                    }
                    Err(e) => report
                        .failed
                        .push(format!("Undo of copy {} failed: {e}", op.id)),
                }
            }
            _ => {
                report.failed.push(format!(
                    "Operation {} ({}) cannot be undone",
                    op.id, op.operation_type
                ));
            }
        }
        Ok(())
    }

    fn prune_empty_dirs(&self, leaf_dirs: &[PathBuf], authority: &PathAuthority) -> u32 {
        let fs = self.organizer.fs();
        let approved_roots: Vec<PathBuf> =
            authority.roots().iter().map(|r| r.path.clone()).collect();
        let mut pruned = 0u32;

        for leaf in leaf_dirs {
            let mut cursor = Some(leaf.as_path());
            while let Some(dir) = cursor {
                // Never prune an approved root or anything outside an approved root!
                if approved_roots.iter().any(|r| r == dir)
                    || !approved_roots.iter().any(|r| dir.starts_with(r))
                {
                    break;
                }
                match fs.rmdir_if_empty(dir) {
                    Ok(true) => {
                        pruned = pruned.saturating_add(1);
                        cursor = dir.parent();
                    }
                    _ => break,
                }
            }
        }
        pruned
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::*;
    use crate::database;
    use crate::engine::operations::{BatchTrigger, Ledger, OpKind, OpStatus};
    use crate::engine::organizer::OrganizeRequest;
    use crate::engine::paths::{ApprovedRoot, PathAuthority, RootKind};
    use crate::engine::safe_fs::{Fs, SafeFs};
    use crate::test_support::{tempdir, write_file};
    use crate::utils::clock::FixedClock;

    async fn setup_undo(root: &Path) -> (UndoService, Organizer, Arc<SafeFs>, PathAuthority) {
        let db = database::open_in_memory().await.expect("open db");
        let ledger = Ledger::new(db);
        let fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
        let organizer = Organizer::new(ledger, fs.clone(), clock);
        let undo = UndoService::new(organizer.clone());
        let auth = PathAuthority::new(vec![ApprovedRoot {
            path: root.to_path_buf(),
            kind: RootKind::Monitored,
        }]);
        (undo, organizer, fs, auth)
    }

    #[tokio::test]
    async fn r_undo_01_undo_last_move_restores_file_to_original_path_and_sets_reversed_by() {
        let (_d, root) = tempdir();
        let (undo, org, fs, auth) = setup_undo(&root).await;
        let src = root.join("Downloads/invoice.pdf");
        let dst = root.join("Finance/invoice.pdf");
        write_file(&src, b"invoice-payload");

        let move_row = org
            .execute(
                OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("move")
            .into_completed()
            .expect("completed");

        let report = undo
            .undo(
                UndoTarget::Last {
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo last");

        assert_eq!(report.undone.len(), 1);
        assert_eq!(report.undone[0].operation_type, OpKind::UndoMove);
        assert_eq!(report.undone[0].status, OpStatus::Completed);
        assert!(fs.exists_no_follow(&src), "file restored to original path");
        assert!(!fs.exists_no_follow(&dst), "removed from organized path");
        assert_eq!(std::fs::read(&src).expect("read"), b"invoice-payload");

        let orig = org
            .ledger()
            .get(&move_row.id)
            .await
            .expect("get")
            .expect("orig");
        assert_eq!(
            orig.reversed_by.as_deref(),
            Some(report.undone[0].id.as_str()),
            "original operation must record reversed_by"
        );
    }

    #[tokio::test]
    async fn r_undo_02_undo_rename_restores_original_filename() {
        let (_d, root) = tempdir();
        let (undo, org, fs, auth) = setup_undo(&root).await;
        let src = root.join("IMG_1234.JPG");
        let dst = root.join("2026-11-14_Vacation.JPG");
        write_file(&src, b"photo-bytes");

        let ren_row = org
            .execute(
                OrganizeRequest::new(OpKind::Rename, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("rename")
            .into_completed()
            .expect("completed");

        let report = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![ren_row.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo rename");

        assert_eq!(report.undone.len(), 1);
        assert_eq!(report.undone[0].operation_type, OpKind::UndoRename);
        assert!(fs.exists_no_follow(&src));
        assert!(!fs.exists_no_follow(&dst));
    }

    #[tokio::test]
    async fn r_undo_03_undo_copy_verifies_blake3_and_original_before_moving_copy_to_trash() {
        let (_d, root) = tempdir();
        let (undo, org, fs, auth) = setup_undo(&root).await;
        let src = root.join("master.wav");
        let dst = root.join("Backup/master.wav");
        write_file(&src, b"lossless-audio-stream");

        let copy_row = org
            .execute(
                OrganizeRequest::new(OpKind::Copy, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("copy")
            .into_completed()
            .expect("completed");

        let report = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![copy_row.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo copy");

        assert_eq!(report.undone.len(), 1);
        assert_eq!(report.undone[0].operation_type, OpKind::UndoCopy);
        assert!(fs.exists_no_follow(&src), "original must still exist");
        assert!(!fs.exists_no_follow(&dst), "copy must be moved to Trash");
        // Verify the copy is in the test `.Trash` directory, not permanently deleted.
        let trash_path = PathBuf::from(&report.undone[0].destination_path);
        assert!(trash_path.starts_with(root.join(".Trash")));
        assert_eq!(
            std::fs::read(&trash_path).expect("read trashed copy"),
            b"lossless-audio-stream"
        );
    }

    #[tokio::test]
    async fn r_undo_04_undo_copy_refuses_if_copy_was_modified_or_original_missing() {
        let (_d, root) = tempdir();
        let (undo, org, fs, auth) = setup_undo(&root).await;

        // Case A: Copy was edited by the user after being copied -> must refuse to trash it!
        let src1 = root.join("doc1.txt");
        let dst1 = root.join("Backup/doc1.txt");
        write_file(&src1, b"initial-copy");
        let copy1 = org
            .execute(
                OrganizeRequest::new(OpKind::Copy, src1.clone(), dst1.clone()),
                &auth,
            )
            .await
            .expect("copy1")
            .into_completed()
            .expect("completed");
        write_file(&dst1, b"user-added-important-edits-to-copy");

        let rep1 = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![copy1.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo report 1");
        assert!(rep1.undone.is_empty());
        assert_eq!(rep1.failed.len(), 1);
        assert!(
            fs.exists_no_follow(&dst1),
            "modified copy must never be trashed"
        );

        // Case B: Original was removed -> copy is now the only copy, must refuse to trash it!
        let src2 = root.join("doc2.txt");
        let dst2 = root.join("Backup/doc2.txt");
        write_file(&src2, b"precious-only-copy");
        let copy2 = org
            .execute(
                OrganizeRequest::new(OpKind::Copy, src2.clone(), dst2.clone()),
                &auth,
            )
            .await
            .expect("copy2")
            .into_completed()
            .expect("completed");
        crate::test_support::remove_file(&src2);

        let rep2 = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![copy2.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo report 2");
        assert!(rep2.undone.is_empty());
        assert_eq!(rep2.failed.len(), 1);
        assert!(
            fs.exists_no_follow(&dst2),
            "copy must not be trashed when original is missing"
        );
    }

    #[tokio::test]
    async fn r_undo_05_undo_refuses_to_overwrite_newer_file_occupying_original_path() {
        let (_d, root) = tempdir();
        let (undo, org, _fs, auth) = setup_undo(&root).await;
        let src = root.join("Downloads/report.pdf");
        let dst = root.join("Sorted/report.pdf");
        write_file(&src, b"moved-version");

        let move_row = org
            .execute(
                OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("move")
            .into_completed()
            .expect("completed");

        // A new file arrives at the original `src` path!
        write_file(&src, b"newer-file-at-source");

        // 1. With `auto_rename_conflicts: false`, undo must report a conflict and leave both untouched.
        let rep = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![move_row.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo conflict check");
        assert!(rep.undone.is_empty());
        assert_eq!(rep.conflicts.len(), 1);
        assert_eq!(std::fs::read(&src).expect("newer"), b"newer-file-at-source");
        assert_eq!(std::fs::read(&dst).expect("moved"), b"moved-version");

        // 2. With `auto_rename_conflicts: true`, undo restores to `Downloads/report (1).pdf`.
        let rep_auto = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![move_row.id.clone()],
                    auto_rename_conflicts: true,
                },
                &auth,
            )
            .await
            .expect("undo with auto_rename");
        assert_eq!(rep_auto.undone.len(), 1);
        assert_eq!(std::fs::read(&src).expect("newer"), b"newer-file-at-source");
        assert_eq!(
            std::fs::read(root.join("Downloads/report (1).pdf")).expect("restored (1)"),
            b"moved-version"
        );
    }

    #[tokio::test]
    async fn r_undo_06_undo_batch_reverses_operations_in_reverse_order_and_prunes_created_empty_dirs(
    ) {
        let (_d, root) = tempdir();
        let (undo, org, fs, auth) = setup_undo(&root).await;

        org.ledger()
            .create_batch("batch-undo-1", BatchTrigger::Manual, None, 2, 1000)
            .await
            .expect("create batch");

        let f1 = root.join("Inbox/a.txt");
        let f2 = root.join("Inbox/b.txt");
        let d1 = root.join("Archive/2026/11/a.txt");
        let d2 = root.join("Archive/2026/11/b.txt");
        write_file(&f1, b"alpha");
        write_file(&f2, b"beta");

        let mut req1 = OrganizeRequest::new(OpKind::Move, f1.clone(), d1.clone());
        req1.batch_id = Some("batch-undo-1".into());
        org.execute(req1, &auth).await.expect("move 1");

        let mut req2 = OrganizeRequest::new(OpKind::Move, f2.clone(), d2.clone());
        req2.batch_id = Some("batch-undo-1".into());
        org.execute(req2, &auth).await.expect("move 2");

        let report = undo
            .undo(
                UndoTarget::Batch {
                    batch_id: "batch-undo-1".into(),
                    restore_structure: true,
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo batch");

        assert_eq!(report.undone.len(), 2);
        // Second operation (`b.txt`) must be undone first (reverse order).
        assert_eq!(report.undone[0].original_name, "b.txt");
        assert_eq!(report.undone[1].original_name, "a.txt");
        assert!(fs.exists_no_follow(&f1));
        assert!(fs.exists_no_follow(&f2));
        assert!(
            report.pruned_dirs >= 1,
            "created empty subdirectories should be pruned"
        );
        assert!(
            !fs.exists_no_follow(&root.join("Archive/2026/11")),
            "empty leaf dir Archive/2026/11 must be pruned"
        );
        assert!(
            fs.exists_no_follow(&root),
            "approved root itself must never be pruned"
        );
    }

    #[tokio::test]
    async fn r_undo_07_already_undone_operation_cannot_be_undone_twice() {
        let (_d, root) = tempdir();
        let (undo, org, _fs, auth) = setup_undo(&root).await;
        let src = root.join("x.txt");
        let dst = root.join("Sorted/x.txt");
        write_file(&src, b"once");

        let row = org
            .execute(
                OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("move")
            .into_completed()
            .expect("completed");

        let first = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![row.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("first undo");
        assert_eq!(first.undone.len(), 1);

        let second = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![row.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("second undo attempt");
        assert!(second.undone.is_empty());
        assert_eq!(second.failed.len(), 1);
    }

    #[tokio::test]
    async fn bug_019_undo_ops_deduplicates_ids_without_spurious_error() {
        let (_d, root) = tempdir();
        let (undo, org, fs, auth) = setup_undo(&root).await;
        let src = root.join("dup.txt");
        let dst = root.join("Sorted/dup.txt");
        write_file(&src, b"dedup-payload");

        let row = org
            .execute(
                OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
                &auth,
            )
            .await
            .expect("move")
            .into_completed()
            .expect("completed");

        let report = undo
            .undo(
                UndoTarget::Ops {
                    op_ids: vec![row.id.clone(), row.id.clone()],
                    auto_rename_conflicts: false,
                },
                &auth,
            )
            .await
            .expect("undo duplicate op_ids");

        assert_eq!(report.undone.len(), 1);
        assert!(
            report.failed.is_empty(),
            "duplicate op_ids must not emit spurious failure: {:?}",
            report.failed
        );
        assert!(fs.exists_no_follow(&src));
        assert!(!fs.exists_no_follow(&dst));
    }
}
