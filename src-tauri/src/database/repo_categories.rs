use sqlx::Row;

use super::Db;
use crate::categories::Category;
use crate::error::AppResult;
use crate::utils::time::now_ms;

/// Column list as a macro so queries stay compile-time constants (sqlx rejects dynamic SQL).
macro_rules! columns {
    () => {
        "id, name, description, icon, color_token, extensions_json, destination_path, priority, builtin, enabled"
    };
}

fn from_row(row: &sqlx::sqlite::SqliteRow) -> AppResult<Category> {
    let extensions_json: String = row.try_get("extensions_json")?;
    Ok(Category {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        icon: row.try_get("icon")?,
        color_token: row.try_get("color_token")?,
        extensions: serde_json::from_str(&extensions_json)?,
        destination_path: row.try_get("destination_path")?,
        priority: row.try_get("priority")?,
        builtin: row.try_get("builtin")?,
        enabled: row.try_get("enabled")?,
    })
}

pub async fn list(db: &Db) -> AppResult<Vec<Category>> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM categories ORDER BY priority, builtin, name"
    ))
    .fetch_all(db)
    .await?;
    rows.iter().map(from_row).collect()
}

pub async fn get(db: &Db, id: &str) -> AppResult<Option<Category>> {
    let row = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM categories WHERE id = ?1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    row.as_ref().map(from_row).transpose()
}

pub async fn insert(db: &Db, c: &Category) -> AppResult<()> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO categories (id, name, description, icon, color_token, extensions_json, destination_path, \
         priority, builtin, enabled, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
    )
    .bind(&c.id)
    .bind(&c.name)
    .bind(&c.description)
    .bind(&c.icon)
    .bind(&c.color_token)
    .bind(serde_json::to_string(&c.extensions)?)
    .bind(&c.destination_path)
    .bind(c.priority)
    .bind(c.builtin)
    .bind(c.enabled)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Built-ins: refresh definition fields, keep the user's enabled/priority/destination.
pub async fn upsert_builtin(db: &Db, c: &Category) -> AppResult<()> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO categories (id, name, description, icon, color_token, extensions_json, destination_path, \
         priority, builtin, enabled, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, 1, 1, ?8, ?8) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, icon = excluded.icon, \
         color_token = excluded.color_token, extensions_json = excluded.extensions_json, builtin = 1, \
         updated_at = excluded.updated_at",
    )
    .bind(&c.id)
    .bind(&c.name)
    .bind(&c.description)
    .bind(&c.icon)
    .bind(&c.color_token)
    .bind(serde_json::to_string(&c.extensions)?)
    .bind(c.priority)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update(db: &Db, c: &Category) -> AppResult<()> {
    sqlx::query(
        "UPDATE categories SET name = ?2, description = ?3, icon = ?4, color_token = ?5, extensions_json = ?6, \
         destination_path = ?7, priority = ?8, enabled = ?9, updated_at = ?10 WHERE id = ?1",
    )
    .bind(&c.id)
    .bind(&c.name)
    .bind(&c.description)
    .bind(&c.icon)
    .bind(&c.color_token)
    .bind(serde_json::to_string(&c.extensions)?)
    .bind(&c.destination_path)
    .bind(c.priority)
    .bind(c.enabled)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_enabled(db: &Db, id: &str, enabled: bool) -> AppResult<bool> {
    let r = sqlx::query("UPDATE categories SET enabled = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(enabled)
        .bind(now_ms())
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete_custom(db: &Db, id: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM categories WHERE id = ?1 AND builtin = 0")
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Assigns priorities `10, 20, 30, ...` in the provided ID order inside a single transaction.
pub async fn reorder(db: &Db, ordered_ids: &[String]) -> AppResult<()> {
    let now = now_ms();
    let mut tx = db.begin().await?;
    for (idx, id) in ordered_ids.iter().enumerate() {
        let priority = i32::try_from((idx + 1) * 10).unwrap_or(1000);
        sqlx::query("UPDATE categories SET priority = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id)
            .bind(priority)
            .bind(now)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
