//! SQLite repository for `rules` and `destination_roots` (P3-T07; DATABASE.md).

use sqlx::Row;

use super::Db;
use crate::error::AppResult;
use crate::rules::models::{
    decode_actions_json, decode_conditions_json, encode_actions_json, encode_conditions_json,
    ExecutionMode, Rule, RuleOrigin,
};
use crate::utils::time::now_ms;

macro_rules! select_rules {
    ($suffix:literal) => {
        concat!(
            "SELECT id, name, description, enabled, priority, source_folders_json, ",
            "conditions_json, actions_json, execution_mode, origin, last_run_at, ",
            "stat_matched, stat_succeeded, stat_failed, created_at, updated_at ",
            "FROM rules ",
            $suffix
        )
    };
}

fn from_row(row: &sqlx::sqlite::SqliteRow) -> AppResult<Rule> {
    let source_folders_json: String = row.try_get("source_folders_json")?;
    let conditions_json: String = row.try_get("conditions_json")?;
    let actions_json: String = row.try_get("actions_json")?;
    let exec_str: String = row.try_get("execution_mode")?;
    let origin_str: String = row.try_get("origin")?;

    let source_folders: Vec<String> = serde_json::from_str(&source_folders_json)?;
    let conditions = decode_conditions_json(&conditions_json)?;
    let (actions, date_source) = decode_actions_json(&actions_json)?;

    Ok(Rule {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        enabled: row.try_get("enabled")?,
        priority: row.try_get("priority")?,
        source_folders,
        conditions,
        actions,
        date_source,
        execution_mode: ExecutionMode::from_db(&exec_str)?,
        origin: RuleOrigin::from_db(&origin_str)?,
        last_run_at: row.try_get("last_run_at")?,
        stat_matched: row.try_get("stat_matched")?,
        stat_succeeded: row.try_get("stat_succeeded")?,
        stat_failed: row.try_get("stat_failed")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn list(db: &Db) -> AppResult<Vec<Rule>> {
    let rows = sqlx::query(select_rules!(
        "ORDER BY priority ASC, created_at ASC, id ASC"
    ))
    .fetch_all(db)
    .await?;
    rows.iter().map(from_row).collect()
}

pub async fn get(db: &Db, id: &str) -> AppResult<Option<Rule>> {
    let row = sqlx::query(select_rules!("WHERE id = ?1"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    row.as_ref().map(from_row).transpose()
}

pub async fn next_priority(db: &Db) -> AppResult<i32> {
    let max: Option<i32> = sqlx::query_scalar("SELECT MAX(priority) FROM rules")
        .fetch_one(db)
        .await?;
    Ok(max.unwrap_or(0).saturating_add(1))
}

pub async fn insert(db: &Db, rule: &Rule) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO rules (id, name, description, enabled, priority, source_folders_json, \
         conditions_json, actions_json, execution_mode, origin, last_run_at, \
         stat_matched, stat_succeeded, stat_failed, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )
    .bind(&rule.id)
    .bind(&rule.name)
    .bind(&rule.description)
    .bind(rule.enabled)
    .bind(rule.priority)
    .bind(serde_json::to_string(&rule.source_folders)?)
    .bind(encode_conditions_json(&rule.conditions)?)
    .bind(encode_actions_json(&rule.actions, rule.date_source)?)
    .bind(rule.execution_mode.as_db())
    .bind(rule.origin.as_db())
    .bind(rule.last_run_at)
    .bind(rule.stat_matched)
    .bind(rule.stat_succeeded)
    .bind(rule.stat_failed)
    .bind(rule.created_at)
    .bind(rule.updated_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update(db: &Db, rule: &Rule) -> AppResult<bool> {
    let r = sqlx::query(
        "UPDATE rules SET name = ?2, description = ?3, enabled = ?4, priority = ?5, \
         source_folders_json = ?6, conditions_json = ?7, actions_json = ?8, \
         execution_mode = ?9, updated_at = ?10 WHERE id = ?1",
    )
    .bind(&rule.id)
    .bind(&rule.name)
    .bind(&rule.description)
    .bind(rule.enabled)
    .bind(rule.priority)
    .bind(serde_json::to_string(&rule.source_folders)?)
    .bind(encode_conditions_json(&rule.conditions)?)
    .bind(encode_actions_json(&rule.actions, rule.date_source)?)
    .bind(rule.execution_mode.as_db())
    .bind(rule.updated_at)
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete(db: &Db, id: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM rules WHERE id = ?1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_enabled(db: &Db, id: &str, enabled: bool) -> AppResult<bool> {
    let r = sqlx::query("UPDATE rules SET enabled = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(enabled)
        .bind(now_ms())
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn reorder(db: &Db, ordered_ids: &[String]) -> AppResult<()> {
    let existing: Vec<String> =
        sqlx::query_scalar("SELECT id FROM rules ORDER BY priority ASC, created_at ASC, id ASC")
            .fetch_all(db)
            .await?;

    let mut final_order = Vec::with_capacity(existing.len());
    for id in ordered_ids {
        if existing.contains(id) && !final_order.contains(id) {
            final_order.push(id.clone());
        }
    }
    for id in existing {
        if !final_order.contains(&id) {
            final_order.push(id);
        }
    }

    let now = now_ms();
    let mut tx = db.begin().await?;
    for (idx, id) in final_order.iter().enumerate() {
        let prio = i32::try_from(idx + 1).unwrap_or(i32::MAX);
        sqlx::query("UPDATE rules SET priority = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id)
            .bind(prio)
            .bind(now)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn record_stats(
    db: &Db,
    id: &str,
    matched_delta: i64,
    succeeded_delta: i64,
    failed_delta: i64,
    ran_at_ms: i64,
) -> AppResult<bool> {
    let r = sqlx::query(
        "UPDATE rules SET \
         stat_matched = stat_matched + ?2, \
         stat_succeeded = stat_succeeded + ?3, \
         stat_failed = stat_failed + ?4, \
         last_run_at = ?5, \
         updated_at = ?5 \
         WHERE id = ?1",
    )
    .bind(id)
    .bind(matched_delta)
    .bind(succeeded_delta)
    .bind(failed_delta)
    .bind(ran_at_ms)
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn list_destination_roots(db: &Db) -> AppResult<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT path FROM destination_roots ORDER BY created_at ASC")
            .fetch_all(db)
            .await?,
    )
}

pub async fn upsert_destination_root(db: &Db, id: &str, path: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO destination_roots (id, path, created_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(path) DO NOTHING",
    )
    .bind(id)
    .bind(path)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(())
}
