//! Versioned configuration export and validated diff-preview import commands (`FR-190..192`, `P6-T05`).

use std::io::Read;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::AppState;
use crate::config_io::{
    apply_config_import, export_config, preview_config_import_with_mode, ConfigImportMode,
    ConfigImportPreview, ConfigImportReport,
};
use crate::engine::fs_view::{ReadOnlyFs, RealFs};
use crate::error::{AppError, UiError};

const MAX_IMPORT_JSON_BYTES: u64 = 5 * 1024 * 1024;

#[tauri::command]
#[specta::specta]
pub async fn config_export(state: State<'_, AppState>) -> Result<String, UiError> {
    Ok(export_config(&state.db, env!("CARGO_PKG_VERSION"), state.clock.now_ms()).await?)
}

/// Opens a native file picker for a `.json` backup and reads up to 5 MiB safely.
#[tauri::command]
#[specta::specta]
pub async fn config_pick_import_file(app: AppHandle) -> Result<Option<String>, UiError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose a FolderFlow configuration backup")
        .add_filter("JSON backup", &["json"]);
    if let Ok(home) = app.path().home_dir() {
        dialog = dialog.set_directory(home);
    }
    dialog.pick_file(move |picked| {
        let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
    });
    let Some(path) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let reader = RealFs.open_read(&path).map_err(AppError::from)?;
    let mut buf = Vec::new();
    reader
        .take(MAX_IMPORT_JSON_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(AppError::from)?;
    if buf.len() as u64 > MAX_IMPORT_JSON_BYTES {
        return Err(AppError::validation(
            "file",
            "Configuration backup file exceeds 5 MiB size limit.",
        )
        .into());
    }
    let text = String::from_utf8(buf).map_err(|_| {
        AppError::validation("file", "Configuration backup must be valid UTF-8 JSON.")
    })?;
    Ok(Some(text))
}

#[tauri::command]
#[specta::specta]
pub async fn config_import_preview(
    state: State<'_, AppState>,
    raw_json: String,
    mode: Option<ConfigImportMode>,
) -> Result<ConfigImportPreview, UiError> {
    Ok(preview_config_import_with_mode(
        &state.db,
        &state.root_policy,
        &raw_json,
        mode.unwrap_or(ConfigImportMode::Merge),
    )
    .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn config_import_apply(
    state: State<'_, AppState>,
    raw_json: String,
    mode: ConfigImportMode,
) -> Result<ConfigImportReport, UiError> {
    let report = apply_config_import(
        &state.db,
        &state.root_policy,
        &raw_json,
        mode,
        state.clock.now_ms(),
    )
    .await?;
    state.refresh_after_config_import().await?;
    Ok(report)
}
