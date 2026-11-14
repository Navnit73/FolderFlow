use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::Row;

use super::repo_operations::{self, ActivityFilter, OperationView, PageRequest};
use super::Db;
use crate::error::AppResult;
use crate::utils::clock::Clock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryStorageStat {
    pub category_id: String,
    pub category_name: String,
    pub color_token: String,
    #[specta(type = f64)]
    pub file_count: u64,
    #[specta(type = f64)]
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RulePerformanceStat {
    pub rule_id: String,
    pub rule_name: String,
    pub enabled: bool,
    #[specta(type = f64)]
    pub matched_ops: u64,
    #[specta(type = f64)]
    pub bytes_organized: u64,
    pub failed_ops: u32,
    #[specta(type = Option<f64>)]
    pub last_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivityPoint {
    pub date: String,
    #[specta(type = f64)]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    #[specta(type = f64)]
    pub files_organized_total: u64,
    #[specta(type = f64)]
    pub files_organized_today: u64,
    #[specta(type = f64)]
    pub files_organized_this_week: u64,
    #[specta(type = f64)]
    pub total_bytes_organized: u64,
    pub active_folders_count: u32,
    pub total_folders_count: u32,
    pub active_rules_count: u32,
    pub total_rules_count: u32,
    pub pending_review_count: u32,
    pub failed_operations_count: u32,
    #[specta(type = f64)]
    pub total_tracked_files: u64,
    #[specta(type = f64)]
    pub total_tracked_bytes: u64,
    pub duplicate_groups_count: u32,
    #[specta(type = f64)]
    pub duplicate_wasted_bytes: u64,
    pub category_breakdown: Vec<CategoryStorageStat>,
    pub top_rules: Vec<RulePerformanceStat>,
    pub daily_activity: Vec<DailyActivityPoint>,
    pub recent_operations: Vec<OperationView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DbMaintenanceReport {
    #[specta(type = f64)]
    pub freelist_pages_before: u64,
    #[specta(type = f64)]
    pub freelist_pages_after: u64,
    #[specta(type = f64)]
    pub size_bytes_before: u64,
    #[specta(type = f64)]
    pub size_bytes_after: u64,
    #[specta(type = f64)]
    pub completed_at_ms: i64,
}

fn nonneg_u64(v: i64) -> u64 {
    u64::try_from(v.max(0)).unwrap_or(0)
}

fn nonneg_u32(v: i64) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(u32::MAX)
}

fn local_midnight_utc_ms(date: chrono::NaiveDate, clock: &dyn Clock, fallback_ms: i64) -> i64 {
    let naive = NaiveDateTime::new(date, chrono::NaiveTime::MIN);
    let approx_utc = naive.and_utc().timestamp_millis();
    let offset = clock.to_local(approx_utc).offset().to_owned();
    naive
        .and_local_timezone(offset)
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(fallback_ms)
}

/// Computes live SQLite-backed dashboard metrics using local calendar boundaries from `clock`.
pub async fn compute_dashboard_stats(db: &Db, clock: &dyn Clock) -> AppResult<DashboardStats> {
    let now_ms = clock.now_ms();
    let today_date = clock.local_date(now_ms);
    let today_start_ms =
        local_midnight_utc_ms(today_date, clock, now_ms.saturating_sub(86_400_000));
    let days_from_mon = i64::from(today_date.weekday().num_days_from_monday());
    let week_start_date = today_date - Duration::days(days_from_mon);
    let week_start_ms = local_midnight_utc_ms(
        week_start_date,
        clock,
        now_ms.saturating_sub(7 * 86_400_000),
    );
    let seven_days_start_date = today_date - Duration::days(6);
    let seven_days_start_ms = local_midnight_utc_ms(
        seven_days_start_date,
        clock,
        now_ms.saturating_sub(7 * 86_400_000),
    );

    let op_row = sqlx::query(
        "SELECT
            COUNT(*) FILTER (
                WHERE status = 'COMPLETED'
                  AND operation_type IN ('move','copy','rename','trash')
                  AND reversed_by IS NULL
            ) AS total_ops,
            COUNT(*) FILTER (
                WHERE status = 'COMPLETED'
                  AND operation_type IN ('move','copy','rename','trash')
                  AND reversed_by IS NULL
                  AND COALESCE(completed_at, started_at) >= ?1
            ) AS today_ops,
            COUNT(*) FILTER (
                WHERE status = 'COMPLETED'
                  AND operation_type IN ('move','copy','rename','trash')
                  AND reversed_by IS NULL
                  AND COALESCE(completed_at, started_at) >= ?2
            ) AS week_ops,
            CAST(COALESCE(SUM(CASE
                WHEN status = 'COMPLETED'
                 AND operation_type IN ('move','copy','rename','trash')
                 AND reversed_by IS NULL
                THEN COALESCE(size, 0) ELSE 0 END), 0) AS INTEGER) AS total_bytes,
            COUNT(*) FILTER (WHERE status = 'FAILED') AS failed_ops
         FROM operations",
    )
    .bind(today_start_ms)
    .bind(week_start_ms)
    .fetch_one(db)
    .await?;

    let counts_row = sqlx::query(
        "SELECT
            (SELECT COUNT(*) FROM monitored_folders WHERE enabled = 1) AS active_folders,
            (SELECT COUNT(*) FROM monitored_folders) AS total_folders,
            (SELECT COUNT(*) FROM rules WHERE enabled = 1) AS active_rules,
            (SELECT COUNT(*) FROM rules) AS total_rules,
            (SELECT COUNT(*) FROM review_queue WHERE resolved_at IS NULL) AS pending_review,
            (SELECT COUNT(*) FROM file_records) AS tracked_files,
            (SELECT CAST(COALESCE(SUM(size), 0) AS INTEGER) FROM file_records) AS tracked_bytes",
    )
    .fetch_one(db)
    .await?;

    let dup_row = sqlx::query(
        "SELECT
            COUNT(*) AS group_count,
            CAST(COALESCE(SUM(CASE WHEN mc > 1 THEN (mc - 1) * file_size ELSE 0 END), 0) AS INTEGER) AS wasted_bytes
         FROM (
            SELECT g.file_size, COUNT(m.file_record_id) AS mc
            FROM duplicate_groups g
            JOIN duplicate_members m ON m.group_id = g.id
            WHERE g.ignored = 0
            GROUP BY g.id
            HAVING COUNT(m.file_record_id) > 1
         )",
    )
    .fetch_one(db)
    .await?;

    let cat_rows = sqlx::query(
        "SELECT
            COALESCE(c.id, 'other') AS category_id,
            COALESCE(c.name, 'Other') AS category_name,
            COALESCE(c.color_token, 'cat-other') AS color_token,
            COUNT(*) AS file_count,
            CAST(COALESCE(SUM(f.size), 0) AS INTEGER) AS total_bytes
         FROM file_records f
         LEFT JOIN categories c ON c.id = f.category
         GROUP BY COALESCE(c.id, 'other'), COALESCE(c.name, 'Other'), COALESCE(c.color_token, 'cat-other')
         ORDER BY total_bytes DESC, file_count DESC",
    )
    .fetch_all(db)
    .await?;

    let category_breakdown = cat_rows
        .into_iter()
        .map(|r| -> AppResult<CategoryStorageStat> {
            Ok(CategoryStorageStat {
                category_id: r.try_get("category_id")?,
                category_name: r.try_get("category_name")?,
                color_token: r.try_get("color_token")?,
                file_count: nonneg_u64(r.try_get("file_count")?),
                total_bytes: nonneg_u64(r.try_get("total_bytes")?),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    let rule_rows = sqlx::query(
        "SELECT
            r.id AS rule_id,
            r.name AS rule_name,
            r.enabled AS enabled,
            r.last_run_at AS last_run_at,
            COUNT(o.id) FILTER (
                WHERE o.status = 'COMPLETED'
                  AND o.operation_type IN ('move','copy','rename','trash')
                  AND o.reversed_by IS NULL
            ) AS matched_ops,
            CAST(COALESCE(SUM(CASE
                WHEN o.status = 'COMPLETED'
                 AND o.operation_type IN ('move','copy','rename','trash')
                 AND o.reversed_by IS NULL
                THEN COALESCE(o.size, 0) ELSE 0 END), 0) AS INTEGER) AS bytes_organized,
            COUNT(o.id) FILTER (WHERE o.status = 'FAILED') AS failed_ops
         FROM rules r
         LEFT JOIN operations o ON o.rule_id = r.id
         GROUP BY r.id, r.name, r.enabled, r.last_run_at
         ORDER BY matched_ops DESC, r.priority ASC, r.name ASC
         LIMIT 6",
    )
    .fetch_all(db)
    .await?;

    let top_rules = rule_rows
        .into_iter()
        .map(|r| -> AppResult<RulePerformanceStat> {
            Ok(RulePerformanceStat {
                rule_id: r.try_get("rule_id")?,
                rule_name: r.try_get("rule_name")?,
                enabled: r.try_get("enabled")?,
                matched_ops: nonneg_u64(r.try_get("matched_ops")?),
                bytes_organized: nonneg_u64(r.try_get("bytes_organized")?),
                failed_ops: nonneg_u32(r.try_get("failed_ops")?),
                last_run_at_ms: r.try_get("last_run_at")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    // Bucket recent 7 local days in Rust via Clock so DST and timezone offsets stay exact.
    let ts_rows = sqlx::query(
        "SELECT COALESCE(completed_at, started_at) AS ts
         FROM operations
         WHERE status = 'COMPLETED'
           AND operation_type IN ('move','copy','rename','trash')
           AND reversed_by IS NULL
           AND COALESCE(completed_at, started_at) >= ?1",
    )
    .bind(seven_days_start_ms)
    .fetch_all(db)
    .await?;

    let mut per_day: HashMap<String, u64> = HashMap::new();
    for r in ts_rows {
        let ts: i64 = r.try_get("ts")?;
        let d = clock.local_date(ts).format("%Y-%m-%d").to_string();
        *per_day.entry(d).or_insert(0) += 1;
    }

    let mut daily_activity = Vec::with_capacity(7);
    for offset in 0..7_i64 {
        let key = (seven_days_start_date + Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string();
        let count = per_day.get(&key).copied().unwrap_or(0);
        daily_activity.push(DailyActivityPoint { date: key, count });
    }

    let recent_page = repo_operations::list_page(
        db,
        &ActivityFilter::default(),
        PageRequest {
            limit: 8,
            offset: 0,
        },
    )
    .await?;

    Ok(DashboardStats {
        files_organized_total: nonneg_u64(op_row.try_get("total_ops")?),
        files_organized_today: nonneg_u64(op_row.try_get("today_ops")?),
        files_organized_this_week: nonneg_u64(op_row.try_get("week_ops")?),
        total_bytes_organized: nonneg_u64(op_row.try_get("total_bytes")?),
        active_folders_count: nonneg_u32(counts_row.try_get("active_folders")?),
        total_folders_count: nonneg_u32(counts_row.try_get("total_folders")?),
        active_rules_count: nonneg_u32(counts_row.try_get("active_rules")?),
        total_rules_count: nonneg_u32(counts_row.try_get("total_rules")?),
        pending_review_count: nonneg_u32(counts_row.try_get("pending_review")?),
        failed_operations_count: nonneg_u32(op_row.try_get("failed_ops")?),
        total_tracked_files: nonneg_u64(counts_row.try_get("tracked_files")?),
        total_tracked_bytes: nonneg_u64(counts_row.try_get("tracked_bytes")?),
        duplicate_groups_count: nonneg_u32(dup_row.try_get("group_count")?),
        duplicate_wasted_bytes: nonneg_u64(dup_row.try_get("wasted_bytes")?),
        category_breakdown,
        top_rules,
        daily_activity,
        recent_operations: recent_page.items,
    })
}

async fn read_sqlite_size_stats(db: &Db) -> AppResult<(u64, u64)> {
    let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(db)
        .await?;
    let page_size: i64 = sqlx::query_scalar("PRAGMA page_size").fetch_one(db).await?;
    let freelist: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(db)
        .await?;
    let total_bytes = nonneg_u64(page_count).saturating_mul(nonneg_u64(page_size));
    Ok((nonneg_u64(freelist), total_bytes))
}

/// Runs `wal_checkpoint(TRUNCATE)`, `ANALYZE`, and `VACUUM` for Performance settings maintenance (FR-183).
pub async fn run_db_maintenance(db: &Db, clock: &dyn Clock) -> AppResult<DbMaintenanceReport> {
    let (freelist_pages_before, size_bytes_before) = read_sqlite_size_stats(db).await?;
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(db)
        .await?;
    sqlx::query("ANALYZE").execute(db).await?;
    sqlx::query("VACUUM").execute(db).await?;
    let (freelist_pages_after, size_bytes_after) = read_sqlite_size_stats(db).await?;

    Ok(DbMaintenanceReport {
        freelist_pages_before,
        freelist_pages_after,
        size_bytes_before,
        size_bytes_after,
        completed_at_ms: clock.now_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::CategoryService;
    use crate::database;
    use crate::utils::clock::FixedClock;

    #[tokio::test]
    async fn r_stats_01_empty_database_returns_all_zero_metrics_without_hardcoding() {
        let db = database::open_in_memory().await.expect("db");
        CategoryService::new(db.clone())
            .ensure_builtins()
            .await
            .expect("cats");
        let clock = FixedClock::utc(1_700_000_000_000);
        let stats = compute_dashboard_stats(&db, &clock).await.expect("stats");

        assert_eq!(stats.files_organized_total, 0);
        assert_eq!(stats.files_organized_today, 0);
        assert_eq!(stats.files_organized_this_week, 0);
        assert_eq!(stats.total_bytes_organized, 0);
        assert_eq!(stats.active_folders_count, 0);
        assert_eq!(stats.total_folders_count, 0);
        assert_eq!(stats.active_rules_count, 0);
        assert_eq!(stats.total_rules_count, 0);
        assert_eq!(stats.pending_review_count, 0);
        assert_eq!(stats.failed_operations_count, 0);
        assert_eq!(stats.total_tracked_files, 0);
        assert_eq!(stats.total_tracked_bytes, 0);
        assert_eq!(stats.duplicate_groups_count, 0);
        assert_eq!(stats.duplicate_wasted_bytes, 0);
        assert!(stats.category_breakdown.is_empty());
        assert!(stats.top_rules.is_empty());
        assert_eq!(stats.daily_activity.len(), 7);
        assert!(stats.daily_activity.iter().all(|p| p.count == 0));
    }

    #[tokio::test]
    async fn r_stats_02_populated_db_aggregates_today_week_total_excludes_undone_and_counts_dups() {
        let db = database::open_in_memory().await.expect("db");
        CategoryService::new(db.clone())
            .ensure_builtins()
            .await
            .expect("cats");

        // Wednesday 2024-06-12 12:00:00 UTC in ms:
        // 2024-06-12 00:00:00Z = 1718150400000
        let wed_noon_ms = 1_718_193_600_000_i64;
        let wed_morning_ms = 1_718_160_000_000_i64; // same day
        let mon_afternoon_ms = 1_718_000_000_000_i64; // Monday same week (2024-06-10)
        let prev_week_ms = 1_717_000_000_000_i64; // earlier month

        // Folder + Rule
        sqlx::query(
            "INSERT INTO monitored_folders (id, path, name, enabled, settings_json, created_at, updated_at)
             VALUES ('f1', '/tmp/Downloads', 'Downloads', 1, '{}', 1, 1),
                    ('f2', '/tmp/Paused', 'Paused', 0, '{}', 1, 1)",
        )
        .execute(&db)
        .await
        .expect("folders");
        sqlx::query(
            "INSERT INTO rules (id, name, priority, enabled, source_folders_json, conditions_json, actions_json, execution_mode, origin, created_at, updated_at)
             VALUES ('r1', 'PDFs', 10, 1, '[]', '{}', '[]', 'first_match', 'user', 1, 1),
                    ('r2', 'Disabled', 20, 0, '[]', '{}', '[]', 'first_match', 'user', 1, 1)",
        )
        .execute(&db)
        .await
        .expect("rules");

        // Operations:
        // op1: completed today (size 1000) -> counts in today, week, total
        // op2: completed Monday (size 2000) -> counts in week, total
        // op3: completed prev week (size 3000) -> counts in total
        // op4: completed today BUT undone (reversed_by = op_undo) -> excluded!
        // op5: undo_move completed today -> excluded!
        // op6: failed op -> increments failed_operations_count
        sqlx::query(
            "INSERT INTO operations (id, operation_type, source_path, destination_path, original_name, new_name, size, rule_id, status, started_at, completed_at, reversed_by)
             VALUES
             ('op1', 'move', '/a/1.pdf', '/b/1.pdf', '1.pdf', '1.pdf', 1000, 'r1', 'COMPLETED', ?1, ?1, NULL),
             ('op2', 'rename', '/a/2.pdf', '/a/2b.pdf', '2.pdf', '2b.pdf', 2000, 'r1', 'COMPLETED', ?2, ?2, NULL),
             ('op3', 'copy', '/a/3.pdf', '/b/3.pdf', '3.pdf', '3.pdf', 3000, 'r1', 'COMPLETED', ?3, ?3, NULL),
             ('op4', 'move', '/a/4.pdf', '/b/4.pdf', '4.pdf', '4.pdf', 4000, 'r1', 'COMPLETED', ?1, ?1, 'op5'),
             ('op5', 'undo_move', '/b/4.pdf', '/a/4.pdf', '4.pdf', '4.pdf', 4000, NULL, 'COMPLETED', ?1, ?1, NULL),
             ('op6', 'move', '/a/5.pdf', '/b/5.pdf', '5.pdf', '5.pdf', 5000, 'r1', 'FAILED', ?1, NULL, NULL)",
        )
        .bind(wed_morning_ms)
        .bind(mon_afternoon_ms)
        .bind(prev_week_ms)
        .execute(&db)
        .await
        .expect("ops");

        // Pending & resolved review items
        sqlx::query(
            "INSERT INTO review_queue (id, path, reason, created_at, resolved_at)
             VALUES ('q1', '/a/x', 'conflict', 100, NULL),
                    ('q2', '/a/y', 'conflict', 100, 200)",
        )
        .execute(&db)
        .await
        .expect("review");

        // Tracked files + Duplicate group
        sqlx::query(
            "INSERT INTO file_records (id, folder_id, path, filename, extension, category, confidence, is_package, size, dev, ino, modified_at_ns, last_scanned_at)
             VALUES
             ('fr1', 'f1', '/tmp/Downloads/a.pdf', 'a.pdf', 'pdf', 'documents', 'high', 0, 500, 1, 1, 1, 1),
             ('fr2', 'f1', '/tmp/Downloads/b.pdf', 'b.pdf', 'pdf', 'documents', 'high', 0, 500, 1, 2, 1, 1),
             ('fr3', 'f1', '/tmp/Downloads/img.png', 'img.png', 'png', 'images', 'high', 0, 1500, 1, 3, 1, 1)",
        )
        .execute(&db)
        .await
        .expect("file_records");

        sqlx::query(
            "INSERT INTO duplicate_groups (id, content_hash, file_size, ignored, created_at)
             VALUES ('g1', 'hash1', 500, 0, 10),
                    ('g_ignored', 'hash2', 9000, 1, 10)",
        )
        .execute(&db)
        .await
        .expect("dup_groups");
        sqlx::query(
            "INSERT INTO duplicate_members (group_id, file_record_id, is_keeper)
             VALUES ('g1', 'fr1', 1), ('g1', 'fr2', 0),
                    ('g_ignored', 'fr3', 1), ('g_ignored', 'fr1', 0)",
        )
        .execute(&db)
        .await
        .expect("dup_members");

        let clock = FixedClock::utc(wed_noon_ms);
        let stats = compute_dashboard_stats(&db, &clock).await.expect("stats");

        assert_eq!(stats.files_organized_today, 1);
        assert_eq!(stats.files_organized_this_week, 2);
        assert_eq!(stats.files_organized_total, 3);
        assert_eq!(stats.total_bytes_organized, 6000);
        assert_eq!(stats.active_folders_count, 1);
        assert_eq!(stats.total_folders_count, 2);
        assert_eq!(stats.active_rules_count, 1);
        assert_eq!(stats.total_rules_count, 2);
        assert_eq!(stats.pending_review_count, 1);
        assert_eq!(stats.failed_operations_count, 1);
        assert_eq!(stats.total_tracked_files, 3);
        assert_eq!(stats.total_tracked_bytes, 2500);
        assert_eq!(stats.duplicate_groups_count, 1);
        assert_eq!(stats.duplicate_wasted_bytes, 500);
        assert_eq!(stats.category_breakdown.len(), 2);
        assert_eq!(stats.category_breakdown[0].category_id, "images");
        assert_eq!(stats.category_breakdown[0].total_bytes, 1500);
        assert_eq!(stats.top_rules[0].rule_id, "r1");
        assert_eq!(stats.top_rules[0].matched_ops, 3);
        assert_eq!(stats.top_rules[0].failed_ops, 1);
        assert_eq!(stats.recent_operations.len(), 6);

        // Local timezone rollover check (+05:30 IST): 19:00 UTC is already 00:30 next day in IST!
        let ist_clock = FixedClock::with_offset_mins(1_718_218_800_000, 330); // 2024-06-12 19:00 UTC = 2024-06-13 00:30 IST
        let ist_stats = compute_dashboard_stats(&db, &ist_clock).await.expect("ist");
        assert_eq!(ist_stats.files_organized_today, 0);
        assert_eq!(ist_stats.files_organized_this_week, 2);
    }

    #[tokio::test]
    async fn r_stats_03_db_maintenance_runs_analyze_and_vacuum_cleanly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = database::open(&dir.path().join("maint.db"))
            .await
            .expect("open");
        let clock = FixedClock::utc(1_700_000_000_000);
        let report = run_db_maintenance(&db, &clock).await.expect("maintenance");
        assert_eq!(report.completed_at_ms, 1_700_000_000_000);
        assert!(report.size_bytes_after > 0);
        assert_eq!(report.freelist_pages_after, 0);
    }
}
