//! Notification history and test trigger commands (`FR-200..202`, `P6-T06`).

use tauri::State;

use crate::app_state::AppState;
use crate::error::UiError;
use crate::notifications::{NotificationRecord, NotifyResult};

#[tauri::command]
#[specta::specta]
pub async fn notifications_list(
    state: State<'_, AppState>,
) -> Result<Vec<NotificationRecord>, UiError> {
    Ok(state.notifications.list_recent())
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_mark_read(
    state: State<'_, AppState>,
    id: Option<String>,
) -> Result<u32, UiError> {
    Ok(state.notifications.mark_read(id.as_deref()))
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_clear(state: State<'_, AppState>) -> Result<u32, UiError> {
    Ok(state.notifications.clear_history())
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_test(
    state: State<'_, AppState>,
) -> Result<Option<NotificationRecord>, UiError> {
    Ok(match state.notifications.send_test() {
        NotifyResult::Delivered(rec) => Some(rec),
        NotifyResult::QueuedForBatch | NotifyResult::SuppressedDisabled => None,
    })
}
