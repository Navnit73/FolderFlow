use std::collections::HashMap;

use super::Db;
use crate::error::AppResult;
use crate::utils::time::now_ms;

pub async fn load_all(db: &Db) -> AppResult<HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Upserts all sections in one transaction (all-or-nothing).
pub async fn save_all(db: &Db, sections: &[(&str, String)]) -> AppResult<()> {
    let now = now_ms();
    let mut tx = db.begin().await?;
    for (key, value) in sections {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
