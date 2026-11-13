use tauri::State;

use crate::app_state::AppState;
use crate::categories::Category;
use crate::engine::monitor::MonitoringStatus;
use crate::error::UiError;

#[tauri::command]
#[specta::specta]
pub async fn monitoring_status(state: State<'_, AppState>) -> Result<MonitoringStatus, UiError> {
    Ok(state.monitor.status().await)
}

/// Pauses or resumes all monitoring (FR-018, menu bar in Phase 7).
#[tauri::command]
#[specta::specta]
pub async fn monitoring_set_paused(
    state: State<'_, AppState>,
    paused: bool,
) -> Result<MonitoringStatus, UiError> {
    if paused {
        state.monitor.pause_all().await;
    } else {
        state.monitor.resume_all().await;
    }
    Ok(state.monitor.status().await)
}

#[tauri::command]
#[specta::specta]
pub async fn categories_list(state: State<'_, AppState>) -> Result<Vec<Category>, UiError> {
    Ok(state.categories.list().await?)
}
