//! Safe duplicate actions (`P6-T03`, FR-114..115):
//! Keep selected, Move non-keepers to review folder (undoable), Move to Trash
//! with explicit confirmation & hash verification (never permanent delete),
//! Ignore/Unignore, CSV/JSON report export, and authorized Finder reveal.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::database::repo_duplicates::{self, DuplicateGroupSummary, DuplicateMemberView};
use crate::database::{repo_files, repo_folders, Db};
use crate::duplicates::hasher::full_hash_file;
use crate::engine::fs_view::{EntryKind, ReadOnlyFs, RealFs};
use crate::engine::operations::{BatchTrigger, OpKind};
use crate::engine::organizer::{OrganizeOutcome, OrganizeRequest, Organizer};
use crate::engine::paths::{nfc, PathAuthority, PathError};
use crate::engine::safe_fs::EntryIdentity;
use crate::error::{AppError, AppResult};
use crate::settings::ConflictPolicy;
use crate::utils::csv_escape;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DupExportFormat {
    Csv,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DupAction {
    Keep {
        group_id: String,
        file_record_id: String,
    },
    MoveToReview {
        group_id: String,
        file_record_ids: Option<Vec<String>>,
        review_dir: Option<String>,
    },
    Trash {
        group_id: String,
        file_record_ids: Option<Vec<String>>,
        confirmed: bool,
    },
    Ignore {
        group_id: String,
        ignored: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DupActionReport {
    pub batch_id: Option<String>,
    pub affected: u32,
    pub skipped: Vec<String>,
    pub updated_group: Option<DuplicateGroupSummary>,
}

fn select_action_targets<'a>(
    group: &'a DuplicateGroupSummary,
    requested_ids: Option<&[String]>,
    action_label: &str,
) -> AppResult<Vec<&'a DuplicateMemberView>> {
    let candidates: Vec<&DuplicateMemberView> = match requested_ids {
        Some(ids) if !ids.is_empty() => {
            let mut seen = HashSet::new();
            let mut resolved = Vec::new();
            for wanted in ids {
                if !seen.insert(wanted.as_str()) {
                    continue;
                }
                let member = group
                    .members
                    .iter()
                    .find(|m| &m.file_record_id == wanted)
                    .ok_or_else(|| {
                        AppError::NotFound(format!(
                            "duplicate member {wanted} in group {}",
                            group.id
                        ))
                    })?;
                resolved.push(member);
            }
            resolved
        }
        _ => group.members.iter().filter(|m| !m.is_keeper).collect(),
    };

    if candidates.is_empty() {
        return Err(AppError::validation(
            "fileRecordIds",
            format!("No duplicate copies selected for {action_label}."),
        ));
    }

    let candidate_set: HashSet<&str> = candidates
        .iter()
        .map(|m| m.file_record_id.as_str())
        .collect();
    let has_surviving_live_copy = group
        .members
        .iter()
        .any(|m| !candidate_set.contains(m.file_record_id.as_str()) && m.exists);
    if candidate_set.len() >= group.members.len() || !has_surviving_live_copy {
        return Err(AppError::validation(
            "fileRecordIds",
            format!(
                "At least one live copy must remain untouched when {action_label}; cannot act on every copy."
            ),
        ));
    }

    Ok(candidates)
}

async fn verify_live_member_hash(
    path: PathBuf,
    expected_hash: String,
    filename: String,
) -> Result<EntryIdentity, String> {
    tokio::task::spawn_blocking(move || {
        let facts = RealFs
            .stat(&path)
            .map_err(|e| format!("{filename}: cannot access file ({e})"))?;
        if facts.kind != EntryKind::File {
            return Err(format!("{filename}: not a regular file"));
        }
        let actual = full_hash_file(&RealFs, &path)
            .map_err(|e| format!("{filename}: failed to verify hash ({e})"))?;
        if actual != expected_hash {
            return Err(format!(
                "{filename}: file contents changed on disk before duplicate action"
            ));
        }
        Ok(EntryIdentity {
            dev: facts.dev,
            ino: facts.ino,
            size: facts.size,
            mtime_ns: facts.modified_ns,
            kind: EntryKind::File,
        })
    })
    .await
    .map_err(|e| format!("verification task failed: {e}"))?
}

async fn record_review_exclusion(db: &Db, folder_id: Option<&str>, review_dir: &Path, now_ms: i64) {
    let dir_str = nfc(review_dir).to_string_lossy().into_owned();
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM exclusions WHERE path = ?1 AND exclusion_type = 'path' LIMIT 1",
    )
    .bind(&dir_str)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();
    if let Some(existing_id) = existing {
        if folder_id.is_none() {
            let _ = sqlx::query("UPDATE exclusions SET folder_id = NULL WHERE id = ?1")
                .bind(&existing_id)
                .execute(db)
                .await;
        }
        return;
    }
    let _ = sqlx::query(
        "INSERT INTO exclusions (id, folder_id, path, exclusion_type, created_at) \
         VALUES (?1, ?2, ?3, 'path', ?4)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(folder_id)
    .bind(&dir_str)
    .bind(now_ms)
    .execute(db)
    .await;
}

async fn execute_verified_member_batch(
    organizer: &Organizer,
    authority: &PathAuthority,
    group: &DuplicateGroupSummary,
    targets: Vec<&DuplicateMemberView>,
    op_kind: OpKind,
    review_root: Option<&Path>,
) -> AppResult<DupActionReport> {
    let db = organizer.ledger().db();
    let batch_id = Uuid::now_v7().to_string();
    let now_ms = organizer.clock().now_ms();
    organizer
        .ledger()
        .create_batch(
            &batch_id,
            BatchTrigger::Manual,
            None,
            u32::try_from(targets.len()).unwrap_or(u32::MAX),
            now_ms,
        )
        .await?;

    let mut affected: u32 = 0;
    let mut skipped: Vec<String> = Vec::new();
    for m in targets {
        let src = PathBuf::from(&m.path);
        let identity = match verify_live_member_hash(
            src.clone(),
            group.content_hash.clone(),
            m.filename.clone(),
        )
        .await
        {
            Ok(id) => id,
            Err(reason) => {
                skipped.push(reason);
                continue;
            }
        };

        let dst = match review_root {
            Some(dir) => dir.join(&m.filename),
            None => src.clone(),
        };
        let mut req = OrganizeRequest::new(op_kind, src, dst);
        req.batch_id = Some(batch_id.clone());
        req.file_record_id = Some(m.file_record_id.clone());
        req.conflict_policy = ConflictPolicy::AutoRename;
        req.reversible = op_kind != OpKind::Trash;
        req.expected_source = Some(identity);

        match organizer.execute(req, authority).await? {
            OrganizeOutcome::Completed(_) => {
                affected = affected.saturating_add(1);
                let _ = repo_files::delete_path(db, &m.path).await;
                let _ = repo_duplicates::remove_member_and_prune(db, &group.id, &m.file_record_id)
                    .await;
            }
            _ => {
                skipped.push(format!("{}: could not complete action", m.filename));
            }
        }
    }

    organizer
        .ledger()
        .complete_batch(&batch_id, organizer.clock().now_ms())
        .await?;

    Ok(DupActionReport {
        batch_id: Some(batch_id),
        affected,
        skipped,
        updated_group: repo_duplicates::get_group_details(db, &group.id).await?,
    })
}

pub async fn apply_dup_action(
    organizer: &Organizer,
    authority: &PathAuthority,
    action: DupAction,
) -> AppResult<DupActionReport> {
    let db = organizer.ledger().db();
    match action {
        DupAction::Keep {
            group_id,
            file_record_id,
        } => {
            repo_duplicates::set_keeper(db, &group_id, &file_record_id).await?;
            Ok(DupActionReport {
                batch_id: None,
                affected: 1,
                skipped: Vec::new(),
                updated_group: repo_duplicates::get_group_details(db, &group_id).await?,
            })
        }
        DupAction::Ignore { group_id, ignored } => {
            let found = repo_duplicates::set_group_ignored(db, &group_id, ignored).await?;
            if !found {
                return Err(AppError::NotFound(format!("duplicate group {group_id}")));
            }
            Ok(DupActionReport {
                batch_id: None,
                affected: 1,
                skipped: Vec::new(),
                updated_group: repo_duplicates::get_group_details(db, &group_id).await?,
            })
        }
        DupAction::MoveToReview {
            group_id,
            file_record_ids,
            review_dir,
        } => {
            let group = repo_duplicates::get_group_details(db, &group_id)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("duplicate group {group_id}")))?;
            let targets = select_action_targets(
                &group,
                file_record_ids.as_deref(),
                "moving duplicates to review",
            )?;

            let default_folder_id = group
                .members
                .iter()
                .find(|m| m.is_keeper)
                .or_else(|| group.members.first())
                .map(|m| m.folder_id.clone())
                .unwrap_or_default();

            let review_root = match review_dir {
                Some(raw) if !raw.trim().is_empty() => nfc(Path::new(raw.trim())),
                _ => {
                    let folder = repo_folders::get(db, &default_folder_id)
                        .await?
                        .ok_or_else(|| AppError::NotFound(format!("folder {default_folder_id}")))?;
                    nfc(Path::new(&folder.path).join("Duplicate Review").as_path())
                }
            };
            authority.authorize(&review_root)?;
            // Scope Duplicate Review exclusions globally (`None`) so custom `review_dir` inside
            // any monitored folder is protected across all folders (`BUG-030`).
            record_review_exclusion(db, None, &review_root, organizer.clock().now_ms()).await;

            execute_verified_member_batch(
                organizer,
                authority,
                &group,
                targets,
                OpKind::Move,
                Some(&review_root),
            )
            .await
        }
        DupAction::Trash {
            group_id,
            file_record_ids,
            confirmed,
        } => {
            if !confirmed {
                return Err(AppError::validation(
                    "confirmed",
                    "Explicit confirmation is required before moving duplicate copies to Trash.",
                ));
            }
            let group = repo_duplicates::get_group_details(db, &group_id)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("duplicate group {group_id}")))?;
            let targets = select_action_targets(
                &group,
                file_record_ids.as_deref(),
                "moving duplicates to Trash",
            )?;
            execute_verified_member_batch(
                organizer,
                authority,
                &group,
                targets,
                OpKind::Trash,
                None,
            )
            .await
        }
    }
}

pub async fn export_duplicates(
    db: &Db,
    format: DupExportFormat,
    include_ignored: bool,
) -> AppResult<String> {
    let resp = repo_duplicates::list_groups(db, include_ignored, None).await?;
    match format {
        DupExportFormat::Json => Ok(serde_json::to_string_pretty(&resp.groups)?),
        DupExportFormat::Csv => {
            let mut out = String::from(
                "group_id,content_hash,file_size,copy_count,wasted_bytes,ignored,is_keeper,filename,path,folder_id\n",
            );
            for g in &resp.groups {
                for m in &g.members {
                    let row = [
                        csv_escape(&g.id),
                        csv_escape(&g.content_hash),
                        g.file_size.to_string(),
                        g.copy_count.to_string(),
                        g.wasted_bytes.to_string(),
                        g.ignored.to_string(),
                        m.is_keeper.to_string(),
                        csv_escape(&m.filename),
                        csv_escape(&m.path),
                        csv_escape(&m.folder_id),
                    ]
                    .join(",");
                    out.push_str(&row);
                    out.push('\n');
                }
            }
            Ok(out)
        }
    }
}

pub async fn resolve_reveal_duplicate_path(
    db: &Db,
    authority: &PathAuthority,
    file_record_id: &str,
) -> AppResult<PathBuf> {
    let path_str: Option<String> =
        sqlx::query_scalar("SELECT path FROM file_records WHERE id = ?1")
            .bind(file_record_id)
            .fetch_optional(db)
            .await?;
    let path_str =
        path_str.ok_or_else(|| AppError::NotFound(format!("file record {file_record_id}")))?;
    let auth = authority.authorize(Path::new(&path_str))?;
    if RealFs.stat(&auth.path).is_err() {
        return Err(PathError::NotFound.into());
    }
    Ok(auth.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::CategoryService;
    use crate::database::repo_duplicates;
    use crate::database::repo_folders::insert as insert_folder;
    use crate::duplicates::finder::{scan_duplicates, DuplicateScanOptions};
    use crate::engine::classifier::Classifier;
    use crate::engine::operations::Ledger;
    use crate::engine::paths::{ApprovedRoot, RootKind};
    use crate::engine::safe_fs::SafeFs;
    use crate::engine::undo::{UndoService, UndoTarget};
    use crate::settings::MonitoringSettings;
    use crate::test_support::{mkdir, tempdir, write_file};
    use crate::utils::clock::FixedClock;
    use std::path::Path;
    use std::sync::Arc;

    async fn setup_env(
        root: &Path,
        files: &[(&str, &[u8])],
    ) -> (
        Db,
        Arc<Organizer>,
        PathAuthority,
        SafeFs,
        PathBuf,
        DuplicateGroupSummary,
    ) {
        let monitored = root.join("Monitored");
        let trash = root.join("Trash");
        mkdir(&monitored);
        mkdir(&trash);
        for (name, bytes) in files {
            write_file(&monitored.join(name), bytes);
        }

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Monitored", &monitored.to_string_lossy(), true)
            .await
            .expect("folder");

        let cat_svc = CategoryService::new(db.clone());
        cat_svc.ensure_builtins().await.expect("builtins");
        let cls = Arc::new(Classifier::new(cat_svc.list().await.expect("cats")));

        let rep = scan_duplicates(
            &db,
            cls,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            1_000,
            None,
        )
        .await
        .expect("scan");
        assert!(
            !rep.groups.groups.is_empty(),
            "fixture needs at least 1 dup group"
        );

        let safe_fs = SafeFs::with_trash_dir(trash.clone());
        let clock = Arc::new(FixedClock::utc(10_000));
        let org = Arc::new(Organizer::new(
            Ledger::new(db.clone()),
            Arc::new(safe_fs.clone()),
            clock,
        ));
        let auth = PathAuthority::new(vec![ApprovedRoot {
            path: monitored.clone(),
            kind: RootKind::Monitored,
        }]);
        (db, org, auth, safe_fs, trash, rep.groups.groups[0].clone())
    }

    #[tokio::test]
    async fn r_dup_07_keep_selected_marks_chosen_member_and_rejects_unknown() {
        let (_d, root) = tempdir();
        let payload = b"same-file-bytes";
        let (db, org, auth, _fs, _trash, group) =
            setup_env(&root, &[("a.txt", payload), ("b.txt", payload)]).await;

        let second = group
            .members
            .iter()
            .find(|m| !m.is_keeper)
            .expect("non-keeper")
            .clone();

        let rep = apply_dup_action(
            &org,
            &auth,
            DupAction::Keep {
                group_id: group.id.clone(),
                file_record_id: second.file_record_id.clone(),
            },
        )
        .await
        .expect("keep");

        assert_eq!(rep.affected, 1);
        let updated = rep.updated_group.expect("updated group");
        let keepers: Vec<&str> = updated
            .members
            .iter()
            .filter(|m| m.is_keeper)
            .map(|m| m.file_record_id.as_str())
            .collect();
        assert_eq!(keepers, vec![second.file_record_id.as_str()]);

        assert!(apply_dup_action(
            &org,
            &auth,
            DupAction::Keep {
                group_id: group.id.clone(),
                file_record_id: "nonexistent".into(),
            },
        )
        .await
        .is_err());
        let _ = db;
    }

    #[tokio::test]
    async fn r_dup_08_move_to_review_is_safe_and_undoable() {
        let (_d, root) = tempdir();
        let payload = b"review-move-payload";
        let (db, org, auth, _fs, _trash, group) = setup_env(
            &root,
            &[
                ("k.txt", payload),
                ("dup1.txt", payload),
                ("dup2.txt", payload),
            ],
        )
        .await;

        let keeper = group.members.iter().find(|m| m.is_keeper).expect("keeper");
        let non_keepers: Vec<_> = group.members.iter().filter(|m| !m.is_keeper).collect();

        let rep = apply_dup_action(
            &org,
            &auth,
            DupAction::MoveToReview {
                group_id: group.id.clone(),
                file_record_ids: None,
                review_dir: None,
            },
        )
        .await
        .expect("move to review");

        assert_eq!(rep.affected, 2);
        let batch_id = rep.batch_id.expect("batch_id");
        assert!(Path::new(&keeper.path).exists(), "keeper must stay put");
        for nk in &non_keepers {
            assert!(!Path::new(&nk.path).exists(), "non-keeper should be moved");
        }
        let review_dir = root.join("Monitored").join("Duplicate Review");
        assert!(review_dir.exists(), "review folder should be created");

        // Undo the batch restores both moved duplicates back to their original paths!
        let undo_svc = UndoService::new((*org).clone());
        let undo_rep = undo_svc
            .undo(
                UndoTarget::Batch {
                    batch_id,
                    auto_rename_conflicts: false,
                    restore_structure: false,
                },
                &auth,
            )
            .await
            .expect("undo review batch");
        assert_eq!(undo_rep.undone.len(), 2);
        for nk in &non_keepers {
            assert!(
                Path::new(&nk.path).exists(),
                "undo must restore {0}",
                nk.path
            );
        }
        let _ = db;
    }

    #[tokio::test]
    async fn r_dup_09_trash_requires_confirmation_never_trashes_all_and_verifies_hash() {
        let (_d, root) = tempdir();
        let payload = b"trash-safety-payload";
        let (db, org, auth, _fs, trash, group) = setup_env(
            &root,
            &[
                ("keep.txt", payload),
                ("dup_ok.txt", payload),
                ("dup_stale.txt", payload),
            ],
        )
        .await;

        let dup_ok = group
            .members
            .iter()
            .find(|m| m.filename == "dup_ok.txt")
            .expect("dup_ok")
            .clone();
        let dup_stale = group
            .members
            .iter()
            .find(|m| m.filename == "dup_stale.txt")
            .expect("dup_stale")
            .clone();

        // 1. Unconfirmed trash must fail without deleting anything
        assert!(apply_dup_action(
            &org,
            &auth,
            DupAction::Trash {
                group_id: group.id.clone(),
                file_record_ids: None,
                confirmed: false,
            },
        )
        .await
        .is_err());
        assert!(Path::new(&dup_ok.path).exists());

        // 2. Trying to trash ALL copies in a group must fail
        let all_ids: Vec<String> = group
            .members
            .iter()
            .map(|m| m.file_record_id.clone())
            .collect();
        assert!(apply_dup_action(
            &org,
            &auth,
            DupAction::Trash {
                group_id: group.id.clone(),
                file_record_ids: Some(all_ids),
                confirmed: true,
            },
        )
        .await
        .is_err());

        // 3. Modify dup_stale externally before trash -> must be skipped safely!
        write_file(Path::new(&dup_stale.path), b"externally-edited-bytes");

        let rep = apply_dup_action(
            &org,
            &auth,
            DupAction::Trash {
                group_id: group.id.clone(),
                file_record_ids: Some(vec![
                    dup_ok.file_record_id.clone(),
                    dup_stale.file_record_id.clone(),
                ]),
                confirmed: true,
            },
        )
        .await
        .expect("trash duplicates");

        assert_eq!(rep.affected, 1, "only matching-hash copy should be trashed");
        assert_eq!(rep.skipped.len(), 1, "modified copy must be skipped");
        assert!(!Path::new(&dup_ok.path).exists());
        assert!(
            Path::new(&dup_stale.path).exists(),
            "modified copy must NOT be trashed"
        );
        // Verify moved into Trash (not permanently deleted)
        let trashed_exists = walkdir::WalkDir::new(&trash)
            .into_iter()
            .flatten()
            .any(|e| e.file_type().is_file() && e.file_name().to_string_lossy().contains("dup_ok"));
        assert!(trashed_exists, "dup_ok must be recoverable in Trash");

        // Group drops below 2 remaining identical members (wait: group still has 2 rows until stale pruned;
        // if 2 remain in group it stays, or let's verify list_groups doesn't panic).
        let _ = repo_duplicates::list_groups(&db, true, None)
            .await
            .expect("list after trash");
    }

    #[tokio::test]
    async fn r_dup_10_export_csv_json_and_reveal_guards_unauthorized_paths() {
        let (_d, root) = tempdir();
        let payload = b"export-and-reveal-bytes";
        let (db, org, auth, _fs, _trash, group) = setup_env(
            &root,
            &[("=evil_formula.txt", payload), ("normal.txt", payload)],
        )
        .await;

        let csv = export_duplicates(&db, DupExportFormat::Csv, true)
            .await
            .expect("csv");
        assert!(csv.contains("group_id,content_hash"));
        assert!(
            csv.contains("'=evil_formula.txt"),
            "CSV export must neutralize formula injection: {csv}"
        );

        let json = export_duplicates(&db, DupExportFormat::Json, true)
            .await
            .expect("json");
        let parsed: Vec<DuplicateGroupSummary> = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].content_hash, group.content_hash);

        // Reveal valid authorized member returns canonical NFC path
        let target_id = &group.members[0].file_record_id;
        let resolved = resolve_reveal_duplicate_path(&db, &auth, target_id)
            .await
            .expect("reveal");
        assert!(resolved.ends_with(&group.members[0].filename));

        // Unknown record ID rejected
        assert!(resolve_reveal_duplicate_path(&db, &auth, "missing-id")
            .await
            .is_err());

        // Path outside PathAuthority rejected even if present in DB
        let outside = root.join("Outside").join("secret.txt");
        write_file(&outside, b"x");
        sqlx::query("UPDATE file_records SET path = ?2 WHERE id = ?1")
            .bind(target_id)
            .bind(outside.to_string_lossy().as_ref())
            .execute(&db)
            .await
            .expect("tamper path");
        assert!(resolve_reveal_duplicate_path(&db, &auth, target_id)
            .await
            .is_err());

        let _ = org;
    }

    #[tokio::test]
    async fn bug_030_cross_folder_review_exclusion_protects_all_monitored_folders() {
        use crate::categories::CategoryService;
        use crate::database::repo_folders;
        use crate::duplicates::finder::{scan_duplicates, DuplicateScanOptions};
        use crate::engine::classifier::Classifier;
        use crate::engine::paths::{ApprovedRoot, PathAuthority, RootKind};
        use crate::engine::reconcile::FolderScanSpec;
        use crate::settings::MonitoringSettings;

        let (_d, root) = tempdir();
        let dir_a = root.join("FolderA");
        let dir_b = root.join("FolderB");
        let custom_review = dir_b.join("Duplicate Review");
        std::fs::create_dir_all(&dir_a).expect("mkdir a");
        std::fs::create_dir_all(&dir_b).expect("mkdir b");

        let payload = b"cross-folder-review-exclusion-bytes";
        write_file(&dir_a.join("keeper.pdf"), payload);
        write_file(&dir_b.join("extra.pdf"), payload);

        let db = crate::database::open_in_memory().await.expect("db");
        CategoryService::new(db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        repo_folders::insert(&db, "f-a", "FolderA", &dir_a.to_string_lossy(), true)
            .await
            .expect("insert f-a");
        repo_folders::insert(&db, "f-b", "FolderB", &dir_b.to_string_lossy(), true)
            .await
            .expect("insert f-b");

        let cls = Arc::new(Classifier::new(
            CategoryService::new(db.clone()).list().await.expect("cats"),
        ));
        let scan = scan_duplicates(
            &db,
            cls,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            1_000,
            None,
        )
        .await
        .expect("scan");
        let group = &scan.groups.groups[0];

        let fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let org = Organizer::new(
            Ledger::new(db.clone()),
            fs,
            Arc::new(FixedClock::utc(2_000)),
        );
        let auth = PathAuthority::new(vec![
            ApprovedRoot {
                path: dir_a.clone(),
                kind: RootKind::Monitored,
            },
            ApprovedRoot {
                path: dir_b.clone(),
                kind: RootKind::Monitored,
            },
        ]);

        // Keeper is in f-a, extra is in f-b, review_dir is inside f-b!
        apply_dup_action(
            &org,
            &auth,
            DupAction::MoveToReview {
                group_id: group.id.clone(),
                file_record_ids: None,
                review_dir: Some(custom_review.to_string_lossy().into_owned()),
            },
        )
        .await
        .expect("move to custom review inside f-b");

        let moved_file = custom_review.join("extra.pdf");
        assert!(moved_file.exists(), "moved file must exist in review dir");

        let folder_b = repo_folders::get(&db, "f-b")
            .await
            .expect("get f-b")
            .expect("exists");
        let spec_b =
            FolderScanSpec::load_for_folder(&db, &folder_b, &MonitoringSettings::default())
                .await
                .expect("spec b");
        assert!(
            spec_b
                .rules
                .is_path_excluded(&spec_b.path, &moved_file, "extra.pdf", Some("pdf"), false, false),
            "BUG-030: Duplicate Review path exclusion must protect custom_review inside FolderB even when keeper is in FolderA"
        );
    }

    #[tokio::test]
    async fn edge_06_export_csv_escapes_quotes_commas_newlines_unicode_and_formula_prefix() {
        let (_d, root) = tempdir();
        let bytes = b"csv-export-edge-case-bytes";
        let tricky_name = "=SUM(A1),\"quoted\" 🎉 Café.csv";
        let (db, _org, _auth, _fs, _trash, _group) =
            setup_env(&root, &[(tricky_name, bytes), ("plain copy.csv", bytes)]).await;

        let csv = export_duplicates(&db, DupExportFormat::Csv, true)
            .await
            .expect("csv export");
        // Formula prefix `=` gets neutralized with leading `'` and quotes/commas trigger RFC-4180 quoting (`""`).
        assert!(
            csv.contains("\"'=SUM(A1),\"\"quoted\"\" 🎉 Café.csv\""),
            "expected formula-guarded & RFC-4180 escaped CSV cell, got:\n{csv}"
        );
    }
}
