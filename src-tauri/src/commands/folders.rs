//! Monitored-folder commands. The frontend passes folder IDs and fixed
//! choices — never arbitrary paths; folder paths come from the native picker
//! or well-known locations resolved in Rust.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::AppState;
use crate::database::{repo_files, repo_folders};
use crate::error::{AppError, UiError};
use crate::folders::{probe_access, FolderAccess, FolderPatch, KnownFolder, MonitoredFolder};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderAction {
    Pause,
    Resume,
    Rescan,
    Restart,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryUsage {
    pub category: String,
    pub files: u32,
    #[specta(type = f64)]
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderSummary {
    pub files: u32,
    #[specta(type = f64)]
    pub bytes: i64,
    pub by_category: Vec<CategoryUsage>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatcherErrorView {
    pub code: String,
    pub message: String,
    #[specta(type = f64)]
    pub occurred_at: i64,
}

async fn pick_folder(
    app: &AppHandle,
    title: &str,
    start: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title(title)
        .set_can_create_directories(false);
    if let Some(dir) = start {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
    });
    rx.await.ok().flatten()
}

async fn added(state: &AppState, folder: MonitoredFolder) -> Result<MonitoredFolder, UiError> {
    state.refresh_authority().await?;
    state.monitor.start(folder.clone()).await;
    Ok(folder)
}

#[tauri::command]
#[specta::specta]
pub async fn folders_list(state: State<'_, AppState>) -> Result<Vec<MonitoredFolder>, UiError> {
    Ok(state.folders.list().await?)
}

/// Opens the native folder picker and starts monitoring the chosen folder.
/// Returns `None` if the user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn folders_add_with_picker(
    app: AppHandle,
    state: State<'_, AppState>,
    recursive: bool,
) -> Result<Option<MonitoredFolder>, UiError> {
    let start = app.path().home_dir().ok();
    let Some(path) = pick_folder(&app, "Choose a folder for FolderFlow to monitor", start).await
    else {
        return Ok(None);
    };
    let folder = state.folders.add(&path, recursive).await?;
    Ok(Some(added(&state, folder).await?))
}

#[tauri::command]
#[specta::specta]
pub async fn folders_add_known(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: KnownFolder,
    recursive: bool,
) -> Result<MonitoredFolder, UiError> {
    let p = app.path();
    let path = match kind {
        KnownFolder::Downloads => p.download_dir(),
        KnownFolder::Desktop => p.desktop_dir(),
        KnownFolder::Documents => p.document_dir(),
        KnownFolder::Pictures => p.picture_dir(),
        KnownFolder::Movies => p.video_dir(),
        KnownFolder::Music => p.audio_dir(),
    }
    .map_err(|e| AppError::Internal(format!("resolve known folder: {e}")))?;
    let folder = state.folders.add(&path, recursive).await?;
    added(&state, folder).await
}

/// Applies edits and restarts monitoring so new settings take effect.
#[tauri::command]
#[specta::specta]
pub async fn folders_update(
    state: State<'_, AppState>,
    id: String,
    patch: FolderPatch,
) -> Result<MonitoredFolder, UiError> {
    let folder = state.folders.update(&id, patch).await?;
    state.refresh_authority().await?;
    if folder.enabled {
        state.monitor.start(folder.clone()).await;
    } else {
        state.monitor.stop(&id).await;
    }
    Ok(folder)
}

/// Stops monitoring and forgets the folder. Files are never touched.
#[tauri::command]
#[specta::specta]
pub async fn folders_remove(state: State<'_, AppState>, id: String) -> Result<(), UiError> {
    state.monitor.stop(&id).await;
    state.folders.remove(&id).await?;
    state.refresh_authority().await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn folders_action(
    state: State<'_, AppState>,
    id: String,
    action: FolderAction,
) -> Result<(), UiError> {
    let folder = state.folders.get(&id).await?;
    match action {
        FolderAction::Pause => state.monitor.pause(&id).await,
        FolderAction::Resume => state.monitor.resume(&id).await,
        FolderAction::Rescan => state.monitor.rescan(&id).await,
        FolderAction::Restart => {
            if folder.enabled {
                state.monitor.start(folder).await;
            }
        }
    }
    Ok(())
}

/// Opens the picker inside the folder and returns a subfolder to exclude.
/// The result is validated again when saved with `folders_update`.
#[tauri::command]
#[specta::specta]
pub async fn folders_pick_subfolder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<String>, UiError> {
    let folder = state.folders.get(&id).await?;
    let root = std::path::PathBuf::from(&folder.path);
    let Some(picked) = pick_folder(&app, "Choose a subfolder to exclude", Some(root.clone())).await
    else {
        return Ok(None);
    };
    let canon = crate::engine::paths::nfc(&picked.canonicalize().map_err(AppError::from)?);
    if canon == root || !canon.starts_with(&root) {
        return Err(AppError::validation(
            "excludedSubfolders",
            "Choose a folder inside this monitored folder.",
        )
        .into());
    }
    Ok(Some(canon.to_string_lossy().into_owned()))
}

#[tauri::command]
#[specta::specta]
pub async fn folders_summary(
    state: State<'_, AppState>,
    id: String,
) -> Result<FolderSummary, UiError> {
    state.folders.get(&id).await?;
    let rows = repo_files::folder_summary(&state.db, &id).await?;
    let by_category: Vec<CategoryUsage> = rows
        .into_iter()
        .map(|(category, files, bytes)| CategoryUsage {
            category,
            files: u32::try_from(files).unwrap_or(u32::MAX),
            bytes,
        })
        .collect();
    Ok(FolderSummary {
        files: by_category.iter().map(|c| c.files).sum(),
        bytes: by_category.iter().map(|c| c.bytes).sum(),
        by_category,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn folders_errors(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<WatcherErrorView>, UiError> {
    state.folders.get(&id).await?;
    Ok(repo_folders::recent_errors(&state.db, &id, 50)
        .await?
        .into_iter()
        .map(|(code, message, occurred_at)| WatcherErrorView {
            code,
            message,
            occurred_at,
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn folders_access(
    state: State<'_, AppState>,
    id: String,
) -> Result<FolderAccess, UiError> {
    let folder = state.folders.get(&id).await?;
    Ok(probe_access(std::path::Path::new(&folder.path)))
}

/// Opens System Settings → Privacy & Security → Files and Folders (FR-003).
#[tauri::command]
#[specta::specta]
pub async fn open_privacy_settings(app: AppHandle) -> Result<bool, UiError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(crate::system::open_privacy_settings());
    })
    .map_err(|e| AppError::Internal(format!("main thread: {e}")))?;
    Ok(rx.await.unwrap_or(false))
}
