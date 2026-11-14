//! Dashboard statistics and SQLite maintenance commands (`FR-140`, `FR-183`, `P6-T07`).

use tauri::State;

use crate::app_state::AppState;
use crate::database::repo_stats::{self, DashboardStats, DbMaintenanceReport};
use crate::error::UiError;

#[tauri::command]
#[specta::specta]
pub async fn dashboard_stats(state: State<'_, AppState>) -> Result<DashboardStats, UiError> {
    Ok(repo_stats::compute_dashboard_stats(&state.db, state.clock.as_ref()).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn db_maintenance(state: State<'_, AppState>) -> Result<DbMaintenanceReport, UiError> {
    Ok(repo_stats::run_db_maintenance(&state.db, state.clock.as_ref()).await?)
}
