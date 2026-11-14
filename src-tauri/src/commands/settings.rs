use tauri::State;

use crate::app_state::AppState;
use crate::error::UiError;
use crate::settings::Settings;

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, UiError> {
    Ok(state.settings.get().await?)
}

/// Replaces all settings after validation. Returns the stored settings.
#[tauri::command]
#[specta::specta]
pub async fn settings_update(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, UiError> {
    let prev = state.settings.get().await?;
    let updated = state.settings.update(settings).await?;
    if updated.monitoring != prev.monitoring {
        state.monitor.set_settings(updated.monitoring.clone());
        for folder in state.folders.list().await? {
            if folder.enabled {
                state.monitor.start(folder).await;
            }
        }
    }
    Ok(updated)
}
