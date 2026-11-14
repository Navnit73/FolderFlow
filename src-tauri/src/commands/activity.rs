//! Activity log, undo, retry, CSV export, Finder reveal, and review-queue commands (`P4-T10`).

use tauri::State;

use crate::app_state::AppState;
use crate::database::repo_operations::{
    ActivityFilter, ActivityPage, OperationView, PageRequest, ReviewItemView,
};
use crate::engine::activity::{RevealTarget, ReviewDecision, ReviewResolveReport};
use crate::engine::undo::{UndoReport, UndoTarget};
use crate::error::UiError;
use crate::system;

#[tauri::command]
#[specta::specta]
pub async fn activity_list(
    state: State<'_, AppState>,
    filter: ActivityFilter,
    page: PageRequest,
) -> Result<ActivityPage, UiError> {
    Ok(state.activity.list(filter, page).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn activity_undo(
    state: State<'_, AppState>,
    target: UndoTarget,
) -> Result<UndoReport, UiError> {
    Ok(state.activity.undo(target).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn activity_retry(
    state: State<'_, AppState>,
    op_id: String,
) -> Result<OperationView, UiError> {
    Ok(state.activity.retry(&op_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn activity_export_csv(
    state: State<'_, AppState>,
    filter: ActivityFilter,
) -> Result<String, UiError> {
    Ok(state.activity.export_csv(filter).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn activity_clear(
    state: State<'_, AppState>,
    before_ms: Option<f64>,
) -> Result<u32, UiError> {
    Ok(state.activity.clear(before_ms.map(|v| v as i64)).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn reveal_in_finder(
    state: State<'_, AppState>,
    op_id: String,
    which: RevealTarget,
) -> Result<bool, UiError> {
    let path = state.activity.resolve_reveal_path(&op_id, which).await?;
    Ok(system::reveal_in_finder(&path))
}

#[tauri::command]
#[specta::specta]
pub async fn review_list(state: State<'_, AppState>) -> Result<Vec<ReviewItemView>, UiError> {
    Ok(state.activity.review_list().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn review_resolve(
    state: State<'_, AppState>,
    id: String,
    decision: ReviewDecision,
) -> Result<ReviewResolveReport, UiError> {
    Ok(state.activity.review_resolve(&id, decision).await?)
}
