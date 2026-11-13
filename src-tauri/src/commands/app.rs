use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::app_state::AppState;
use crate::error::{AppError, UiError};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub log_dir: String,
    pub database_path: String,
    pub schema_version: u32,
}

/// Read-only information for Settings → About.
#[tauri::command]
#[specta::specta]
pub async fn app_info(state: State<'_, AppState>) -> Result<AppInfo, UiError> {
    let schema_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(&state.db)
            .await
            .map_err(AppError::from)?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        data_dir: state.paths.data_dir.display().to_string(),
        log_dir: state.paths.log_dir.display().to_string(),
        database_path: state.paths.db_path.display().to_string(),
        schema_version: u32::try_from(schema_version).unwrap_or(0),
    })
}
