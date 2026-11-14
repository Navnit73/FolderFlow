//! Startup crash recovery (`P4-T09`, `SAFETY_MODEL.md` § Startup recovery table).
//!
//! Runs before folder watchers start and resolves every non-terminal operation row
//! (`PENDING`, `VALIDATING`, `IN_PROGRESS`, `VERIFYING`, `ROLLBACK_REQUIRED`) to a
//! consistent terminal state (`COMPLETED`, `ROLLED_BACK`, or `FAILED`), plus sweeps
//! any orphan `.folderflow-*.partial` staging files to Trash.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use specta::Type;
use walkdir::WalkDir;

use crate::database::repo_operations::{OperationRow, OperationTransitionPatch};
use crate::engine::conflicts::{claim_with_rename_excl, ConflictClaim, ConflictPolicy};
use crate::engine::fs_view::EntryKind;
use crate::engine::operations::{Ledger, OpKind, OpStatus};
use crate::engine::organizer::file_name_string;
use crate::engine::safe_fs::{Blake3Hash, CopyOutput, EntryIdentity, Fs};
use crate::error::AppResult;
use crate::utils::clock::Clock;

/// High-priority recovery alert emitted if both source and destination are missing.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAlert {
    pub op_id: String,
    pub source_path: String,
    pub destination_path: String,
    pub message: String,
}

/// Summary of startup recovery actions.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub completed: Vec<String>,
    pub rolled_back: Vec<String>,
    pub failed: Vec<String>,
    pub orphan_partials_trashed: u32,
    pub alerts: Vec<RecoveryAlert>,
}

/// Resolves all non-terminal ledger rows per `SAFETY_MODEL.md` § Startup recovery and
/// moves any stray `.folderflow-*.partial` files under `scan_roots` to Trash.
pub async fn recover_startup(
    ledger: &Ledger,
    fs: &dyn Fs,
    clock: &dyn Clock,
    scan_roots: &[PathBuf],
) -> AppResult<RecoveryReport> {
    let mut report = RecoveryReport::default();
    let non_terminal = ledger.list_non_terminal().await?;
    let mut handled_staging = HashSet::<PathBuf>::new();

    for op in non_terminal {
        if let Some(sp) = &op.staging_path {
            handled_staging.insert(PathBuf::from(sp));
        }
        recover_one(ledger, fs, clock, &op, &mut report).await?;
    }

    // Sweep any orphan `.folderflow-*.partial` files under `scan_roots` to Trash.
    for root in scan_roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
            let path = entry.path();
            if handled_staging.contains(path) {
                continue;
            }
            if is_folderflow_partial_file(path)
                && fs.exists_no_follow(path)
                && fs.move_to_trash(path).is_ok()
            {
                report.orphan_partials_trashed = report.orphan_partials_trashed.saturating_add(1);
            }
        }
    }

    Ok(report)
}

fn is_folderflow_partial_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with(".folderflow-") && name.ends_with(".partial")
}

async fn recover_one(
    ledger: &Ledger,
    fs: &dyn Fs,
    clock: &dyn Clock,
    op: &OperationRow,
    report: &mut RecoveryReport,
) -> AppResult<()> {
    let now = clock.now_ms();
    let src = Path::new(&op.source_path);
    let dst = Path::new(&op.destination_path);
    let staging = op.staging_path.as_deref().map(Path::new);

    let src_exists = fs.exists_no_follow(src);
    let dst_exists = fs.exists_no_follow(dst);
    let staging_exists = staging.is_some_and(|p| fs.exists_no_follow(p));

    // Row 9: Any non-terminal where source, staging, and destination are ALL absent -> FAILED + alert.
    if !src_exists && !dst_exists && !staging_exists {
        ledger
            .transition(
                &op.id,
                op.status,
                OpStatus::Failed,
                OperationTransitionPatch {
                    staging_path: Some(None),
                    error_code: Some(Some("recovery_missing_files".into())),
                    error_message: Some(Some(
                        "Neither source nor destination found during startup recovery.".into(),
                    )),
                    completed_at: Some(now),
                    ..Default::default()
                },
            )
            .await?;
        report.failed.push(op.id.clone());
        report.alerts.push(RecoveryAlert {
            op_id: op.id.clone(),
            source_path: op.source_path.clone(),
            destination_path: op.destination_path.clone(),
            message: format!(
                "Interrupted operation {} could not find {} or {}.",
                op.id, op.source_path, op.destination_path
            ),
        });
        return Ok(());
    }

    match op.status {
        // Row 1: PENDING / VALIDATING -> nothing touched disk -> ROLLED_BACK.
        OpStatus::Pending | OpStatus::Validating => {
            if let Some(sp) = staging.filter(|_| staging_exists) {
                let _ = fs.move_to_trash(sp);
            }
            ledger
                .transition(
                    &op.id,
                    op.status,
                    OpStatus::RolledBack,
                    OperationTransitionPatch {
                        staging_path: Some(None),
                        error_code: Some(Some("recovered_pre_mutation".into())),
                        error_message: Some(Some(
                            "Rolled back interrupted pre-mutation operation on startup.".into(),
                        )),
                        completed_at: Some(now),
                        ..Default::default()
                    },
                )
                .await?;
            report.rolled_back.push(op.id.clone());
        }

        OpStatus::InProgress => {
            let is_same_vol_move = !op.cross_volume
                && staging.is_none()
                && matches!(
                    op.operation_type,
                    OpKind::Move | OpKind::Rename | OpKind::UndoMove | OpKind::UndoRename
                );

            if is_same_vol_move {
                if !src_exists && dst_exists {
                    // Row 3: IN_PROGRESS (same-vol) where source is absent and destination exists with matching inode.
                    let dst_stat = fs.lstat(dst).ok();
                    let ino_matches = match (op.ino, dst_stat.as_ref()) {
                        (Some(expected_ino), Some(stat)) => {
                            i64::try_from(stat.ino).ok() == Some(expected_ino)
                        }
                        (_, Some(_)) => true,
                        _ => false,
                    };
                    if ino_matches {
                        ledger
                            .transition(
                                &op.id,
                                OpStatus::InProgress,
                                OpStatus::Completed,
                                OperationTransitionPatch {
                                    completed_at: Some(now),
                                    ..Default::default()
                                },
                            )
                            .await?;
                        report.completed.push(op.id.clone());
                        return Ok(());
                    }
                }
                // Row 2: IN_PROGRESS (same-vol) where source still exists -> move never happened -> ROLLED_BACK.
                ledger
                    .transition(
                        &op.id,
                        OpStatus::InProgress,
                        OpStatus::RolledBack,
                        OperationTransitionPatch {
                            error_code: Some(Some("recovered_in_progress".into())),
                            error_message: Some(Some(
                                "Same-volume move did not occur before interruption; rolled back."
                                    .into(),
                            )),
                            completed_at: Some(now),
                            ..Default::default()
                        },
                    )
                    .await?;
                report.rolled_back.push(op.id.clone());
            } else {
                // Row 4: IN_PROGRESS (cross-vol / copy) -> trash staging -> ROLLED_BACK.
                if let Some(sp) = staging.filter(|_| staging_exists) {
                    let _ = fs.move_to_trash(sp);
                }
                ledger
                    .transition(
                        &op.id,
                        OpStatus::InProgress,
                        OpStatus::RolledBack,
                        OperationTransitionPatch {
                            staging_path: Some(None),
                            error_code: Some(Some("recovered_staged_in_progress".into())),
                            error_message: Some(Some(
                                "Moved partial staging file to Trash and rolled back on startup."
                                    .into(),
                            )),
                            completed_at: Some(now),
                            ..Default::default()
                        },
                    )
                    .await?;
                report.rolled_back.push(op.id.clone());
            }
        }

        OpStatus::Verifying => {
            let is_move = matches!(op.operation_type, OpKind::Move | OpKind::UndoMove);

            if src_exists && staging_exists && !dst_exists {
                // Row 5: VERIFYING (cross-vol / copy), source + staging exist, dest absent.
                // Re-verify hash; if match -> rename_excl(staging, dst) -> (remove source if move) -> COMPLETED;
                // else trash staging -> ROLLED_BACK.
                let sp = staging.expect("staging_exists");
                if let Some((expected_src_id, expected_copy)) =
                    expected_verification_data(op, fs, src)
                {
                    if let Ok(token) = fs.verify_copied(src, &expected_src_id, sp, &expected_copy) {
                        if let Ok(ConflictClaim::Claimed { final_path, .. }) =
                            claim_with_rename_excl(fs, sp, dst, ConflictPolicy::AutoRename)
                        {
                            if !is_move || fs.remove_verified_source(src, token).is_ok() {
                                let final_str = final_path.to_string_lossy().into_owned();
                                let final_name = file_name_string(&final_path);
                                ledger
                                    .transition(
                                        &op.id,
                                        OpStatus::Verifying,
                                        OpStatus::Completed,
                                        OperationTransitionPatch {
                                            destination_path: Some(final_str),
                                            new_name: Some(final_name),
                                            staging_path: Some(None),
                                            completed_at: Some(now),
                                            ..Default::default()
                                        },
                                    )
                                    .await?;
                                report.completed.push(op.id.clone());
                                return Ok(());
                            }
                        }
                    }
                }
                // Hash mismatch or verification failed -> Trash staging -> ROLLED_BACK.
                let _ = fs.move_to_trash(sp);
                rollback_from_verifying(ledger, &op.id, now, "recovery_verify_failed").await?;
                report.rolled_back.push(op.id.clone());
            } else if src_exists && !staging_exists && dst_exists {
                // Rows 6 & 7: VERIFYING where rename(staging -> dst) already happened before crash.
                if let Some((expected_src_id, expected_copy)) =
                    expected_verification_data(op, fs, src)
                {
                    if let Ok(token) = fs.verify_copied(src, &expected_src_id, dst, &expected_copy)
                    {
                        if is_move {
                            let _ = fs.remove_verified_source(src, token);
                        }
                        ledger
                            .transition(
                                &op.id,
                                OpStatus::Verifying,
                                OpStatus::Completed,
                                OperationTransitionPatch {
                                    staging_path: Some(None),
                                    completed_at: Some(now),
                                    ..Default::default()
                                },
                            )
                            .await?;
                        report.completed.push(op.id.clone());
                        return Ok(());
                    }
                }
                rollback_from_verifying(ledger, &op.id, now, "recovery_dst_mismatch").await?;
                report.rolled_back.push(op.id.clone());
            } else if !src_exists && dst_exists {
                // Source already removed and destination exists -> COMPLETED.
                if let Some(sp) = staging.filter(|_| staging_exists) {
                    let _ = fs.move_to_trash(sp);
                }
                ledger
                    .transition(
                        &op.id,
                        OpStatus::Verifying,
                        OpStatus::Completed,
                        OperationTransitionPatch {
                            staging_path: Some(None),
                            completed_at: Some(now),
                            ..Default::default()
                        },
                    )
                    .await?;
                report.completed.push(op.id.clone());
            } else {
                if let Some(sp) = staging.filter(|_| staging_exists) {
                    let _ = fs.move_to_trash(sp);
                }
                rollback_from_verifying(ledger, &op.id, now, "recovery_incomplete_verifying")
                    .await?;
                report.rolled_back.push(op.id.clone());
            }
        }

        // Row 8: ROLLBACK_REQUIRED -> Trash staging if present -> ROLLED_BACK.
        OpStatus::RollbackRequired => {
            if let Some(sp) = staging.filter(|_| staging_exists) {
                let _ = fs.move_to_trash(sp);
            }
            ledger
                .transition(
                    &op.id,
                    OpStatus::RollbackRequired,
                    OpStatus::RolledBack,
                    OperationTransitionPatch {
                        staging_path: Some(None),
                        error_code: Some(Some("recovered_rollback_required".into())),
                        completed_at: Some(now),
                        ..Default::default()
                    },
                )
                .await?;
            report.rolled_back.push(op.id.clone());
        }

        OpStatus::Completed | OpStatus::Failed | OpStatus::RolledBack => {}
    }

    Ok(())
}

async fn rollback_from_verifying(
    ledger: &Ledger,
    op_id: &str,
    now: i64,
    code: &str,
) -> AppResult<()> {
    ledger
        .transition(
            op_id,
            OpStatus::Verifying,
            OpStatus::RollbackRequired,
            OperationTransitionPatch::default(),
        )
        .await?;
    ledger
        .transition(
            op_id,
            OpStatus::RollbackRequired,
            OpStatus::RolledBack,
            OperationTransitionPatch {
                staging_path: Some(None),
                error_code: Some(Some(code.to_owned())),
                completed_at: Some(now),
                ..Default::default()
            },
        )
        .await?;
    Ok(())
}

fn expected_verification_data(
    op: &OperationRow,
    fs: &dyn Fs,
    src: &Path,
) -> Option<(EntryIdentity, CopyOutput)> {
    let hash_str = op.content_hash.as_deref()?;
    let blake3 = Blake3Hash::from_str(hash_str).ok()?;
    let current_src = fs.lstat(src).ok()?;
    let dev = op
        .dev
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(current_src.dev);
    let ino = op
        .ino
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(current_src.ino);
    let size = op
        .size
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(current_src.size);
    let mtime_ns = op.mtime_ns.unwrap_or(current_src.mtime_ns);

    Some((
        EntryIdentity {
            dev,
            ino,
            size,
            mtime_ns,
            kind: EntryKind::File,
        },
        CopyOutput {
            bytes_copied: size,
            blake3,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::database;
    use crate::database::repo_operations::{NewOperationRow, OperationTransitionPatch};
    use crate::engine::operations::{Ledger, OpKind, OpStatus};
    use crate::engine::safe_fs::{Blake3Hash, Fs, SafeFs};
    use crate::test_support::{mkdir, tempdir, write_file};
    use crate::utils::clock::FixedClock;

    async fn setup_recovery(root: &Path) -> (Ledger, SafeFs, FixedClock) {
        let db = database::open_in_memory().await.expect("open db");
        let ledger = Ledger::new(db);
        let fs = SafeFs::with_trash_dir(root.join(".Trash"));
        let clock = FixedClock::utc(1_700_000_050_000);
        (ledger, fs, clock)
    }

    fn make_intent(
        id: &str,
        kind: OpKind,
        src: &Path,
        dst: &Path,
        cross_volume: bool,
    ) -> NewOperationRow {
        NewOperationRow {
            id: id.to_owned(),
            batch_id: None,
            file_record_id: None,
            operation_type: kind,
            source_path: src.to_string_lossy().into_owned(),
            destination_path: dst.to_string_lossy().into_owned(),
            staging_path: None,
            original_name: src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            new_name: dst
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dev: None,
            ino: None,
            size: None,
            mtime_ns: None,
            content_hash: None,
            cross_volume,
            rule_id: None,
            reversible: true,
            started_at: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn r_recov_01_pending_and_validating_rows_become_rolled_back() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let s1 = root.join("p.txt");
        let s2 = root.join("v.txt");
        write_file(&s1, b"pending");
        write_file(&s2, b"validating");

        ledger
            .record_intent(&make_intent(
                "op-pend",
                OpKind::Move,
                &s1,
                &root.join("out/p.txt"),
                false,
            ))
            .await
            .expect("intent 1");

        ledger
            .record_intent(&make_intent(
                "op-val",
                OpKind::Move,
                &s2,
                &root.join("out/v.txt"),
                false,
            ))
            .await
            .expect("intent 2");
        ledger
            .transition(
                "op-val",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("validating");

        let rep = recover_startup(&ledger, &fs, &clock, std::slice::from_ref(&root))
            .await
            .expect("recover");
        assert_eq!(rep.rolled_back.len(), 2);
        assert_eq!(
            ledger
                .get("op-pend")
                .await
                .expect("get")
                .expect("row")
                .status,
            OpStatus::RolledBack
        );
        assert_eq!(
            ledger
                .get("op-val")
                .await
                .expect("get")
                .expect("row")
                .status,
            OpStatus::RolledBack
        );
    }

    #[tokio::test]
    async fn r_recov_02_in_progress_same_vol_source_present_dest_absent_rolls_back() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let src = root.join("stay.txt");
        let dst = root.join("Moved/stay.txt");
        write_file(&src, b"still-at-source");

        ledger
            .record_intent(&make_intent("op-ip-src", OpKind::Move, &src, &dst, false))
            .await
            .expect("intent");
        ledger
            .transition(
                "op-ip-src",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-ip-src", OperationTransitionPatch::default())
            .await
            .expect("in_progress");

        let rep = recover_startup(&ledger, &fs, &clock, std::slice::from_ref(&root))
            .await
            .expect("recover");
        assert_eq!(rep.rolled_back, vec!["op-ip-src".to_owned()]);
        assert!(fs.exists_no_follow(&src));
        assert_eq!(
            ledger
                .get("op-ip-src")
                .await
                .expect("get")
                .expect("row")
                .status,
            OpStatus::RolledBack
        );
    }

    #[tokio::test]
    async fn r_recov_03_in_progress_same_vol_source_absent_dest_matches_inode_completes() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let src = root.join("already_moved.txt");
        let dst = root.join("already_moved_dest.txt");
        write_file(&src, b"moved-before-crash");
        let id = fs.lstat(&src).expect("lstat");

        let mut intent = make_intent("op-ip-dst", OpKind::Move, &src, &dst, false);
        intent.dev = i64::try_from(id.dev).ok();
        intent.ino = i64::try_from(id.ino).ok();
        intent.size = i64::try_from(id.size).ok();
        intent.mtime_ns = Some(id.mtime_ns);
        ledger.record_intent(&intent).await.expect("intent");
        ledger
            .transition(
                "op-ip-dst",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-ip-dst", OperationTransitionPatch::default())
            .await
            .expect("in_progress");

        // Simulate rename completed just before crash.
        fs.rename_excl(&src, &dst).expect("rename");

        let rep = recover_startup(&ledger, &fs, &clock, std::slice::from_ref(&root))
            .await
            .expect("recover");
        assert_eq!(rep.completed, vec!["op-ip-dst".to_owned()]);
        assert_eq!(
            ledger
                .get("op-ip-dst")
                .await
                .expect("get")
                .expect("row")
                .status,
            OpStatus::Completed
        );
    }

    #[tokio::test]
    async fn r_recov_04_in_progress_cross_vol_trashes_staging_and_rolls_back() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let src = root.join("xvol.bin");
        let dst = root.join("dest/xvol.bin");
        let staging = root.join("dest/.folderflow-op-xvol-ip.partial");
        mkdir(&root.join("dest"));
        write_file(&src, b"full-source-data");
        write_file(&staging, b"partial-half");

        let mut intent = make_intent("op-xvol-ip", OpKind::Move, &src, &dst, true);
        intent.staging_path = Some(staging.to_string_lossy().into_owned());
        ledger.record_intent(&intent).await.expect("intent");
        ledger
            .transition(
                "op-xvol-ip",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-xvol-ip", OperationTransitionPatch::default())
            .await
            .expect("in_progress");

        let rep = recover_startup(&ledger, &fs, &clock, std::slice::from_ref(&root))
            .await
            .expect("recover");
        assert_eq!(rep.rolled_back, vec!["op-xvol-ip".to_owned()]);
        assert!(fs.exists_no_follow(&src));
        assert!(!fs.exists_no_follow(&staging), "staging moved to Trash");
    }

    #[tokio::test]
    async fn r_recov_05_verifying_with_valid_staging_completes_move_and_removes_source() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let src = root.join("v_ok.bin");
        let dst = root.join("dest/v_ok.bin");
        let staging = root.join("dest/.folderflow-op-ver-ok.partial");
        mkdir(&root.join("dest"));
        let payload = b"verified-complete-payload";
        write_file(&src, payload);
        write_file(&staging, payload);

        let src_id = fs.lstat(&src).expect("lstat src");
        let hash_hex = Blake3Hash(*blake3::hash(payload).as_bytes()).to_hex();

        let mut intent = make_intent("op-ver-ok", OpKind::Move, &src, &dst, true);
        intent.staging_path = Some(staging.to_string_lossy().into_owned());
        intent.dev = i64::try_from(src_id.dev).ok();
        intent.ino = i64::try_from(src_id.ino).ok();
        intent.size = i64::try_from(src_id.size).ok();
        intent.mtime_ns = Some(src_id.mtime_ns);
        intent.content_hash = Some(hash_hex);
        ledger.record_intent(&intent).await.expect("intent");
        ledger
            .transition(
                "op-ver-ok",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-ver-ok", OperationTransitionPatch::default())
            .await
            .expect("ip");
        ledger
            .transition(
                "op-ver-ok",
                OpStatus::InProgress,
                OpStatus::Verifying,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("verifying");

        let rep = recover_startup(&ledger, &fs, &clock, std::slice::from_ref(&root))
            .await
            .expect("recover");
        assert_eq!(rep.completed, vec!["op-ver-ok".to_owned()]);
        assert!(!fs.exists_no_follow(&src), "verified source removed");
        assert!(!fs.exists_no_follow(&staging), "staging renamed to dst");
        assert_eq!(std::fs::read(&dst).expect("dst"), payload);
    }

    #[tokio::test]
    async fn r_recov_06_verifying_with_corrupt_staging_trashes_staging_and_rolls_back() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let src = root.join("v_bad.bin");
        let dst = root.join("dest/v_bad.bin");
        let staging = root.join("dest/.folderflow-op-ver-bad.partial");
        mkdir(&root.join("dest"));
        let payload = b"good-source-payload";
        write_file(&src, payload);
        write_file(&staging, b"corrupted-staging-bytes");

        let src_id = fs.lstat(&src).expect("lstat src");
        let hash_hex = Blake3Hash(*blake3::hash(payload).as_bytes()).to_hex();

        let mut intent = make_intent("op-ver-bad", OpKind::Move, &src, &dst, true);
        intent.staging_path = Some(staging.to_string_lossy().into_owned());
        intent.dev = i64::try_from(src_id.dev).ok();
        intent.ino = i64::try_from(src_id.ino).ok();
        intent.size = i64::try_from(src_id.size).ok();
        intent.mtime_ns = Some(src_id.mtime_ns);
        intent.content_hash = Some(hash_hex);
        ledger.record_intent(&intent).await.expect("intent");
        ledger
            .transition(
                "op-ver-bad",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-ver-bad", OperationTransitionPatch::default())
            .await
            .expect("ip");
        ledger
            .transition(
                "op-ver-bad",
                OpStatus::InProgress,
                OpStatus::Verifying,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("verifying");

        let rep = recover_startup(&ledger, &fs, &clock, std::slice::from_ref(&root))
            .await
            .expect("recover");
        assert_eq!(rep.rolled_back, vec!["op-ver-bad".to_owned()]);
        assert_eq!(std::fs::read(&src).expect("src intact"), payload);
        assert!(!fs.exists_no_follow(&staging), "corrupt staging trashed");
        assert!(!fs.exists_no_follow(&dst));
    }

    #[tokio::test]
    async fn r_recov_07_verifying_after_staging_renamed_removes_verified_source_and_completes() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let src = root.join("v_ren.bin");
        let dst = root.join("dest/v_ren.bin");
        mkdir(&root.join("dest"));
        let payload = b"already-renamed-from-staging";
        write_file(&src, payload);
        write_file(&dst, payload);

        let src_id = fs.lstat(&src).expect("lstat src");
        let hash_hex = Blake3Hash(*blake3::hash(payload).as_bytes()).to_hex();

        let mut intent = make_intent("op-ver-ren", OpKind::Move, &src, &dst, true);
        intent.dev = i64::try_from(src_id.dev).ok();
        intent.ino = i64::try_from(src_id.ino).ok();
        intent.size = i64::try_from(src_id.size).ok();
        intent.mtime_ns = Some(src_id.mtime_ns);
        intent.content_hash = Some(hash_hex);
        ledger.record_intent(&intent).await.expect("intent");
        ledger
            .transition(
                "op-ver-ren",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-ver-ren", OperationTransitionPatch::default())
            .await
            .expect("ip");
        ledger
            .transition(
                "op-ver-ren",
                OpStatus::InProgress,
                OpStatus::Verifying,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("verifying");

        let rep = recover_startup(&ledger, &fs, &clock, std::slice::from_ref(&root))
            .await
            .expect("recover");
        assert_eq!(rep.completed, vec!["op-ver-ren".to_owned()]);
        assert!(!fs.exists_no_follow(&src), "verified source removed");
        assert_eq!(std::fs::read(&dst).expect("dst"), payload);
    }

    #[tokio::test]
    async fn r_recov_08_rollback_required_and_orphan_partial_files_moved_to_trash() {
        let (_d, root) = tempdir();
        let (ledger, fs, clock) = setup_recovery(&root).await;
        let src = root.join("rb.txt");
        let dst = root.join("out/rb.txt");
        let staging = root.join("out/.folderflow-op-rb.partial");
        let orphan = root.join("out/.folderflow-stray-999.partial");
        let missing_src = root.join("vanished.txt");
        let missing_dst = root.join("out/vanished.txt");
        mkdir(&root.join("out"));
        write_file(&src, b"keep-me");
        write_file(&staging, b"rb-staging");
        write_file(&orphan, b"orphan-staging");

        // 1. ROLLBACK_REQUIRED row
        let mut intent = make_intent("op-rb", OpKind::Move, &src, &dst, true);
        intent.staging_path = Some(staging.to_string_lossy().into_owned());
        ledger.record_intent(&intent).await.expect("intent");
        ledger
            .transition(
                "op-rb",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-rb", OperationTransitionPatch::default())
            .await
            .expect("ip");
        ledger
            .transition(
                "op-rb",
                OpStatus::InProgress,
                OpStatus::RollbackRequired,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("rb_req");

        // 2. Non-terminal row where BOTH source and destination are missing -> FAILED + alert.
        ledger
            .record_intent(&make_intent(
                "op-missing-both",
                OpKind::Move,
                &missing_src,
                &missing_dst,
                false,
            ))
            .await
            .expect("intent missing");
        ledger
            .transition(
                "op-missing-both",
                OpStatus::Pending,
                OpStatus::Validating,
                OperationTransitionPatch::default(),
            )
            .await
            .expect("val");
        ledger
            .begin_in_progress("op-missing-both", OperationTransitionPatch::default())
            .await
            .expect("ip");

        let rep = recover_startup(&ledger, &fs, &clock, &[root.join("out")])
            .await
            .expect("recover");

        assert_eq!(rep.rolled_back, vec!["op-rb".to_owned()]);
        assert_eq!(rep.failed, vec!["op-missing-both".to_owned()]);
        assert_eq!(rep.alerts.len(), 1);
        assert_eq!(rep.orphan_partials_trashed, 1);
        assert!(!fs.exists_no_follow(&staging));
        assert!(!fs.exists_no_follow(&orphan));
        assert!(fs.exists_no_follow(&src));
    }

    #[tokio::test]
    async fn r_recov_09_corrupted_sqlite_db_is_backed_up_and_recreated_on_open() {
        let (_d, root) = tempdir();
        let db_path = root.join("folderflow.db");
        // Write garbage bytes so the SQLite file is corrupt (`NFR-032`).
        write_file(
            &db_path,
            b"NOT_A_VALID_SQLITE_HEADER_GARBAGE_BYTES_1234567890",
        );

        let db = database::open(&db_path)
            .await
            .expect("corrupt DB must be backed up and recreated");

        // Verify the fresh DB works and has migrated schema.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
            .fetch_one(&db)
            .await
            .expect("query fresh db");
        assert_eq!(count, 0);

        // Verify a `.corrupt-*.bak` backup file was preserved alongside `folderflow.db`.
        let entries: Vec<String> = std::fs::read_dir(&root)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            entries
                .iter()
                .any(|name| name.starts_with("folderflow.db.corrupt-") && name.ends_with(".bak")),
            "expected corrupt DB backup file in {entries:?}"
        );
    }

    #[tokio::test]
    async fn bug_018_cross_vol_auto_rename_fault_on_remove_source_recovers_to_completed() {
        use crate::engine::organizer::{OrganizeRequest, Organizer};
        use crate::engine::paths::{ApprovedRoot, PathAuthority, RootKind};
        use crate::engine::safe_fs::fault::{FaultKind, FaultyFs, FsOpKind};
        use crate::settings::ConflictPolicy;
        use std::sync::Arc;

        let (_d, root) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        let ledger = Ledger::new(db);
        let inner_fs: Arc<dyn Fs> = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let faulty = Arc::new(FaultyFs::new(inner_fs));
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::utc(1_700_000_000_000));
        let org = Organizer::new(ledger.clone(), faulty.clone(), clock.clone());
        let auth = PathAuthority::new(vec![ApprovedRoot {
            path: root.clone(),
            kind: RootKind::Monitored,
        }]);

        let src = root.join("vol_a/report.txt");
        let dst = root.join("vol_b/report.txt");
        let dst_auto = root.join("vol_b/report (1).txt");
        write_file(&src, b"cross-vol-incoming");
        write_file(&dst, b"existing-occupied-at-dst");

        // Force EXDEV between `vol_a` and `vol_b`, and inject EIO on `RemoveVerifiedSource`
        // after staging is renamed to `report (1).txt`.
        faulty.simulate_cross_volume(root.join("vol_a"), root.join("vol_b"));
        faulty.fail_next_op(FsOpKind::RemoveVerifiedSource, FaultKind::Errno(libc::EIO));

        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.conflict_policy = ConflictPolicy::AutoRename;
        let res = org.execute(req, &auth).await;
        assert!(res.is_err(), "remove_verified_source fault returns Err");

        // Now run startup recovery: because `report (1).txt` was already verified and renamed
        // into place before `remove_verified_source` faulted, recovery must verify `report (1).txt`,
        // unlink the verified `src`, and transition the row to `COMPLETED`.
        let rep = recover_startup(
            &ledger,
            faulty.as_ref(),
            clock.as_ref(),
            std::slice::from_ref(&root),
        )
        .await
        .expect("recover");
        assert_eq!(
            rep.completed.len(),
            1,
            "expected operation to recover to COMPLETED, got {rep:?}"
        );
        assert!(
            !faulty.exists_no_follow(&src),
            "verified source must be removed"
        );
        assert_eq!(
            std::fs::read(&dst).expect("existing dst intact"),
            b"existing-occupied-at-dst"
        );
        assert_eq!(
            std::fs::read(&dst_auto).expect("auto-renamed dst intact"),
            b"cross-vol-incoming"
        );
    }
}
