use sqlx::Row;

use super::Db;
use crate::error::AppResult;
use crate::folders::{FolderHealth, FolderSettings, MonitoredFolder};
use crate::utils::time::now_ms;

macro_rules! select_folders {
    ($where:literal) => {
        concat!(
            "SELECT f.id, f.name, f.path, f.enabled, f.recursive, f.settings_json, f.health, f.last_error, ",
            "f.last_scanned_at, f.created_at, ",
            "(SELECT COUNT(*) FROM file_records r WHERE r.folder_id = f.id) AS file_count, ",
            "(SELECT COALESCE(SUM(size), 0) FROM file_records r WHERE r.folder_id = f.id) AS total_bytes ",
            "FROM monitored_folders f ",
            $where
        )
    };
}

fn from_row(row: &sqlx::sqlite::SqliteRow) -> AppResult<MonitoredFolder> {
    let settings_json: String = row.try_get("settings_json")?;
    let health: String = row.try_get("health")?;
    let file_count: i64 = row.try_get("file_count")?;
    Ok(MonitoredFolder {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        path: row.try_get("path")?,
        enabled: row.try_get("enabled")?,
        recursive: row.try_get("recursive")?,
        // Unknown / older settings shapes fall back to defaults instead of failing.
        settings: serde_json::from_str(&settings_json).unwrap_or_default(),
        health: FolderHealth::from_db(&health),
        last_error: row.try_get("last_error")?,
        file_count: u32::try_from(file_count).unwrap_or(u32::MAX),
        total_bytes: row.try_get("total_bytes")?,
        last_scanned_at: row.try_get("last_scanned_at")?,
        created_at: row.try_get("created_at")?,
    })
}

pub async fn list(db: &Db) -> AppResult<Vec<MonitoredFolder>> {
    let rows = sqlx::query(select_folders!("ORDER BY f.created_at, f.name"))
        .fetch_all(db)
        .await?;
    rows.iter().map(from_row).collect()
}

pub async fn get(db: &Db, id: &str) -> AppResult<Option<MonitoredFolder>> {
    let row = sqlx::query(select_folders!("WHERE f.id = ?1"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    row.as_ref().map(from_row).transpose()
}

pub async fn insert(db: &Db, id: &str, name: &str, path: &str, recursive: bool) -> AppResult<()> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO monitored_folders (id, name, path, enabled, recursive, settings_json, health, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 1, ?4, ?5, 'unknown', ?6, ?6)",
    )
    .bind(id)
    .bind(name)
    .bind(path)
    .bind(recursive)
    .bind(serde_json::to_string(&FolderSettings::default())?)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update(db: &Db, f: &MonitoredFolder) -> AppResult<()> {
    sqlx::query(
        "UPDATE monitored_folders SET name = ?2, enabled = ?3, recursive = ?4, settings_json = ?5, updated_at = ?6 \
         WHERE id = ?1",
    )
    .bind(&f.id)
    .bind(&f.name)
    .bind(f.enabled)
    .bind(f.recursive)
    .bind(serde_json::to_string(&f.settings)?)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete(db: &Db, id: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM monitored_folders WHERE id = ?1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_health(
    db: &Db,
    id: &str,
    health: FolderHealth,
    error: Option<&str>,
) -> AppResult<bool> {
    let r = sqlx::query(
        "UPDATE monitored_folders SET health = ?2, last_error = ?3, updated_at = ?4 WHERE id = ?1",
    )
    .bind(id)
    .bind(health.as_db())
    .bind(error)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn record_scan(db: &Db, id: &str, at_ms: i64) -> AppResult<()> {
    sqlx::query("UPDATE monitored_folders SET last_scanned_at = ?2, file_count = (SELECT COUNT(*) FROM file_records WHERE folder_id = ?1) WHERE id = ?1")
        .bind(id)
        .bind(at_ms)
        .execute(db)
        .await?;
    Ok(())
}

/// Forgets the scan baseline so the next scan records what exists without
/// reporting it as new (used when a folder's scope changes).
pub async fn reset_scan(db: &Db, id: &str) -> AppResult<()> {
    sqlx::query("UPDATE monitored_folders SET last_scanned_at = NULL WHERE id = ?1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Keeps the newest 500 watcher errors per folder.
pub async fn log_watcher_error(
    db: &Db,
    folder_id: &str,
    code: &str,
    message: &str,
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query("INSERT INTO watcher_errors (folder_id, code, message, occurred_at) VALUES (?1, ?2, ?3, ?4)")
        .bind(folder_id)
        .bind(code)
        .bind(message)
        .bind(now_ms())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM watcher_errors WHERE folder_id = ?1 AND id NOT IN \
         (SELECT id FROM watcher_errors WHERE folder_id = ?1 ORDER BY occurred_at DESC, id DESC LIMIT 500)",
    )
    .bind(folder_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Most recent watcher errors for a folder: (code, message, occurred_at ms).
pub async fn recent_errors(
    db: &Db,
    folder_id: &str,
    limit: i64,
) -> AppResult<Vec<(String, String, i64)>> {
    Ok(sqlx::query_as(
        "SELECT code, message, occurred_at FROM watcher_errors WHERE folder_id = ?1 \
         ORDER BY occurred_at DESC, id DESC LIMIT ?2",
    )
    .bind(folder_id)
    .bind(limit)
    .fetch_all(db)
    .await?)
}
