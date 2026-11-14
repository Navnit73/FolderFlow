//! Persistence for global and per-folder exclusions (`exclusions` table; FR-016, FR-184, FR-190).

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::Row;

use super::Db;
use crate::error::{AppError, AppResult};
use crate::utils::time::now_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionKind {
    Path,
    Glob,
    Extension,
}

impl ExclusionKind {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Glob => "glob",
            Self::Extension => "extension",
        }
    }

    pub fn from_db(s: &str) -> AppResult<Self> {
        match s {
            "path" => Ok(Self::Path),
            "glob" => Ok(Self::Glob),
            "extension" => Ok(Self::Extension),
            other => Err(AppError::validation(
                "exclusionType",
                format!("unknown exclusion type: {other}"),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionEntry {
    pub id: String,
    pub folder_id: Option<String>,
    pub path: String,
    pub exclusion_type: ExclusionKind,
    #[specta(type = f64)]
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionInput {
    pub folder_id: Option<String>,
    pub path: String,
    pub exclusion_type: ExclusionKind,
}

pub fn validate_input(mut input: ExclusionInput) -> AppResult<ExclusionInput> {
    input.path = input.path.trim().to_owned();
    if input.path.is_empty() || input.path.contains('\0') {
        return Err(AppError::validation(
            "path",
            "Exclusion pattern or path cannot be empty.",
        ));
    }
    match input.exclusion_type {
        ExclusionKind::Path => {
            crate::engine::paths::lexical_check(std::path::Path::new(&input.path))?;
            input.path = crate::engine::paths::nfc(std::path::Path::new(&input.path))
                .to_string_lossy()
                .into_owned();
        }
        ExclusionKind::Glob => {
            if input.path.contains("..") || input.path.contains('/') {
                return Err(AppError::validation(
                    "path",
                    "Glob exclusions match filenames and cannot contain `/` or `..`.",
                ));
            }
            glob::Pattern::new(&input.path)
                .map_err(|e| AppError::validation("path", format!("Invalid glob pattern: {e}")))?;
        }
        ExclusionKind::Extension => {
            let ext = input.path.trim_start_matches('.').to_lowercase();
            if ext.is_empty()
                || ext.len() > 20
                || !ext
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(AppError::validation("path", "Invalid extension format."));
            }
            input.path = ext;
        }
    }
    Ok(input)
}

fn row_to_entry(r: sqlx::sqlite::SqliteRow) -> AppResult<ExclusionEntry> {
    let kind_s: String = r.get("exclusion_type");
    Ok(ExclusionEntry {
        id: r.get("id"),
        folder_id: r.get("folder_id"),
        path: r.get("path"),
        exclusion_type: ExclusionKind::from_db(&kind_s)?,
        created_at: r.get("created_at"),
    })
}

pub async fn list(db: &Db) -> AppResult<Vec<ExclusionEntry>> {
    let rows = sqlx::query(
        "SELECT id, folder_id, path, exclusion_type, created_at \
         FROM exclusions ORDER BY created_at ASC, id ASC",
    )
    .fetch_all(db)
    .await?;

    rows.into_iter().map(row_to_entry).collect()
}

pub async fn list_for_folder(db: &Db, folder_id: &str) -> AppResult<Vec<ExclusionEntry>> {
    let rows = sqlx::query(
        "SELECT id, folder_id, path, exclusion_type, created_at \
         FROM exclusions WHERE folder_id IS NULL OR folder_id = ?1 \
         ORDER BY created_at ASC, id ASC",
    )
    .bind(folder_id)
    .fetch_all(db)
    .await?;

    rows.into_iter().map(row_to_entry).collect()
}

pub async fn create(db: &Db, input: ExclusionInput) -> AppResult<ExclusionEntry> {
    let input = validate_input(input)?;
    let id = uuid::Uuid::now_v7().to_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO exclusions (id, folder_id, path, exclusion_type, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(&input.folder_id)
    .bind(&input.path)
    .bind(input.exclusion_type.as_db())
    .bind(now)
    .execute(db)
    .await?;

    Ok(ExclusionEntry {
        id,
        folder_id: input.folder_id,
        path: input.path,
        exclusion_type: input.exclusion_type,
        created_at: now,
    })
}

pub async fn delete(db: &Db, id: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM exclusions WHERE id = ?1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}
