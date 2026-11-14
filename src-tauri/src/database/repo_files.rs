//! `file_records`: what FolderFlow last saw on disk. Never the source of truth
//! for whether a file exists — callers always re-stat (DATABASE.md).

use std::path::PathBuf;

use super::Db;
use crate::engine::classifier::Confidence;
use crate::engine::fs_view::EntryKind;
use crate::engine::scanner::ScannedFile;
use crate::error::AppResult;

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Upserts a batch in one transaction and returns the paths that are new or
/// whose size / mtime / inode changed. A change also invalidates the cached
/// content hash (FR-112).
pub async fn upsert_batch(
    db: &Db,
    folder_id: &str,
    files: &[ScannedFile],
    seen_at: i64,
) -> AppResult<Vec<PathBuf>> {
    let mut changed = Vec::new();
    let mut tx = db.begin().await?;
    for f in files {
        let path = f.facts.path.to_string_lossy();
        let (size, ino, dev) = (
            to_i64(f.facts.size),
            to_i64(f.facts.ino),
            to_i64(f.facts.dev),
        );
        let prev: Option<(i64, i64, i64)> =
            sqlx::query_as("SELECT size, modified_at_ns, ino FROM file_records WHERE path = ?1")
                .bind(path.as_ref())
                .fetch_optional(&mut *tx)
                .await?;
        if prev != Some((size, f.facts.modified_ns, ino)) {
            changed.push(f.facts.path.clone());
        }
        sqlx::query(
            "INSERT INTO file_records (id, folder_id, path, filename, extension, category, size, dev, ino, \
             created_at_ns, modified_at_ns, last_scanned_at, mime, confidence, is_package) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
             ON CONFLICT(path) DO UPDATE SET \
               content_hash = CASE WHEN file_records.size = excluded.size AND file_records.modified_at_ns = excluded.modified_at_ns \
                 AND file_records.ino = excluded.ino AND file_records.dev = excluded.dev THEN file_records.content_hash ELSE NULL END, \
               hash_size = CASE WHEN file_records.size = excluded.size AND file_records.modified_at_ns = excluded.modified_at_ns \
                 AND file_records.ino = excluded.ino AND file_records.dev = excluded.dev THEN file_records.hash_size ELSE NULL END, \
               hash_mtime_ns = CASE WHEN file_records.size = excluded.size AND file_records.modified_at_ns = excluded.modified_at_ns \
                 AND file_records.ino = excluded.ino AND file_records.dev = excluded.dev THEN file_records.hash_mtime_ns ELSE NULL END, \
               folder_id = excluded.folder_id, filename = excluded.filename, extension = excluded.extension, \
               category = excluded.category, size = excluded.size, dev = excluded.dev, ino = excluded.ino, \
               created_at_ns = excluded.created_at_ns, modified_at_ns = excluded.modified_at_ns, \
               last_scanned_at = excluded.last_scanned_at, mime = excluded.mime, confidence = excluded.confidence, \
               is_package = excluded.is_package",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(folder_id)
        .bind(path.as_ref())
        .bind(&f.facts.name)
        .bind(&f.facts.extension)
        .bind(&f.category)
        .bind(size)
        .bind(dev)
        .bind(ino)
        .bind(f.facts.created_ns)
        .bind(f.facts.modified_ns)
        .bind(seen_at)
        .bind(&f.mime)
        .bind(match f.confidence {
            Confidence::High => "high",
            Confidence::Low => "low",
        })
        .bind(f.facts.kind == EntryKind::Package)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(changed)
}

/// A stamp for a new scan of `folder_id`: the current time, but always
/// strictly greater than any stamp already stored for the folder, so "not
/// seen by this scan" stays correct if two scans start in the same
/// millisecond or the wall clock moves backwards (BUG-013).
pub async fn next_scan_stamp(db: &Db, folder_id: &str, now: i64) -> AppResult<i64> {
    let max: Option<i64> =
        sqlx::query_scalar("SELECT MAX(last_scanned_at) FROM file_records WHERE folder_id = ?1")
            .bind(folder_id)
            .fetch_one(db)
            .await?;
    Ok(max.map_or(now, |m| now.max(m.saturating_add(1))))
}

/// Removes records of files not seen by the scan that started at `before`.
pub async fn delete_not_seen(db: &Db, folder_id: &str, before: i64) -> AppResult<u64> {
    let r = sqlx::query("DELETE FROM file_records WHERE folder_id = ?1 AND last_scanned_at < ?2")
        .bind(folder_id)
        .bind(before)
        .execute(db)
        .await?;
    Ok(r.rows_affected())
}

pub async fn delete_path(db: &Db, path: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM file_records WHERE path = ?1")
        .bind(path)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Per-category totals for one folder, largest first.
pub async fn folder_summary(db: &Db, folder_id: &str) -> AppResult<Vec<(String, i64, i64)>> {
    Ok(sqlx::query_as(
        "SELECT COALESCE(category, 'other') AS category, COUNT(*), COALESCE(SUM(size), 0) AS bytes \
         FROM file_records WHERE folder_id = ?1 GROUP BY 1 ORDER BY bytes DESC, category",
    )
    .bind(folder_id)
    .fetch_all(db)
    .await?)
}

/// Loads tracked files for a folder as pure `FileFacts` snapshots for dry-run / rule testing.
pub async fn list_facts_for_folder(
    db: &Db,
    folder_id: &str,
    folder_path: &str,
    folder_name: &str,
) -> AppResult<Vec<crate::rules::models::FileFacts>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT path, filename, extension, category, size, created_at_ns, modified_at_ns, \
         last_scanned_at, content_hash, mime, \
         (EXISTS ( \
            SELECT 1 FROM duplicate_members dm \
            JOIN duplicate_groups dg ON dg.id = dm.group_id \
            WHERE dm.file_record_id = file_records.id AND dg.ignored = 0 \
         ) OR ( \
            file_records.content_hash IS NOT NULL AND ( \
                SELECT COUNT(*) FROM file_records f2 \
                WHERE f2.content_hash = file_records.content_hash AND f2.size = file_records.size \
            ) > 1 \
         )) AS is_duplicate \
         FROM file_records WHERE folder_id = ?1 ORDER BY filename",
    )
    .bind(folder_id)
    .fetch_all(db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let filename: String = row.try_get("filename")?;
        let ext_opt: Option<String> = row.try_get("extension")?;
        let (stem, split_ext) = crate::rules::models::FileFacts::split_name(&filename);
        let extension = ext_opt.unwrap_or(split_ext);
        let size_i64: i64 = row.try_get("size")?;
        let created_ns: Option<i64> = row.try_get("created_at_ns")?;
        let modified_ns: i64 = row.try_get("modified_at_ns")?;
        let scanned_at: i64 = row.try_get("last_scanned_at")?;
        let category: Option<String> = row.try_get("category")?;
        let is_duplicate: bool = row.try_get("is_duplicate")?;
        out.push(crate::rules::models::FileFacts {
            path: row.try_get("path")?,
            name: filename.clone(),
            stem,
            extension,
            mime: row.try_get("mime")?,
            category: category.unwrap_or_else(|| "other".into()),
            size: u64::try_from(size_i64).unwrap_or(0),
            created_ms: created_ns.map(|ns| ns / 1_000_000),
            modified_ms: modified_ns / 1_000_000,
            added_ms: Some(scanned_at),
            is_hidden: filename.starts_with('.'),
            is_duplicate,
            content_hash: row.try_get("content_hash")?,
            source_folder_id: folder_id.to_owned(),
            source_folder_path: folder_path.to_owned(),
            source_folder_name: folder_name.to_owned(),
        });
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct CachedFileHash {
    pub id: String,
    pub path: String,
    pub dev: i64,
    pub ino: i64,
    pub size: i64,
    pub modified_at_ns: i64,
    pub content_hash: Option<String>,
    pub hash_size: Option<i64>,
    pub hash_mtime_ns: Option<i64>,
}

fn row_to_cached_hash(r: &sqlx::sqlite::SqliteRow) -> CachedFileHash {
    use sqlx::Row;
    CachedFileHash {
        id: r.get("id"),
        path: r.get("path"),
        dev: r.get("dev"),
        ino: r.get("ino"),
        size: r.get("size"),
        modified_at_ns: r.get("modified_at_ns"),
        content_hash: r.get("content_hash"),
        hash_size: r.get("hash_size"),
        hash_mtime_ns: r.get("hash_mtime_ns"),
    }
}

pub async fn get_hash_record_by_path(db: &Db, path: &str) -> AppResult<Option<CachedFileHash>> {
    let row = sqlx::query(
        "SELECT id, path, dev, ino, size, modified_at_ns, content_hash, hash_size, hash_mtime_ns \
         FROM file_records WHERE path = ?1",
    )
    .bind(path)
    .fetch_optional(db)
    .await?;

    Ok(row.as_ref().map(row_to_cached_hash))
}

pub async fn update_content_hash(
    db: &Db,
    path: &str,
    dev: u64,
    ino: u64,
    size: u64,
    modified_ns: i64,
    content_hash: &str,
) -> AppResult<bool> {
    let size_i = to_i64(size);
    let r = sqlx::query(
        "UPDATE file_records \
         SET content_hash = ?2, hash_size = ?3, hash_mtime_ns = ?4, \
             dev = ?5, ino = ?6, size = ?3, modified_at_ns = ?4 \
         WHERE path = ?1",
    )
    .bind(path)
    .bind(content_hash)
    .bind(size_i)
    .bind(modified_ns)
    .bind(to_i64(dev))
    .bind(to_i64(ino))
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}

#[derive(Debug, Clone)]
pub struct TrackedHashCandidate {
    pub id: String,
    pub folder_id: String,
    pub path: String,
    pub filename: String,
    pub size: i64,
    pub dev: i64,
    pub ino: i64,
    pub modified_at_ns: i64,
    pub content_hash: Option<String>,
    pub hash_size: Option<i64>,
    pub hash_mtime_ns: Option<i64>,
}

pub async fn list_tracked_candidates_for_folders(
    db: &Db,
    folder_ids: Option<&[String]>,
) -> AppResult<Vec<TrackedHashCandidate>> {
    use sqlx::Row;
    let rows = match folder_ids {
        Some(ids) if !ids.is_empty() => {
            let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "SELECT id, folder_id, path, filename, size, dev, ino, modified_at_ns, \
                 content_hash, hash_size, hash_mtime_ns FROM file_records WHERE folder_id IN (",
            );
            let mut sep = qb.separated(", ");
            for id in ids {
                sep.push_bind(id);
            }
            sep.push_unseparated(") AND is_package = 0 ORDER BY size DESC, path ASC");
            qb.build().fetch_all(db).await?
        }
        Some(_) => Vec::new(),
        None => {
            sqlx::query(
                "SELECT fr.id, fr.folder_id, fr.path, fr.filename, fr.size, fr.dev, fr.ino, \
                 fr.modified_at_ns, fr.content_hash, fr.hash_size, fr.hash_mtime_ns \
                 FROM file_records fr \
                 JOIN monitored_folders mf ON mf.id = fr.folder_id \
                 WHERE mf.enabled = 1 AND fr.is_package = 0 \
                 ORDER BY fr.size DESC, fr.path ASC",
            )
            .fetch_all(db)
            .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|r| TrackedHashCandidate {
            id: r.get("id"),
            folder_id: r.get("folder_id"),
            path: r.get("path"),
            filename: r.get("filename"),
            size: r.get("size"),
            dev: r.get("dev"),
            ino: r.get("ino"),
            modified_at_ns: r.get("modified_at_ns"),
            content_hash: r.get("content_hash"),
            hash_size: r.get("hash_size"),
            hash_mtime_ns: r.get("hash_mtime_ns"),
        })
        .collect())
}

/// Lists non-package tracked file records in enabled monitored folders whose size matches `sizes`.
pub async fn list_tracked_by_sizes(db: &Db, sizes: &[i64]) -> AppResult<Vec<CachedFileHash>> {
    if sizes.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for chunk in sizes.chunks(500) {
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT fr.id, fr.path, fr.dev, fr.ino, fr.size, fr.modified_at_ns, \
             fr.content_hash, fr.hash_size, fr.hash_mtime_ns \
             FROM file_records fr \
             JOIN monitored_folders mf ON mf.id = fr.folder_id \
             WHERE mf.enabled = 1 AND fr.is_package = 0 AND fr.size IN (",
        );
        let mut sep = qb.separated(", ");
        for size in chunk {
            sep.push_bind(*size);
        }
        sep.push_unseparated(")");
        let rows = qb.build().fetch_all(db).await?;
        out.extend(rows.iter().map(row_to_cached_hash));
    }
    Ok(out)
}
