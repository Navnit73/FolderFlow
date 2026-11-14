use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::AppState;
use crate::error::UiError;
use crate::security::license::{LicenseService, LicenseStatus};

#[tauri::command]
#[specta::specta]
pub async fn license_status(state: State<'_, AppState>) -> Result<LicenseStatus, UiError> {
    Ok(state.license.status()?)
}

#[tauri::command]
#[specta::specta]
pub async fn license_activate(
    state: State<'_, AppState>,
    key: String,
) -> Result<LicenseStatus, UiError> {
    Ok(state.license.activate(&key).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn license_activate_offline(
    state: State<'_, AppState>,
    signed_key_or_json: String,
) -> Result<LicenseStatus, UiError> {
    Ok(state.license.activate_offline(&signed_key_or_json)?)
}

#[tauri::command]
#[specta::specta]
pub async fn license_pick_and_activate_offline_file(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<LicenseStatus>, UiError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a FolderFlow License File")
        .add_filter("FolderFlow License", &["fflicense", "json", "txt"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Ok(Some(file_path)) = rx.await else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|e| {
        crate::error::AppError::validation("licenseFile", format!("Unsupported path: {e}"))
    })?;
    let license: Arc<LicenseService> = state.license.clone();
    let status = tokio::task::spawn_blocking(move || license.activate_offline_file(&path))
        .await
        .map_err(|e| crate::error::AppError::Internal(format!("license task failed: {e}")))??;
    Ok(Some(status))
}

#[tauri::command]
#[specta::specta]
pub async fn license_deactivate(state: State<'_, AppState>) -> Result<LicenseStatus, UiError> {
    Ok(state.license.deactivate().await?)
}
