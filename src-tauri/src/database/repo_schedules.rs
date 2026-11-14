//! Persistence for automated schedules (`schedules` table, `P5-T05`, `FR-130..134`).

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::Row;

use super::Db;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleType {
    Realtime,
    Interval,
    Daily,
    Weekly,
    OnLaunch,
    Manual,
}

impl ScheduleType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Realtime => "realtime",
            Self::Interval => "interval",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::OnLaunch => "on_launch",
            Self::Manual => "manual",
        }
    }

    pub fn from_db(s: &str) -> AppResult<Self> {
        match s {
            "realtime" => Ok(Self::Realtime),
            "interval" => Ok(Self::Interval),
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            "on_launch" => Ok(Self::OnLaunch),
            "manual" => Ok(Self::Manual),
            other => Err(AppError::validation(
                "scheduleType",
                format!("unknown schedule type: {other}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn to_chrono(self) -> chrono::Weekday {
        match self {
            Self::Mon => chrono::Weekday::Mon,
            Self::Tue => chrono::Weekday::Tue,
            Self::Wed => chrono::Weekday::Wed,
            Self::Thu => chrono::Weekday::Thu,
            Self::Fri => chrono::Weekday::Fri,
            Self::Sat => chrono::Weekday::Sat,
            Self::Sun => chrono::Weekday::Sun,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatchUpPolicy {
    #[default]
    Skip,
    RunOnce,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuietHours {
    /// Local wall-clock time `HH:MM` (24-hour).
    pub start: String,
    /// Local wall-clock time `HH:MM` (24-hour).
    pub end: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleConfig {
    /// Used when `schedule_type == Interval` (e.g., 300, 900, 1800, 3600).
    pub interval_secs: Option<u32>,
    /// Local wall-clock time `HH:MM` used for `Daily` and `Weekly`.
    pub time: Option<String>,
    /// Days of week for `Weekly` schedules.
    #[serde(default)]
    pub weekdays: Vec<Weekday>,
    /// Optional quiet hours window during which automatic execution is deferred.
    pub quiet_hours: Option<QuietHours>,
    /// Behavior when scheduled runs were missed while asleep (`FR-133`).
    #[serde(default)]
    pub catch_up: CatchUpPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub rule_id: Option<String>,
    pub folder_id: Option<String>,
    pub schedule_type: ScheduleType,
    pub config: ScheduleConfig,
    pub enabled: bool,
    #[specta(type = Option<f64>)]
    pub last_run_at: Option<i64>,
    #[specta(type = Option<f64>)]
    pub next_run_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInput {
    pub id: Option<String>,
    pub rule_id: Option<String>,
    pub folder_id: Option<String>,
    pub schedule_type: ScheduleType,
    pub config: ScheduleConfig,
    pub enabled: bool,
}

fn from_row(row: &sqlx::sqlite::SqliteRow) -> AppResult<Schedule> {
    let type_str: String = row.try_get("schedule_type")?;
    let cfg_json: String = row.try_get("schedule_config")?;
    let config: ScheduleConfig = serde_json::from_str(&cfg_json)?;
    Ok(Schedule {
        id: row.try_get("id")?,
        rule_id: row.try_get("rule_id")?,
        folder_id: row.try_get("folder_id")?,
        schedule_type: ScheduleType::from_db(&type_str)?,
        config,
        enabled: row.try_get("enabled")?,
        last_run_at: row.try_get("last_run_at")?,
        next_run_at: row.try_get("next_run_at")?,
    })
}

macro_rules! select_schedules {
    ($where:literal) => {
        concat!(
            "SELECT id, rule_id, folder_id, schedule_type, schedule_config, enabled, last_run_at, next_run_at ",
            "FROM schedules ",
            $where
        )
    };
}

pub async fn list(db: &Db) -> AppResult<Vec<Schedule>> {
    let rows = sqlx::query(select_schedules!("ORDER BY id ASC"))
        .fetch_all(db)
        .await?;
    rows.iter().map(from_row).collect()
}

pub async fn get(db: &Db, id: &str) -> AppResult<Option<Schedule>> {
    let row = sqlx::query(select_schedules!("WHERE id = ?1"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    row.as_ref().map(from_row).transpose()
}

pub async fn upsert(db: &Db, schedule: &Schedule) -> AppResult<()> {
    let cfg_json = serde_json::to_string(&schedule.config)?;
    sqlx::query(
        "INSERT INTO schedules (id, rule_id, folder_id, schedule_type, schedule_config, enabled, last_run_at, next_run_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(id) DO UPDATE SET \
           rule_id = excluded.rule_id, \
           folder_id = excluded.folder_id, \
           schedule_type = excluded.schedule_type, \
           schedule_config = excluded.schedule_config, \
           enabled = excluded.enabled, \
           next_run_at = excluded.next_run_at",
    )
    .bind(&schedule.id)
    .bind(&schedule.rule_id)
    .bind(&schedule.folder_id)
    .bind(schedule.schedule_type.as_db())
    .bind(cfg_json)
    .bind(schedule.enabled)
    .bind(schedule.last_run_at)
    .bind(schedule.next_run_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_enabled(
    db: &Db,
    id: &str,
    enabled: bool,
    next_run_at: Option<i64>,
) -> AppResult<bool> {
    let r = sqlx::query("UPDATE schedules SET enabled = ?2, next_run_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(enabled)
        .bind(next_run_at)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn record_run(
    db: &Db,
    id: &str,
    last_run_at: Option<i64>,
    next_run_at: Option<i64>,
) -> AppResult<bool> {
    let r = sqlx::query("UPDATE schedules SET last_run_at = ?2, next_run_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(last_run_at)
        .bind(next_run_at)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete(db: &Db, id: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM schedules WHERE id = ?1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}
