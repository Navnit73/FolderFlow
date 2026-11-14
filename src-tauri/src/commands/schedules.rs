//! Automation scheduler commands (`P5-T07`, `FR-130..134`).

use tauri::State;

use crate::app_state::AppState;
use crate::database::repo_schedules::{Schedule, ScheduleInput};
use crate::engine::scheduler::ScheduleRunOutcome;
use crate::error::UiError;

#[tauri::command]
#[specta::specta]
pub async fn schedules_list(state: State<'_, AppState>) -> Result<Vec<Schedule>, UiError> {
    Ok(state.scheduler.list().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn schedules_upsert(
    state: State<'_, AppState>,
    input: ScheduleInput,
) -> Result<Schedule, UiError> {
    state.refresh_authority().await?;
    Ok(state.scheduler.upsert(input).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn schedules_delete(state: State<'_, AppState>, id: String) -> Result<(), UiError> {
    Ok(state.scheduler.delete(&id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn schedules_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<Schedule, UiError> {
    state.refresh_authority().await?;
    Ok(state.scheduler.set_enabled(&id, enabled).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn schedules_run_now(
    state: State<'_, AppState>,
    id: String,
) -> Result<ScheduleRunOutcome, UiError> {
    state.refresh_authority().await?;
    Ok(state.scheduler.run_now(&id).await?)
}

