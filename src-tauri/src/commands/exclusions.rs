//! Global and per-folder exclusion management commands (`P6-T08`, `FR-185`).

use tauri::State;

use crate::app_state::AppState;
use crate::database::repo_exclusions::{self, ExclusionEntry, ExclusionInput};
use crate::error::UiError;

#[tauri::command]
#[specta::specta]
pub async fn exclusions_list(
    state: State<'_, AppState>,
    folder_id: Option<String>,
) -> Result<Vec<ExclusionEntry>, UiError> {
    let all = repo_exclusions::list(&state.db).await?;
    Ok(match folder_id {
        Some(fid) => all
            .into_iter()
            .filter(|e| e.folder_id.as_deref() == Some(fid.as_str()))
            .collect(),
        None => all,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn exclusions_create(
    state: State<'_, AppState>,
    input: ExclusionInput,
) -> Result<ExclusionEntry, UiError> {
    let created = repo_exclusions::create(&state.db, input).await?;
    state.monitor.refresh_exclusions().await;
    Ok(created)
}

#[tauri::command]
#[specta::specta]
pub async fn exclusions_delete(state: State<'_, AppState>, id: String) -> Result<bool, UiError> {
    let deleted = repo_exclusions::delete(&state.db, &id).await?;
    if deleted {
        state.monitor.refresh_exclusions().await;
    }
    Ok(deleted)
}
