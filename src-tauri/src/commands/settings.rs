use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::error::{AppError, UiError};
use crate::settings::Settings;
use crate::system::LoginItemStatus;

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, UiError> {
    Ok(state.settings.get().await?)
}

/// Replaces all settings after validation. Returns the stored settings.
#[tauri::command]
#[specta::specta]
pub async fn settings_update(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, UiError> {
    settings.validate()?;
    let prev = state.settings.get().await?;
    if settings.general.launch_at_login != prev.general.launch_at_login {
        state
            .login_item
            .set_enabled(settings.general.launch_at_login)?;
    }
    let updated = state.settings.update(settings).await?;
    state.sync_settings(&updated);
    if updated.monitoring != prev.monitoring {
        for folder in state.folders.list().await? {
            if folder.enabled {
                state.monitor.start(folder).await;
            }
        }
    }
    if updated.general.show_menu_bar_icon != prev.general.show_menu_bar_icon {
        crate::system::tray::refresh_tray_async(app).await;
    }
    Ok(updated)
}

/// Returns current macOS `SMAppService` Launch at Login registration state (`FR-180`, `P7-T02`).
#[tauri::command]
#[specta::specta]
pub async fn login_item_status(state: State<'_, AppState>) -> Result<LoginItemStatus, UiError> {
    Ok(state.login_item.status()?)
}

/// Opens System Settings → General → Login Items (`FR-180`, `P7-T02`).
#[tauri::command]
#[specta::specta]
pub async fn open_login_items_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, UiError> {
    let login_item = state.login_item.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(login_item.open_login_items_settings());
    })
    .map_err(|e| AppError::Internal(format!("main thread: {e}")))?;
    Ok(rx.await.unwrap_or(false))
}
