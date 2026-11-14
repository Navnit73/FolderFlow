//! SQLite repository for `operation_batches`, `operations`, and `review_queue` (`P4-T04`, `P4-T10`).

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, SqliteConnection};

use crate::database::Db;
use crate::engine::operations::{BatchTrigger, OpKind, OpStatus};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationBatchRow {
    pub id: String,
    pub trigger: BatchTrigger,
    pub plan_id: Option<String>,
    pub total: u32,
    #[specta(type = f64)]
    pub started_at: i64,
    #[specta(type = Option<f64>)]
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOperationRow {
    pub id: String,
    pub batch_id: Option<String>,
    pub file_record_id: Option<String>,
    pub operation_type: OpKind,
    pub source_path: String,
    pub destination_path: String,
    pub staging_path: Option<String>,
    pub original_name: String,
    pub new_name: String,
    pub dev: Option<i64>,
    pub ino: Option<i64>,
    pub size: Option<i64>,
    pub mtime_ns: Option<i64>,
    pub content_hash: Option<String>,
    pub cross_volume: bool,
    pub rule_id: Option<String>,
    pub reversible: bool,
    pub started_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct OperationTransitionPatch {
    pub destination_path: Option<String>,
    pub new_name: Option<String>,
    pub staging_path: Option<Option<String>>,
    pub dev: Option<i64>,
    pub ino: Option<i64>,
    pub size: Option<i64>,
    pub mtime_ns: Option<i64>,
    pub content_hash: Option<String>,
    pub cross_volume: Option<bool>,
    pub error_code: Option<Option<String>>,
    pub error_message: Option<Option<String>>,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRow {
    pub id: String,
    pub batch_id: Option<String>,
    pub file_record_id: Option<String>,
    pub operation_type: OpKind,
    pub source_path: String,
    pub destination_path: String,
    pub staging_path: Option<String>,
    pub original_name: String,
    pub new_name: String,
    pub dev: Option<i64>,
    pub ino: Option<i64>,
    pub size: Option<i64>,
    pub mtime_ns: Option<i64>,
    pub content_hash: Option<String>,
    pub cross_volume: bool,
    pub rule_id: Option<String>,
    pub rule_name: Option<String>,
    pub status: OpStatus,
    pub reversible: bool,
    pub reversed_by: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFilter {
    pub search: Option<String>,
    pub status: Option<OpStatus>,
    pub operation_type: Option<OpKind>,
    pub rule_id: Option<String>,
    pub folder_path_prefix: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageRequest {
    pub limit: u32,
    pub offset: u32,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPage {
    pub items: Vec<OperationView>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationView {
    pub id: String,
    pub batch_id: Option<String>,
    pub operation_type: OpKind,
    pub source_path: String,
    pub destination_path: String,
    pub original_name: String,
    pub new_name: String,
    #[specta(type = Option<f64>)]
    pub size: Option<i64>,
    pub content_hash: Option<String>,
    pub cross_volume: bool,
    pub rule_id: Option<String>,
    pub rule_name: Option<String>,
    pub status: OpStatus,
    pub reversible: bool,
    pub reversed_by: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    #[specta(type = f64)]
    pub started_at: i64,
    #[specta(type = Option<f64>)]
    pub completed_at: Option<i64>,
}

impl From<OperationRow> for OperationView {
    fn from(r: OperationRow) -> Self {
        Self {
            id: r.id,
            batch_id: r.batch_id,
            operation_type: r.operation_type,
            source_path: r.source_path,
            destination_path: r.destination_path,
            original_name: r.original_name,
            new_name: r.new_name,
            size: r.size,
            content_hash: r.content_hash,
            cross_volume: r.cross_volume,
            rule_id: r.rule_id,
            rule_name: r.rule_name,
            status: r.status,
            reversible: r.reversible,
            reversed_by: r.reversed_by,
            error_code: r.error_code,
            error_message: r.error_message,
            started_at: r.started_at,
            completed_at: r.completed_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewProposedAction {
    pub operation_type: OpKind,
    pub source_path: String,
    pub destination_path: String,
    pub original_name: String,
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItemView {
    pub id: String,
    pub path: String,
    pub reason: String,
    pub rule_id: Option<String>,
    pub proposed: Option<ReviewProposedAction>,
    #[specta(type = f64)]
    pub created_at: i64,
    #[specta(type = Option<f64>)]
    pub resolved_at: Option<i64>,
}

async fn ensure_synchronous_full(conn: &mut SqliteConnection) -> AppResult<()> {
    sqlx::query("PRAGMA synchronous = FULL;")
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn query_synchronous_pragma(db: &Db) -> AppResult<i64> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    let val: i64 = sqlx::query_scalar("PRAGMA synchronous;")
        .fetch_one(&mut *conn)
        .await?;
    Ok(val)
}

pub async fn create_batch(
    db: &Db,
    id: &str,
    trigger: BatchTrigger,
    plan_id: Option<&str>,
    total: u32,
    started_at: i64,
) -> AppResult<OperationBatchRow> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    sqlx::query(
        "INSERT INTO operation_batches (id, trigger, plan_id, total, started_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(trigger.as_str())
    .bind(plan_id)
    .bind(i64::from(total))
    .bind(started_at)
    .execute(&mut *conn)
    .await?;

    Ok(OperationBatchRow {
        id: id.to_owned(),
        trigger,
        plan_id: plan_id.map(str::to_owned),
        total,
        started_at,
        completed_at: None,
    })
}

pub async fn complete_batch(db: &Db, id: &str, completed_at: i64) -> AppResult<()> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    sqlx::query("UPDATE operation_batches SET completed_at = ? WHERE id = ?")
        .bind(completed_at)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn get_batch(db: &Db, id: &str) -> AppResult<Option<OperationBatchRow>> {
    let row = sqlx::query(
        "SELECT id, trigger, plan_id, total, started_at, completed_at \
         FROM operation_batches WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;

    let Some(r) = row else {
        return Ok(None);
    };
    let trigger_str: String = r.try_get("trigger")?;
    let total_i64: i64 = r.try_get("total")?;
    Ok(Some(OperationBatchRow {
        id: r.try_get("id")?,
        trigger: BatchTrigger::from_str(&trigger_str)?,
        plan_id: r.try_get("plan_id")?,
        total: u32::try_from(total_i64).unwrap_or(0),
        started_at: r.try_get("started_at")?,
        completed_at: r.try_get("completed_at")?,
    }))
}

pub async fn insert_operation_intent(db: &Db, op: &NewOperationRow) -> AppResult<OperationRow> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;

    sqlx::query(
        "INSERT INTO operations (\
            id, batch_id, file_record_id, operation_type, source_path, destination_path, \
            staging_path, original_name, new_name, dev, ino, size, mtime_ns, content_hash, \
            cross_volume, rule_id, status, reversible, started_at\
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'PENDING', ?, ?)",
    )
    .bind(&op.id)
    .bind(&op.batch_id)
    .bind(&op.file_record_id)
    .bind(op.operation_type.as_str())
    .bind(&op.source_path)
    .bind(&op.destination_path)
    .bind(&op.staging_path)
    .bind(&op.original_name)
    .bind(&op.new_name)
    .bind(op.dev)
    .bind(op.ino)
    .bind(op.size)
    .bind(op.mtime_ns)
    .bind(&op.content_hash)
    .bind(i64::from(op.cross_volume))
    .bind(&op.rule_id)
    .bind(i64::from(op.reversible))
    .bind(op.started_at)
    .execute(&mut *conn)
    .await?;

    fetch_operation_on(&mut conn, &op.id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Operation {}", op.id)))
}

pub async fn transition_operation(
    db: &Db,
    id: &str,
    expected_from: OpStatus,
    to: OpStatus,
    patch: &OperationTransitionPatch,
) -> AppResult<OperationRow> {
    expected_from.validate_transition(to)?;

    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;

    let current = fetch_operation_on(&mut conn, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Operation {id}")))?;
    if current.status != expected_from {
        return Err(AppError::validation(
            "status",
            format!(
                "expected operation {id} in status {expected_from}, found {}",
                current.status
            ),
        ));
    }

    let dst_path = patch
        .destination_path
        .as_deref()
        .unwrap_or(&current.destination_path);
    let new_name = patch.new_name.as_deref().unwrap_or(&current.new_name);
    let staging_path: Option<&str> = match &patch.staging_path {
        Some(opt) => opt.as_deref(),
        None => current.staging_path.as_deref(),
    };
    let dev = patch.dev.or(current.dev);
    let ino = patch.ino.or(current.ino);
    let size = patch.size.or(current.size);
    let mtime_ns = patch.mtime_ns.or(current.mtime_ns);
    let content_hash = patch
        .content_hash
        .as_deref()
        .or(current.content_hash.as_deref());
    let cross_volume = patch.cross_volume.unwrap_or(current.cross_volume);
    let error_code: Option<&str> = match &patch.error_code {
        Some(opt) => opt.as_deref(),
        None => current.error_code.as_deref(),
    };
    let error_message: Option<&str> = match &patch.error_message {
        Some(opt) => opt.as_deref(),
        None => current.error_message.as_deref(),
    };
    let completed_at = patch.completed_at.or(current.completed_at);

    let res = sqlx::query(
        "UPDATE operations SET \
            status = ?, destination_path = ?, new_name = ?, staging_path = ?, \
            dev = ?, ino = ?, size = ?, mtime_ns = ?, content_hash = ?, \
            cross_volume = ?, error_code = ?, error_message = ?, completed_at = ? \
         WHERE id = ? AND status = ?",
    )
    .bind(to.as_str())
    .bind(dst_path)
    .bind(new_name)
    .bind(staging_path)
    .bind(dev)
    .bind(ino)
    .bind(size)
    .bind(mtime_ns)
    .bind(content_hash)
    .bind(i64::from(cross_volume))
    .bind(error_code)
    .bind(error_message)
    .bind(completed_at)
    .bind(id)
    .bind(expected_from.as_str())
    .execute(&mut *conn)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::validation(
            "status",
            format!("concurrent state change on operation {id}"),
        ));
    }

    fetch_operation_on(&mut conn, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Operation {id}")))
}

/// Records the final auto-renamed `destination_path` and clears `staging_path` while in
/// `VERIFYING` state right after `rename_excl(staging, final_path)` succeeds (`BUG-018`).
pub async fn update_verifying_destination(
    db: &Db,
    id: &str,
    destination_path: &str,
    new_name: &str,
) -> AppResult<()> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    sqlx::query(
        "UPDATE operations SET destination_path = ?, new_name = ?, staging_path = NULL \
         WHERE id = ? AND status = 'VERIFYING'",
    )
    .bind(destination_path)
    .bind(new_name)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn mark_reversed_by(db: &Db, id: &str, undo_op_id: &str) -> AppResult<()> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    let res =
        sqlx::query("UPDATE operations SET reversed_by = ? WHERE id = ? AND reversed_by IS NULL")
            .bind(undo_op_id)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::validation(
            "reversedBy",
            format!("operation {id} is already undone or does not exist"),
        ));
    }
    Ok(())
}

pub async fn get_operation(db: &Db, id: &str) -> AppResult<Option<OperationRow>> {
    let mut conn = db.acquire().await?;
    fetch_operation_on(&mut conn, id).await
}

pub async fn list_by_batch(db: &Db, batch_id: &str) -> AppResult<Vec<OperationRow>> {
    let rows = sqlx::query(
        "SELECT o.id, o.batch_id, o.file_record_id, o.operation_type, o.source_path, \
                o.destination_path, o.staging_path, o.original_name, o.new_name, \
                o.dev, o.ino, o.size, o.mtime_ns, o.content_hash, o.cross_volume, \
                o.rule_id, r.name AS rule_name, o.status, o.reversible, o.reversed_by, \
                o.error_code, o.error_message, o.started_at, o.completed_at \
         FROM operations o \
         LEFT JOIN rules r ON r.id = o.rule_id \
         WHERE o.batch_id = ? ORDER BY o.started_at ASC, o.rowid ASC",
    )
    .bind(batch_id)
    .fetch_all(db)
    .await?;
    rows.iter().map(map_operation_row).collect()
}

pub async fn list_non_terminal(db: &Db) -> AppResult<Vec<OperationRow>> {
    let rows = sqlx::query(
        "SELECT o.id, o.batch_id, o.file_record_id, o.operation_type, o.source_path, \
                o.destination_path, o.staging_path, o.original_name, o.new_name, \
                o.dev, o.ino, o.size, o.mtime_ns, o.content_hash, o.cross_volume, \
                o.rule_id, r.name AS rule_name, o.status, o.reversible, o.reversed_by, \
                o.error_code, o.error_message, o.started_at, o.completed_at \
         FROM operations o \
         LEFT JOIN rules r ON r.id = o.rule_id \
         WHERE o.status NOT IN ('COMPLETED', 'FAILED', 'ROLLED_BACK') \
         ORDER BY o.started_at ASC, o.rowid ASC",
    )
    .fetch_all(db)
    .await?;
    rows.iter().map(map_operation_row).collect()
}

pub async fn get_last_undoable_operation(db: &Db) -> AppResult<Option<OperationRow>> {
    let row = sqlx::query(
        "SELECT o.id, o.batch_id, o.file_record_id, o.operation_type, o.source_path, \
                o.destination_path, o.staging_path, o.original_name, o.new_name, \
                o.dev, o.ino, o.size, o.mtime_ns, o.content_hash, o.cross_volume, \
                o.rule_id, r.name AS rule_name, o.status, o.reversible, o.reversed_by, \
                o.error_code, o.error_message, o.started_at, o.completed_at \
         FROM operations o \
         LEFT JOIN rules r ON r.id = o.rule_id \
         WHERE o.status = 'COMPLETED' \
           AND o.reversible = 1 \
           AND o.reversed_by IS NULL \
           AND o.operation_type IN ('move', 'copy', 'rename') \
         ORDER BY o.started_at DESC, o.rowid DESC LIMIT 1",
    )
    .fetch_optional(db)
    .await?;
    row.as_ref().map(map_operation_row).transpose()
}

pub async fn list_page(
    db: &Db,
    filter: &ActivityFilter,
    page: PageRequest,
) -> AppResult<ActivityPage> {
    let limit = page.limit.clamp(1, 500);
    let offset = page.offset;

    let status_bind = filter.status.map(|s| s.as_str().to_owned());
    let op_type_bind = filter.operation_type.map(|t| t.as_str().to_owned());
    let rule_bind = filter
        .rule_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let prefix_bind = filter
        .folder_path_prefix
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}%"));
    let search_bind = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{s}%"));

    let total_i64: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations o \
         WHERE (?1 IS NULL OR o.status = ?1) \
           AND (?2 IS NULL OR o.operation_type = ?2) \
           AND (?3 IS NULL OR o.rule_id = ?3) \
           AND (?4 IS NULL OR o.source_path LIKE ?4 OR o.destination_path LIKE ?4) \
           AND (?5 IS NULL OR o.original_name LIKE ?5 OR o.new_name LIKE ?5 \
                OR o.source_path LIKE ?5 OR o.destination_path LIKE ?5)",
    )
    .bind(&status_bind)
    .bind(&op_type_bind)
    .bind(&rule_bind)
    .bind(&prefix_bind)
    .bind(&search_bind)
    .fetch_one(db)
    .await?;
    let total = u32::try_from(total_i64).unwrap_or(u32::MAX);

    let rows = sqlx::query(
        "SELECT o.id, o.batch_id, o.file_record_id, o.operation_type, o.source_path, \
                o.destination_path, o.staging_path, o.original_name, o.new_name, \
                o.dev, o.ino, o.size, o.mtime_ns, o.content_hash, o.cross_volume, \
                o.rule_id, r.name AS rule_name, o.status, o.reversible, o.reversed_by, \
                o.error_code, o.error_message, o.started_at, o.completed_at \
         FROM operations o \
         LEFT JOIN rules r ON r.id = o.rule_id \
         WHERE (?1 IS NULL OR o.status = ?1) \
           AND (?2 IS NULL OR o.operation_type = ?2) \
           AND (?3 IS NULL OR o.rule_id = ?3) \
           AND (?4 IS NULL OR o.source_path LIKE ?4 OR o.destination_path LIKE ?4) \
           AND (?5 IS NULL OR o.original_name LIKE ?5 OR o.new_name LIKE ?5 \
                OR o.source_path LIKE ?5 OR o.destination_path LIKE ?5) \
         ORDER BY o.started_at DESC, o.rowid DESC LIMIT ?6 OFFSET ?7",
    )
    .bind(&status_bind)
    .bind(&op_type_bind)
    .bind(&rule_bind)
    .bind(&prefix_bind)
    .bind(&search_bind)
    .bind(i64::from(limit))
    .bind(i64::from(offset))
    .fetch_all(db)
    .await?;

    let items = rows
        .iter()
        .map(|r| map_operation_row(r).map(OperationView::from))
        .collect::<AppResult<Vec<_>>>()?;

    Ok(ActivityPage {
        items,
        total,
        limit,
        offset,
    })
}

pub async fn clear_history(db: &Db, before_ms: Option<i64>) -> AppResult<u32> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    let res = if let Some(cutoff) = before_ms {
        sqlx::query(
            "DELETE FROM operations \
             WHERE status IN ('COMPLETED', 'FAILED', 'ROLLED_BACK') AND started_at <= ?",
        )
        .bind(cutoff)
        .execute(&mut *conn)
        .await?
    } else {
        sqlx::query(
            "DELETE FROM operations \
             WHERE status IN ('COMPLETED', 'FAILED', 'ROLLED_BACK')",
        )
        .execute(&mut *conn)
        .await?
    };
    Ok(u32::try_from(res.rows_affected()).unwrap_or(u32::MAX))
}

pub async fn enqueue_review(
    db: &Db,
    id: &str,
    path: &str,
    reason: &str,
    rule_id: Option<&str>,
    proposed: Option<&ReviewProposedAction>,
    created_at: i64,
) -> AppResult<ReviewItemView> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    let proposed_json = proposed.map(serde_json::to_string).transpose()?;
    sqlx::query(
        "INSERT INTO review_queue (id, path, reason, rule_id, proposed_json, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(path)
    .bind(reason)
    .bind(rule_id)
    .bind(&proposed_json)
    .bind(created_at)
    .execute(&mut *conn)
    .await?;

    Ok(ReviewItemView {
        id: id.to_owned(),
        path: path.to_owned(),
        reason: reason.to_owned(),
        rule_id: rule_id.map(str::to_owned),
        proposed: proposed.cloned(),
        created_at,
        resolved_at: None,
    })
}

pub async fn list_pending_reviews(db: &Db) -> AppResult<Vec<ReviewItemView>> {
    let rows = sqlx::query(
        "SELECT id, path, reason, rule_id, proposed_json, created_at, resolved_at \
         FROM review_queue WHERE resolved_at IS NULL ORDER BY created_at DESC",
    )
    .fetch_all(db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let proposed_json: Option<String> = r.try_get("proposed_json")?;
        let proposed = proposed_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        out.push(ReviewItemView {
            id: r.try_get("id")?,
            path: r.try_get("path")?,
            reason: r.try_get("reason")?,
            rule_id: r.try_get("rule_id")?,
            proposed,
            created_at: r.try_get("created_at")?,
            resolved_at: r.try_get("resolved_at")?,
        });
    }
    Ok(out)
}

pub async fn get_review_item(db: &Db, id: &str) -> AppResult<Option<ReviewItemView>> {
    let row = sqlx::query(
        "SELECT id, path, reason, rule_id, proposed_json, created_at, resolved_at \
         FROM review_queue WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;

    let Some(r) = row else {
        return Ok(None);
    };
    let proposed_json: Option<String> = r.try_get("proposed_json")?;
    let proposed = proposed_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(Some(ReviewItemView {
        id: r.try_get("id")?,
        path: r.try_get("path")?,
        reason: r.try_get("reason")?,
        rule_id: r.try_get("rule_id")?,
        proposed,
        created_at: r.try_get("created_at")?,
        resolved_at: r.try_get("resolved_at")?,
    }))
}

pub async fn mark_review_resolved(db: &Db, id: &str, resolved_at: i64) -> AppResult<()> {
    let mut conn = db.acquire().await?;
    ensure_synchronous_full(&mut conn).await?;
    let res =
        sqlx::query("UPDATE review_queue SET resolved_at = ? WHERE id = ? AND resolved_at IS NULL")
            .bind(resolved_at)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Review item {id}")));
    }
    Ok(())
}

async fn fetch_operation_on(
    conn: &mut SqliteConnection,
    id: &str,
) -> AppResult<Option<OperationRow>> {
    let row = sqlx::query(
        "SELECT o.id, o.batch_id, o.file_record_id, o.operation_type, o.source_path, \
                o.destination_path, o.staging_path, o.original_name, o.new_name, \
                o.dev, o.ino, o.size, o.mtime_ns, o.content_hash, o.cross_volume, \
                o.rule_id, r.name AS rule_name, o.status, o.reversible, o.reversed_by, \
                o.error_code, o.error_message, o.started_at, o.completed_at \
         FROM operations o \
         LEFT JOIN rules r ON r.id = o.rule_id \
         WHERE o.id = ?",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    row.as_ref().map(map_operation_row).transpose()
}

fn map_operation_row(r: &sqlx::sqlite::SqliteRow) -> AppResult<OperationRow> {
    let op_type_str: String = r.try_get("operation_type")?;
    let status_str: String = r.try_get("status")?;
    let cross_vol_i64: i64 = r.try_get("cross_volume")?;
    let reversible_i64: i64 = r.try_get("reversible")?;

    Ok(OperationRow {
        id: r.try_get("id")?,
        batch_id: r.try_get("batch_id")?,
        file_record_id: r.try_get("file_record_id")?,
        operation_type: OpKind::from_str(&op_type_str)?,
        source_path: r.try_get("source_path")?,
        destination_path: r.try_get("destination_path")?,
        staging_path: r.try_get("staging_path")?,
        original_name: r.try_get("original_name")?,
        new_name: r.try_get("new_name")?,
        dev: r.try_get("dev")?,
        ino: r.try_get("ino")?,
        size: r.try_get("size")?,
        mtime_ns: r.try_get("mtime_ns")?,
        content_hash: r.try_get("content_hash")?,
        cross_volume: cross_vol_i64 != 0,
        rule_id: r.try_get("rule_id")?,
        rule_name: r.try_get("rule_name")?,
        status: OpStatus::from_str(&status_str)?,
        reversible: reversible_i64 != 0,
        reversed_by: r.try_get("reversed_by")?,
        error_code: r.try_get("error_code")?,
        error_message: r.try_get("error_message")?,
        started_at: r.try_get("started_at")?,
        completed_at: r.try_get("completed_at")?,
    })
}
