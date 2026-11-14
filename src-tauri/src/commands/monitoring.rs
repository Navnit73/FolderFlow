use tauri::{AppHandle, State};

use crate::app_state::AppState;
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
    app: AppHandle,
    state: State<'_, AppState>,
    paused: bool,
) -> Result<MonitoringStatus, UiError> {
    state.pipeline.set_paused(paused);
    if paused {
        state.monitor.pause_all().await;
    } else {
        state.monitor.resume_all().await;
    }
    crate::system::tray::refresh_tray_async(app).await;
    Ok(state.monitor.status().await)
}
