//! Hash cache backed by SQLite `file_records` and keyed by `(dev, ino, size, modified_ns)`
//! (`P6-T01`, FR-112).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::database::repo_files::CachedFileHash;
use crate::database::{repo_files, Db};
use crate::duplicates::hasher::full_hash_file;
use crate::engine::fs_view::{EntryKind, FileFacts, ReadOnlyFs, RealFs};
use crate::engine::paths::nfc;
use crate::engine::planner::PlanCandidate;
use crate::error::{AppError, AppResult};
use crate::rules::models::{rules_hash_requirements, Rule};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedHashResult {
    pub hash: String,
    pub from_cache: bool,
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn fresh_cached_hash(rec: &CachedFileHash, facts: &FileFacts) -> Option<String> {
    let size_i = to_i64(facts.size);
    let dev_i = to_i64(facts.dev);
    let ino_i = to_i64(facts.ino);
    if rec.dev == dev_i
        && rec.ino == ino_i
        && rec.size == size_i
        && rec.modified_at_ns == facts.modified_ns
        && rec.hash_size == Some(size_i)
        && rec.hash_mtime_ns == Some(facts.modified_ns)
    {
        return rec.content_hash.clone();
    }
    None
}

async fn blocking_full_hash(path: PathBuf) -> AppResult<String> {
    tokio::task::spawn_blocking(move || full_hash_file(&RealFs, &path))
        .await
        .map_err(|e| AppError::Internal(format!("hash task failed: {e}")))?
        .map_err(AppError::from)
}

async fn resolve_with_real_fs(
    db: &Db,
    path_nfc: &Path,
    facts: &FileFacts,
    cached_rec: Option<&CachedFileHash>,
) -> AppResult<String> {
    let path_str = path_nfc.to_string_lossy();
    let owned_rec;
    let rec_opt = match cached_rec {
        Some(r) => Some(r),
        None => {
            owned_rec = repo_files::get_hash_record_by_path(db, path_str.as_ref()).await?;
            owned_rec.as_ref()
        }
    };
    if let Some(rec) = rec_opt {
        if let Some(hash) = fresh_cached_hash(rec, facts) {
            return Ok(hash);
        }
    }
    let hash = blocking_full_hash(path_nfc.to_path_buf()).await?;
    repo_files::update_content_hash(
        db,
        path_str.as_ref(),
        facts.dev,
        facts.ino,
        facts.size,
        facts.modified_ns,
        &hash,
    )
    .await?;
    Ok(hash)
}

/// Returns the cached BLAKE3 hash if `(dev, ino, size, modified_ns)` matches live metadata;
/// otherwise hashes via `fs` (streaming fixed buffer) and persists the refreshed fingerprint.
pub async fn get_or_hash_file(
    db: &Db,
    fs: &dyn ReadOnlyFs,
    path: &Path,
) -> AppResult<CachedHashResult> {
    let facts = fs.stat(path)?;
    if facts.kind != EntryKind::File {
        return Err(AppError::validation(
            "path",
            "Only regular files can be hashed",
        ));
    }

    let path_nfc = nfc(path);
    let path_str = path_nfc.to_string_lossy();

    if let Some(rec) = repo_files::get_hash_record_by_path(db, path_str.as_ref()).await? {
        if let Some(hash) = fresh_cached_hash(&rec, &facts) {
            return Ok(CachedHashResult {
                hash,
                from_cache: true,
            });
        }
    }

    let hash = full_hash_file(fs, path)?;
    repo_files::update_content_hash(
        db,
        path_str.as_ref(),
        facts.dev,
        facts.ino,
        facts.size,
        facts.modified_ns,
        &hash,
    )
    .await?;

    Ok(CachedHashResult {
        hash,
        from_cache: false,
    })
}

/// Populates [`PlanCandidate::content_hash`] and [`PlanCandidate::is_duplicate`] for candidates
/// whenever active rules inspect `ContentHash`/`DuplicateStatus` or `hash_on_scan` is enabled (`BUG-027`).
///
/// Performs zero DB queries and zero file I/O when neither condition is present in `rules`
/// and `hash_on_scan` is `false`.
pub async fn hydrate_candidates_for_rules(
    db: &Db,
    candidates: &mut [PlanCandidate],
    rules: &[Rule],
    hash_on_scan: bool,
) -> AppResult<()> {
    let (need_hash, need_dup) = rules_hash_requirements(rules);
    if candidates.is_empty() || (!need_hash && !need_dup && !hash_on_scan) {
        return Ok(());
    }

    let paths: Vec<PathBuf> = candidates.iter().map(|c| nfc(&c.path)).collect();
    let stats: Vec<Option<FileFacts>> = tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| RealFs.stat(&p).ok().filter(|f| f.kind == EntryKind::File))
            .collect()
    })
    .await
    .map_err(|e| AppError::Internal(format!("stat task failed: {e}")))?;

    let mut cand_paths: HashSet<PathBuf> = HashSet::new();
    let mut size_counts: HashMap<u64, usize> = HashMap::new();
    for (idx, facts_opt) in stats.iter().enumerate() {
        if let Some(facts) = facts_opt {
            cand_paths.insert(nfc(&candidates[idx].path));
            if facts.size > 0 {
                *size_counts.entry(facts.size).or_insert(0) += 1;
            }
        }
    }

    let mut db_by_size: HashMap<u64, Vec<CachedFileHash>> = HashMap::new();
    if need_dup {
        let mut sizes: Vec<i64> = size_counts.keys().copied().map(to_i64).collect();
        sizes.sort_unstable();
        sizes.dedup();
        for rec in repo_files::list_tracked_by_sizes(db, &sizes).await? {
            if rec.size > 0 && !cand_paths.contains(&nfc(Path::new(&rec.path))) {
                if let Ok(sz) = u64::try_from(rec.size) {
                    db_by_size.entry(sz).or_default().push(rec);
                }
            }
        }
    }

    let mut cand_sizes: Vec<Option<u64>> = vec![None; candidates.len()];
    for (idx, facts_opt) in stats.iter().enumerate() {
        let Some(facts) = facts_opt else {
            continue;
        };
        cand_sizes[idx] = Some(facts.size);
        let size_collides = need_dup
            && facts.size > 0
            && (size_counts.get(&facts.size).copied().unwrap_or(0) >= 2
                || db_by_size.contains_key(&facts.size));
        if !(need_hash || hash_on_scan || size_collides) {
            continue;
        }
        if candidates[idx].content_hash.is_none() {
            let p_nfc = nfc(&candidates[idx].path);
            if let Ok(hash) = resolve_with_real_fs(db, &p_nfc, facts, None).await {
                candidates[idx].content_hash = Some(hash);
            }
        }
    }

    if !need_dup {
        return Ok(());
    }

    let mut hash_paths: HashMap<(u64, String), HashSet<PathBuf>> = HashMap::new();
    for (idx, cand) in candidates.iter().enumerate() {
        if let (Some(size), Some(hash)) = (cand_sizes[idx], cand.content_hash.as_ref()) {
            if size > 0 {
                hash_paths
                    .entry((size, hash.clone()))
                    .or_default()
                    .insert(nfc(&cand.path));
            }
        }
    }
    for (idx, cand) in candidates.iter_mut().enumerate() {
        if let (Some(size), Some(hash)) = (cand_sizes[idx], cand.content_hash.as_ref()) {
            if size > 0
                && hash_paths
                    .get(&(size, hash.clone()))
                    .is_some_and(|set| set.len() >= 2)
            {
                cand.is_duplicate = true;
            }
        }
    }

    let mut resolved_other_hashes: HashMap<String, Option<String>> = HashMap::new();
    for (idx, cand) in candidates.iter_mut().enumerate() {
        if cand.is_duplicate {
            continue;
        }
        let (Some(size), Some(cand_hash)) = (cand_sizes[idx], cand.content_hash.as_ref()) else {
            continue;
        };
        if size == 0 {
            continue;
        }
        let Some(other_recs) = db_by_size.get(&size) else {
            continue;
        };
        for rec in other_recs {
            if let Some(cached) = resolved_other_hashes.get(&rec.path) {
                if cached.as_deref() == Some(cand_hash.as_str()) {
                    cand.is_duplicate = true;
                    break;
                }
                continue;
            }
            let other_path = nfc(Path::new(&rec.path));
            let stat_path = other_path.clone();
            let other_facts = tokio::task::spawn_blocking(move || {
                RealFs
                    .stat(&stat_path)
                    .ok()
                    .filter(|f| f.kind == EntryKind::File)
            })
            .await
            .map_err(|e| AppError::Internal(format!("stat task failed: {e}")))?;
            let resolved = match other_facts {
                Some(f) if f.size == size => resolve_with_real_fs(db, &other_path, &f, Some(rec))
                    .await
                    .ok(),
                _ => None,
            };
            let matches = resolved.as_deref() == Some(cand_hash.as_str());
            resolved_other_hashes.insert(rec.path.clone(), resolved);
            if matches {
                cand.is_duplicate = true;
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repo_files::upsert_batch;
    use crate::database::repo_folders::insert as insert_folder;
    use crate::engine::classifier::Confidence;
    use crate::engine::fs_view::RealFs;
    use crate::engine::scanner::ScannedFile;
    use crate::test_support::{set_file_mtime_secs, tempdir, write_file};
    use std::path::PathBuf;

    async fn setup_db_with_file(dir: &Path, name: &str, bytes: &[u8]) -> (Db, PathBuf, String) {
        let db = crate::database::open_in_memory().await.expect("db");
        let folder_id = "f-test".to_string();
        insert_folder(&db, &folder_id, "Test", &dir.to_string_lossy(), false)
            .await
            .expect("folder");
        let p = dir.join(name);
        write_file(&p, bytes);
        let facts = RealFs.stat(&p).expect("stat");
        upsert_batch(
            &db,
            &folder_id,
            &[ScannedFile {
                facts,
                category: "documents".into(),
                mime: None,
                confidence: Confidence::High,
            }],
            1_000,
        )
        .await
        .expect("upsert");
        (db, p, folder_id)
    }

    #[tokio::test]
    async fn r_hash_04_cache_hits_and_invalidates_on_mtime_size_or_inode_change() {
        let (_d, root) = tempdir();
        let (db, path, folder_id) = setup_db_with_file(&root, "report.pdf", b"v1-bytes").await;

        // 1. Cold miss -> computes & caches
        let first = get_or_hash_file(&db, &RealFs, &path).await.expect("first");
        assert!(!first.from_cache);
        assert_eq!(first.hash, blake3::hash(b"v1-bytes").to_hex().to_string());

        // 2. Second lookup with unchanged (dev, ino, size, mtime) -> cache hit
        let second = get_or_hash_file(&db, &RealFs, &path).await.expect("second");
        assert!(second.from_cache);
        assert_eq!(second.hash, first.hash);

        // 3. Same size, different mtime -> cache invalidated & re-hashed
        let alt_mtime_bytes = b"v2-bytes"; // exact same byte length!
        write_file(&path, alt_mtime_bytes);
        set_file_mtime_secs(&path, 1_900_000_000);
        let after_mtime = get_or_hash_file(&db, &RealFs, &path)
            .await
            .expect("after mtime");
        assert!(
            !after_mtime.from_cache,
            "mtime change must invalidate cache"
        );
        assert_eq!(
            after_mtime.hash,
            blake3::hash(alt_mtime_bytes).to_hex().to_string()
        );

        // Warm again -> hit
        assert!(
            get_or_hash_file(&db, &RealFs, &path)
                .await
                .expect("warm")
                .from_cache
        );

        // 4. Size change + rescan upsert -> invalidated
        write_file(&path, b"longer-v3-content");
        let facts = RealFs.stat(&path).expect("stat v3");
        upsert_batch(
            &db,
            &folder_id,
            &[ScannedFile {
                facts,
                category: "documents".into(),
                mime: None,
                confidence: Confidence::High,
            }],
            2_000,
        )
        .await
        .expect("rescan");
        let after_size = get_or_hash_file(&db, &RealFs, &path)
            .await
            .expect("after size");
        assert!(!after_size.from_cache, "size change must invalidate cache");
        assert_eq!(
            after_size.hash,
            blake3::hash(b"longer-v3-content").to_hex().to_string()
        );

        // 5. Simulated inode change in DB -> cache invalidated even if size/mtime equal
        sqlx::query("UPDATE file_records SET ino = ino + 999999 WHERE path = ?1")
            .bind(path.to_string_lossy().as_ref())
            .execute(&db)
            .await
            .expect("tweak ino");
        let after_ino = get_or_hash_file(&db, &RealFs, &path)
            .await
            .expect("after ino");
        assert!(
            !after_ino.from_cache,
            "inode mismatch must invalidate cache"
        );
        assert_eq!(after_ino.hash, after_size.hash);
    }
}
