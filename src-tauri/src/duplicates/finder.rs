//! Multi-stage duplicate finder (`P6-T02`, FR-110..113):
//!
//! 1. Size prefilter: files with unique sizes are never read or hashed.
//! 2. Partial hash (start + middle + end 64 KiB windows): eliminates size-colliding non-dups.
//! 3. Full streaming BLAKE3 hash (backed by [`crate::duplicates::cache`]): groups true duplicates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::database::repo_duplicates::{
    self, DuplicateGroupsResponse, NewDuplicateGroup, NewDuplicateMember,
};
use crate::database::repo_files::{self, TrackedHashCandidate};
use crate::database::{repo_folders, Db};
use crate::duplicates::hasher::{full_hash_file, partial_hash_file};
use crate::engine::classifier::Classifier;
use crate::engine::fs_view::{EntryKind, ReadOnlyFs, RealFs};
use crate::engine::reconcile::FolderScanSpec;
use crate::engine::scanner::{self, ScanError, ScanOptions, ScannedFile};
use crate::error::{AppError, AppResult};
use crate::folders::MonitoredFolder;
use crate::settings::MonitoringSettings;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScanOptions {
    /// Limit scanning to specific monitored folder IDs (`None` = all enabled monitored folders).
    pub folder_ids: Option<Vec<String>>,
    /// When `true`, also treat 0-byte files as duplicate candidates (`wasted_bytes = 0`).
    #[serde(default)]
    pub include_empty: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScanReport {
    pub files_scanned: u32,
    pub unique_size_skipped: u32,
    pub partial_hashed: u32,
    pub full_hashed: u32,
    pub cache_hits: u32,
    pub groups_found: u32,
    #[specta(type = f64)]
    pub total_wasted_bytes: u64,
    pub groups: DuplicateGroupsResponse,
}

/// Progress callback invoked during Stage 2/3 hashing (`(hashed, total, bytes)`).
pub type ProgressSink = Arc<dyn Fn(u32, u32, u64) + Send + Sync>;

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn cache_matches(
    cand: &TrackedHashCandidate,
    facts: &crate::engine::fs_view::FileFacts,
) -> Option<String> {
    let hash = cand.content_hash.as_ref()?;
    let size_i = to_i64(facts.size);
    if cand.dev == to_i64(facts.dev)
        && cand.ino == to_i64(facts.ino)
        && cand.size == size_i
        && cand.modified_at_ns == facts.modified_ns
        && cand.hash_size == Some(size_i)
        && cand.hash_mtime_ns == Some(facts.modified_ns)
    {
        Some(hash.clone())
    } else {
        None
    }
}

async fn resolve_target_folders(
    db: &Db,
    folder_ids: Option<&[String]>,
) -> AppResult<Vec<MonitoredFolder>> {
    let all = repo_folders::list(db).await?;
    match folder_ids {
        None => Ok(all.into_iter().filter(|f| f.enabled).collect()),
        Some(ids) => {
            let mut out = Vec::new();
            for wanted in ids {
                let found = all
                    .iter()
                    .find(|f| &f.id == wanted)
                    .cloned()
                    .ok_or_else(|| AppError::NotFound(format!("folder {wanted}")))?;
                if !out
                    .iter()
                    .any(|existing: &MonitoredFolder| existing.id == found.id)
                {
                    out.push(found);
                }
            }
            Ok(out)
        }
    }
}

async fn scan_and_refresh_folder(
    db: &Db,
    folder: &MonitoredFolder,
    classifier: Arc<Classifier>,
    monitoring: &MonitoringSettings,
    now_ms: i64,
) -> AppResult<()> {
    let spec = FolderScanSpec::load_for_folder(db, folder, monitoring).await?;
    let scan_opts = ScanOptions {
        recursive: spec.recursive,
        rules: spec.rules,
    };
    let root = spec.path;
    let batches =
        tokio::task::spawn_blocking(move || -> Result<Vec<Vec<ScannedFile>>, ScanError> {
            let mut collected = Vec::new();
            scanner::scan(&root, &scan_opts, &classifier, |batch| {
                collected.push(batch);
                Ok(())
            })?;
            Ok(collected)
        })
        .await
        .map_err(|e| AppError::Internal(format!("duplicate scan task failed: {e}")))?
        .map_err(|e| match e {
            ScanError::NotFound => AppError::NotFound(format!("folder {}", folder.name)),
            ScanError::PermissionDenied => {
                AppError::Internal("permission denied scanning folder".into())
            }
            ScanError::Io(m) | ScanError::Sink(m) => AppError::Internal(m),
        })?;

    let stamp = repo_files::next_scan_stamp(db, &folder.id, now_ms).await?;
    for batch in &batches {
        repo_files::upsert_batch(db, &folder.id, batch, stamp).await?;
    }
    repo_files::delete_not_seen(db, &folder.id, stamp).await?;
    repo_folders::record_scan(db, &folder.id, stamp).await?;
    Ok(())
}

/// Runs a read-only multi-stage duplicate scan across monitored folders and updates
/// `duplicate_groups` / `duplicate_members` in SQLite.
pub async fn scan_duplicates(
    db: &Db,
    classifier: Arc<Classifier>,
    monitoring: &MonitoringSettings,
    opts: DuplicateScanOptions,
    now_ms: i64,
    progress: Option<ProgressSink>,
) -> AppResult<DuplicateScanReport> {
    if opts.folder_ids.as_ref().is_some_and(Vec::is_empty) {
        let groups = repo_duplicates::list_groups(db, false, None).await?;
        return Ok(DuplicateScanReport {
            files_scanned: 0,
            unique_size_skipped: 0,
            partial_hashed: 0,
            full_hashed: 0,
            cache_hits: 0,
            groups_found: groups.totals.active_groups,
            total_wasted_bytes: groups.totals.total_wasted_bytes,
            groups,
        });
    }
    let folders = resolve_target_folders(db, opts.folder_ids.as_deref()).await?;
    for folder in &folders {
        scan_and_refresh_folder(db, folder, classifier.clone(), monitoring, now_ms).await?;
    }

    let folder_ids: Vec<String> = folders.into_iter().map(|f| f.id).collect();
    let candidates = repo_files::list_tracked_candidates_for_folders(db, Some(&folder_ids)).await?;
    let files_scanned = u32::try_from(candidates.len()).unwrap_or(u32::MAX);

    // Stage 1: Group by file size; skip unique sizes (never read or hash them).
    let mut by_size: HashMap<u64, Vec<TrackedHashCandidate>> = HashMap::new();
    for c in candidates {
        let size_u = u64::try_from(c.size).unwrap_or(0);
        if size_u == 0 && !opts.include_empty {
            continue;
        }
        by_size.entry(size_u).or_default().push(c);
    }

    let mut report = DuplicateScanReport {
        files_scanned,
        ..Default::default()
    };

    let mut stage2_buckets: Vec<(u64, Vec<TrackedHashCandidate>)> = Vec::new();
    for (size_u, bucket) in by_size {
        if bucket.len() < 2 {
            report.unique_size_skipped = report.unique_size_skipped.saturating_add(1);
        } else {
            stage2_buckets.push((size_u, bucket));
        }
    }

    // Collect pre-verified cache hits (only when ALL members in a size bucket hit valid cache)
    // and Stage-2 partial-hash candidates.
    let mut resolved_hashes: Vec<(String, TrackedHashCandidate)> = Vec::new();
    let mut stage3_candidates: Vec<TrackedHashCandidate> = Vec::new();

    for (size_u, bucket) in stage2_buckets {
        let all_cached = tokio::task::spawn_blocking(move || {
            let mut cached = Vec::with_capacity(bucket.len());
            let mut missing_any = false;
            for c in &bucket {
                if let Ok(facts) = RealFs.stat(Path::new(&c.path)) {
                    if facts.kind == EntryKind::File {
                        if let Some(h) = cache_matches(c, &facts) {
                            cached.push((h, c.clone()));
                            continue;
                        }
                    }
                }
                missing_any = true;
                break;
            }
            if missing_any {
                Err(bucket)
            } else {
                Ok(cached)
            }
        })
        .await
        .map_err(|e| AppError::Internal(format!("cache check failed: {e}")))?;

        match all_cached {
            Ok(hits) => {
                report.cache_hits = report
                    .cache_hits
                    .saturating_add(u32::try_from(hits.len()).unwrap_or(u32::MAX));
                resolved_hashes.extend(hits);
            }
            Err(bucket) => {
                // Stage 2: Partial hash eliminates size-colliding files differing at start/mid/end.
                let partial_res = tokio::task::spawn_blocking(move || {
                    let mut by_partial: HashMap<String, Vec<TrackedHashCandidate>> = HashMap::new();
                    let mut partial_count: u32 = 0;
                    for c in bucket {
                        match partial_hash_file(&RealFs, Path::new(&c.path), size_u) {
                            Ok(ph) => {
                                partial_count = partial_count.saturating_add(1);
                                by_partial.entry(ph).or_default().push(c);
                            }
                            Err(err) => {
                                tracing::warn!(
                                    target: "folderflow::duplicates",
                                    error = %err,
                                    "skipping unreadable duplicate candidate in stage 2"
                                );
                            }
                        }
                    }
                    (partial_count, by_partial)
                })
                .await
                .map_err(|e| AppError::Internal(format!("partial hash failed: {e}")))?;

                report.partial_hashed = report.partial_hashed.saturating_add(partial_res.0);
                for (_, sub) in partial_res.1 {
                    if sub.len() >= 2 {
                        stage3_candidates.extend(sub);
                    }
                }
            }
        }
    }

    // Stage 3: Full streaming BLAKE3 hash with cache lookup & progress events.
    let total_stage3 = u32::try_from(stage3_candidates.len()).unwrap_or(u32::MAX);
    let mut processed_bytes = 0u64;
    if let Some(cb) = &progress {
        cb(0, total_stage3, 0);
    }

    for (idx, c) in stage3_candidates.into_iter().enumerate() {
        let p = PathBuf::from(&c.path);
        let c_clone = c.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let facts = RealFs.stat(&p)?;
            if facts.kind != EntryKind::File {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "not a regular file",
                ));
            }
            if let Some(hit) = cache_matches(&c_clone, &facts) {
                return Ok((hit, true, facts));
            }
            let h = full_hash_file(&RealFs, &p)?;
            Ok((h, false, facts))
        })
        .await
        .map_err(|e| AppError::Internal(format!("full hash task failed: {e}")))?;

        match outcome {
            Ok((hash, from_cache, facts)) => {
                if from_cache {
                    report.cache_hits = report.cache_hits.saturating_add(1);
                } else {
                    report.full_hashed = report.full_hashed.saturating_add(1);
                    repo_files::update_content_hash(
                        db,
                        &c.path,
                        facts.dev,
                        facts.ino,
                        facts.size,
                        facts.modified_ns,
                        &hash,
                    )
                    .await?;
                }
                processed_bytes = processed_bytes.saturating_add(facts.size);
                resolved_hashes.push((hash, c));
            }
            Err(err) => {
                tracing::warn!(
                    target: "folderflow::duplicates",
                    error = %err,
                    "skipping unreadable duplicate candidate in stage 3"
                );
            }
        }

        if let Some(cb) = &progress {
            cb(
                u32::try_from(idx + 1).unwrap_or(u32::MAX),
                total_stage3,
                processed_bytes,
            );
        }
    }

    // Group by (content_hash, size) — retain only true duplicate sets (>= 2 members).
    let mut by_hash: HashMap<(String, u64), Vec<NewDuplicateMember>> = HashMap::new();
    for (hash, c) in resolved_hashes {
        let size_u = u64::try_from(c.size).unwrap_or(0);
        by_hash
            .entry((hash, size_u))
            .or_default()
            .push(NewDuplicateMember {
                file_record_id: c.id,
                path: c.path,
                modified_ns: c.modified_at_ns,
                created_ns: None,
            });
    }

    let new_groups: Vec<NewDuplicateGroup> = by_hash
        .into_iter()
        .filter(|(_, members)| members.len() >= 2)
        .map(|((content_hash, file_size), members)| NewDuplicateGroup {
            content_hash,
            file_size,
            members,
        })
        .collect();

    repo_duplicates::replace_groups_from_scan(db, &new_groups, opts.folder_ids.as_deref(), now_ms)
        .await?;
    let groups = repo_duplicates::list_groups(db, false, None).await?;
    report.groups_found = groups.totals.active_groups;
    report.total_wasted_bytes = groups.totals.total_wasted_bytes;
    report.groups = groups;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::CategoryService;
    use crate::database::repo_duplicates;
    use crate::database::repo_folders::insert as insert_folder;
    use crate::test_support::{mkdir, snapshot_tree, symlink, tempdir, write_file};

    async fn test_classifier(db: &Db) -> Arc<Classifier> {
        let svc = CategoryService::new(db.clone());
        svc.ensure_builtins().await.expect("builtins");
        let cats = svc.list().await.expect("cats");
        Arc::new(Classifier::new(cats))
    }

    #[tokio::test]
    async fn r_dup_01_size_prefilter_skips_hashing_unique_sizes() {
        let (_d, root) = tempdir();
        write_file(&root.join("a.txt"), b"alpha");
        write_file(&root.join("b.txt"), b"beta-longer");
        write_file(&root.join("c.txt"), b"gamma-even-longer");

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            1_000,
            None,
        )
        .await
        .expect("scan");

        assert_eq!(rep.files_scanned, 3);
        assert_eq!(rep.unique_size_skipped, 3);
        assert_eq!(rep.partial_hashed, 0, "unique sizes must never be hashed");
        assert_eq!(rep.full_hashed, 0);
        assert_eq!(rep.groups_found, 0);
    }

    #[tokio::test]
    async fn r_dup_02_partial_hash_eliminates_size_colliding_non_duplicates() {
        let (_d, root) = tempdir();
        let size = 512 * 1024;
        let a = vec![0x11u8; size];
        let mut b = a.clone();
        b[size / 2] ^= 0xFF; // same length & identical start/end, differs in middle

        write_file(&root.join("one.bin"), &a);
        write_file(&root.join("two.bin"), &b);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            1_000,
            None,
        )
        .await
        .expect("scan");

        assert_eq!(rep.files_scanned, 2);
        assert_eq!(rep.partial_hashed, 2);
        assert_eq!(
            rep.full_hashed, 0,
            "partial hash difference must short-circuit full hashing"
        );
        assert_eq!(rep.groups_found, 0);
    }

    #[tokio::test]
    async fn r_dup_03_full_hash_groups_true_dups_and_splits_partial_collisions() {
        let (_d, root) = tempdir();
        let size = 512 * 1024;
        let a = vec![0x22u8; size];
        let mut b_partial_collision = a.clone();
        // Mutate between start window [0..64K] and mid window [224K..288K], so Stage 2 matches
        b_partial_collision[90 * 1024] ^= 0xFF;

        write_file(&root.join("true_1.bin"), &a);
        write_file(&root.join("true_2.bin"), &a);
        write_file(&root.join("pseudo.bin"), &b_partial_collision);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            1_000,
            None,
        )
        .await
        .expect("scan");

        assert_eq!(rep.partial_hashed, 3);
        assert_eq!(rep.full_hashed, 3);
        assert_eq!(rep.groups_found, 1);
        assert_eq!(rep.groups.groups[0].copy_count, 2);
        let filenames: Vec<&str> = rep.groups.groups[0]
            .members
            .iter()
            .map(|m| m.filename.as_str())
            .collect();
        assert!(filenames.contains(&"true_1.bin"));
        assert!(filenames.contains(&"true_2.bin"));
        assert!(!filenames.contains(&"pseudo.bin"));
    }

    #[tokio::test]
    async fn r_dup_04_detects_same_content_different_names_across_folders() {
        let (_d, root) = tempdir();
        let f_a = root.join("FolderA");
        let f_b = root.join("FolderB");
        mkdir(&f_a);
        mkdir(&f_b);
        let bytes = b"identical-document-payload-for-duplicate-check";
        write_file(&f_a.join("original.pdf"), bytes);
        write_file(&f_b.join("copy_with_other_name.pdf"), bytes);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "fa", "FolderA", &f_a.to_string_lossy(), true)
            .await
            .expect("fa");
        insert_folder(&db, "fb", "FolderB", &f_b.to_string_lossy(), true)
            .await
            .expect("fb");

        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            2_000,
            None,
        )
        .await
        .expect("scan");

        assert_eq!(rep.groups_found, 1);
        let g = &rep.groups.groups[0];
        assert!(g.same_content_different_names);
        assert!(!g.all_same_name);
        let folders: Vec<&str> = g.members.iter().map(|m| m.folder_id.as_str()).collect();
        assert!(folders.contains(&"fa") && folders.contains(&"fb"));
    }

    #[tokio::test]
    async fn r_dup_05_copy_count_wasted_bytes_and_ignored_groups() {
        let (_d, root) = tempdir();
        let data = vec![0x5Au8; 4096];
        write_file(&root.join("c1.dat"), &data);
        write_file(&root.join("c2.dat"), &data);
        write_file(&root.join("c3.dat"), &data);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            3_000,
            None,
        )
        .await
        .expect("scan");

        assert_eq!(rep.groups_found, 1);
        assert_eq!(rep.groups.groups[0].copy_count, 3);
        assert_eq!(rep.groups.groups[0].wasted_bytes, 8192);
        assert_eq!(rep.total_wasted_bytes, 8192);
        assert_eq!(rep.groups.totals.total_duplicate_copies, 2);

        // Ignoring the group excludes it from active wasted bytes, and rescanning preserves ignored=true.
        repo_duplicates::set_group_ignored(&db, &rep.groups.groups[0].id, true)
            .await
            .expect("ignore");
        let after_ignore = repo_duplicates::list_groups(&db, false, None)
            .await
            .expect("list active");
        assert_eq!(after_ignore.totals.total_wasted_bytes, 0);
        assert_eq!(after_ignore.totals.ignored_groups, 1);
        assert!(after_ignore.groups.is_empty());

        let rescan = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            4_000,
            None,
        )
        .await
        .expect("rescan");
        assert_eq!(rescan.groups.totals.ignored_groups, 1);
        assert_eq!(rescan.total_wasted_bytes, 0);
    }

    #[tokio::test]
    async fn r_dup_06_zero_filesystem_writes_and_skips_symlinks_and_packages() {
        let (_d, root) = tempdir();
        let data = b"duplicate-content";
        let f1 = root.join("a.txt");
        write_file(&f1, data);
        write_file(&root.join("b.txt"), data);
        // Symlink pointing to a.txt must not be counted as a third duplicate!
        symlink(&f1, root.join("alias.txt"));
        // .app package contents must be skipped
        write_file(&root.join("App.app/Contents/MacOS/bin"), data);
        write_file(&root.join("Other.app/Contents/MacOS/bin"), data);
        // Partial download must be ignored
        write_file(&root.join("part1.download"), data);
        write_file(&root.join("part2.download"), data);

        let before = snapshot_tree(&root);
        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            5_000,
            None,
        )
        .await
        .expect("scan");

        assert_eq!(rep.groups_found, 1);
        assert_eq!(rep.groups.groups[0].copy_count, 2);
        let after = snapshot_tree(&root);
        assert_eq!(
            before, after,
            "duplicate finder must never mutate the filesystem"
        );
    }

    #[tokio::test]
    async fn bug_026_duplicate_scan_honors_global_glob_and_extension_exclusions() {
        use crate::database::repo_exclusions::{self, ExclusionInput, ExclusionKind};

        let (_d, root) = tempdir();
        let data = b"identical-payload-for-exclusions";
        // Pair 1: blocked by global glob exclusion (`skip-*.pdf`)
        write_file(&root.join("skip-a.pdf"), data);
        write_file(&root.join("skip-b.pdf"), data);
        // Pair 2: blocked by global extension exclusion (`iso`)
        let iso_data = b"iso-duplicate-payload-123";
        write_file(&root.join("disk1.iso"), iso_data);
        write_file(&root.join("disk2.iso"), iso_data);
        // Pair 3: allowed regular duplicate pair
        let keep_data = b"normal-duplicate-payload";
        write_file(&root.join("doc1.txt"), keep_data);
        write_file(&root.join("doc2.txt"), keep_data);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        repo_exclusions::create(
            &db,
            ExclusionInput {
                folder_id: None,
                path: "skip-*.pdf".into(),
                exclusion_type: ExclusionKind::Glob,
            },
        )
        .await
        .expect("glob excl");
        repo_exclusions::create(
            &db,
            ExclusionInput {
                folder_id: None,
                path: "iso".into(),
                exclusion_type: ExclusionKind::Extension,
            },
        )
        .await
        .expect("ext excl");

        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            6_000,
            None,
        )
        .await
        .expect("scan");

        assert_eq!(
            rep.groups_found, 1,
            "BUG-026: excluded globs and extensions must not appear in duplicate groups"
        );
        assert_eq!(
            rep.groups.groups[0].members[0].extension.as_deref(),
            Some("txt")
        );
    }

    #[tokio::test]
    async fn bug_028_ignored_only_scan_reports_zero_active_groups() {
        let (_d, root) = tempdir();
        let data = vec![0x42u8; 4096];
        write_file(&root.join("a.bin"), &data);
        write_file(&root.join("b.bin"), &data);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        let first = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            1_000,
            None,
        )
        .await
        .expect("first scan");
        assert_eq!(first.groups_found, 1);

        repo_duplicates::set_group_ignored(&db, &first.groups.groups[0].id, true)
            .await
            .expect("ignore");

        let rescan = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            2_000,
            None,
        )
        .await
        .expect("rescan");

        assert_eq!(
            rescan.groups_found, 0,
            "BUG-028: ignored-only scan must report 0 active groups found, not 1"
        );
        assert_eq!(rescan.groups.totals.active_groups, 0);
        assert_eq!(rescan.groups.totals.ignored_groups, 1);
    }

    #[tokio::test]
    async fn bug_029_scoped_scan_preserves_unscanned_groups_and_handles_empty_folder_ids() {
        let (_d, root) = tempdir();
        let dir_a = root.join("FolderA");
        let dir_b = root.join("FolderB");
        let payload_a = vec![0x11u8; 2048];
        let payload_b = vec![0x22u8; 3072];
        write_file(&dir_a.join("a1.dat"), &payload_a);
        write_file(&dir_a.join("a2.dat"), &payload_a);
        write_file(&dir_b.join("b1.dat"), &payload_b);
        write_file(&dir_b.join("b2.dat"), &payload_b);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f-a", "A", &dir_a.to_string_lossy(), true)
            .await
            .expect("f-a");
        insert_folder(&db, "f-b", "B", &dir_b.to_string_lossy(), true)
            .await
            .expect("f-b");

        let full = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            1_000,
            None,
        )
        .await
        .expect("full scan");
        assert_eq!(full.groups.groups.len(), 2);

        // Empty folder_ids filter must NOT wipe existing duplicate groups in SQLite.
        let empty_scan = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions {
                folder_ids: Some(Vec::new()),
                ..Default::default()
            },
            2_000,
            None,
        )
        .await
        .expect("empty scoped scan");
        assert_eq!(
            empty_scan.groups.totals.active_groups, 2,
            "BUG-029: empty folder_ids scan must not wipe existing duplicate groups"
        );

        // Scanning only FolderA must preserve FolderB's duplicate group.
        let scoped = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &MonitoringSettings::default(),
            DuplicateScanOptions {
                folder_ids: Some(vec!["f-a".into()]),
                ..Default::default()
            },
            3_000,
            None,
        )
        .await
        .expect("scoped scan");
        assert_eq!(
            scoped.groups.totals.active_groups, 2,
            "BUG-029: scanning only f-a must preserve existing groups in unscanned f-b"
        );
    }

    #[tokio::test]
    async fn edge_06_duplicate_scan_handles_unicode_emoji_leading_dot_double_ext_spaces_and_255b_names(
    ) {
        let (_d, root) = tempdir();
        // Pair 1: Unicode NFC + emoji + spaces + double extension `.tar.gz`
        let arch_bytes = b"archive-duplicate-payload-with-unicode-caf\xc3\xa9";
        write_file(&root.join("Caf\u{00e9} 🎉 notes.tar.gz"), arch_bytes);
        write_file(&root.join("Caf\u{00e9} 🎉 notes copy.tar.gz"), arch_bytes);

        // Pair 2: Leading dot filenames (hidden = true unless hidden skipped; note MonitoringSettings default includes or excludes hidden? Test explicit!)
        let dot_bytes = b"dotfile-secret-payload-duplicate-123";
        write_file(&root.join(".env.backup"), dot_bytes);
        write_file(&root.join(".env.backup.2"), dot_bytes);

        // Pair 3: 255-byte filenames (max POSIX NAME_MAX)
        let long_a = format!("{}.bin", "a".repeat(251));
        let long_b = format!("{}.bin", "b".repeat(251));
        assert_eq!(long_a.len(), 255);
        assert_eq!(long_b.len(), 255);
        let long_bytes = b"255-byte-filename-duplicate-payload";
        write_file(&root.join(&long_a), long_bytes);
        write_file(&root.join(&long_b), long_bytes);

        let db = crate::database::open_in_memory().await.expect("db");
        insert_folder(&db, "f1", "Root", &root.to_string_lossy(), true)
            .await
            .expect("folder");

        let monitoring = MonitoringSettings {
            ignore_hidden_files: false,
            ..Default::default()
        };
        let rep = scan_duplicates(
            &db,
            test_classifier(&db).await,
            &monitoring,
            DuplicateScanOptions::default(),
            5_000,
            None,
        )
        .await
        .expect("scan edge-case filenames");

        assert_eq!(
            rep.groups.totals.active_groups, 3,
            "all 3 edge-case pairs (unicode/emoji/.tar.gz, .dot, 255B) must form duplicate groups"
        );
        assert_eq!(rep.groups.totals.total_duplicate_copies, 3);
    }
}
