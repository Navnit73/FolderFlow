//! Onboarding commands (`P5-T08`, `FR-001..009`).

use tauri::State;

use crate::app_state::AppState;
use crate::error::UiError;
use crate::onboarding::{
    OnboardingCompleteReport, OnboardingScanReport, OnboardingState, OnboardingStepUpdate,
};

#[tauri::command]
#[specta::specta]
pub async fn onboarding_state(state: State<'_, AppState>) -> Result<OnboardingState, UiError> {
    Ok(state.onboarding.get_state().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn onboarding_set_step(
    state: State<'_, AppState>,
    update: OnboardingStepUpdate,
) -> Result<OnboardingState, UiError> {
    Ok(state.onboarding.set_step(update).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn onboarding_scan_preview(
    state: State<'_, AppState>,
    selected_preset_ids: Option<Vec<String>>,
) -> Result<OnboardingScanReport, UiError> {
    Ok(state
        .onboarding
        .scan_and_preview(selected_preset_ids)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn onboarding_complete(
    state: State<'_, AppState>,
    selected_preset_ids: Vec<String>,
    confirm_enable_automation: bool,
) -> Result<OnboardingCompleteReport, UiError> {
    let report = state
        .onboarding
        .complete(selected_preset_ids, confirm_enable_automation)
        .await?;
    state.refresh_authority().await?;
    Ok(report)
}

#[tauri::command]
#[specta::specta]
pub async fn onboarding_reset(state: State<'_, AppState>) -> Result<OnboardingState, UiError> {
    Ok(state.onboarding.reset().await?)
}
