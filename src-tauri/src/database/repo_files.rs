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
                 AND file_records.ino = excluded.ino THEN file_records.content_hash ELSE NULL END, \
               hash_size = CASE WHEN file_records.size = excluded.size AND file_records.modified_at_ns = excluded.modified_at_ns \
                 AND file_records.ino = excluded.ino THEN file_records.hash_size ELSE NULL END, \
               hash_mtime_ns = CASE WHEN file_records.size = excluded.size AND file_records.modified_at_ns = excluded.modified_at_ns \
                 AND file_records.ino = excluded.ino THEN file_records.hash_mtime_ns ELSE NULL END, \
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
