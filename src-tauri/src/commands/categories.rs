//! Category CRUD, priority reorder, and live classifier rebuild commands (`FR-170`, `P6-T08`).

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::categories::{Category, CategoryInput};
use crate::commands::rules::pick_and_approve_destination_folder;
use crate::error::UiError;

#[tauri::command]
#[specta::specta]
pub async fn categories_list(state: State<'_, AppState>) -> Result<Vec<Category>, UiError> {
    Ok(state.categories.list().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn categories_create(
    state: State<'_, AppState>,
    input: CategoryInput,
) -> Result<Category, UiError> {
    let created = state.categories.create(input).await?;
    state.refresh_classifier().await?;
    Ok(created)
}

#[tauri::command]
#[specta::specta]
pub async fn categories_update(
    state: State<'_, AppState>,
    id: String,
    input: CategoryInput,
) -> Result<Category, UiError> {
    let updated = state.categories.update(&id, input).await?;
    state.refresh_classifier().await?;
    Ok(updated)
}

#[tauri::command]
#[specta::specta]
pub async fn categories_delete(state: State<'_, AppState>, id: String) -> Result<(), UiError> {
    state.categories.delete(&id).await?;
    state.refresh_classifier().await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn categories_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<Category, UiError> {
    let updated = state.categories.set_enabled(&id, enabled).await?;
    state.refresh_classifier().await?;
    Ok(updated)
}

#[tauri::command]
#[specta::specta]
pub async fn categories_reorder(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<Vec<Category>, UiError> {
    let updated = state.categories.reorder(ordered_ids).await?;
    state.refresh_classifier().await?;
    Ok(updated)
}

#[tauri::command]
#[specta::specta]
pub async fn categories_pick_destination_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, UiError> {
    pick_and_approve_destination_folder(app, state, "Choose a destination folder for this category")
        .await
}
