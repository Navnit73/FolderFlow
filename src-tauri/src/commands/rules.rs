//! Rule management, validation, preset, template preview, and dry-run test commands (P3-T07).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::AppState;
use crate::error::UiError;
use crate::rules::{
    render_filename_template, render_subpath_template, starter_templates, DateSource, Rule,
    RuleDraft, RuleOrigin, RuleTemplate, RuleTestReport, SampleFile, ValidationIssue,
};
use crate::utils::clock::{Clock, SystemClock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePreviewInput {
    pub template: String,
    #[serde(default)]
    pub is_filename: bool,
    #[serde(default)]
    pub date_source: DateSource,
    pub sample: SampleFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePreviewOutput {
    pub valid: bool,
    pub rendered: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn rules_list(state: State<'_, AppState>) -> Result<Vec<Rule>, UiError> {
    Ok(state.rules.list().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_get(state: State<'_, AppState>, id: String) -> Result<Rule, UiError> {
    Ok(state.rules.get(&id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_create(state: State<'_, AppState>, draft: RuleDraft) -> Result<Rule, UiError> {
    Ok(state.rules.create(draft, RuleOrigin::User).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_create_from_preset(
    state: State<'_, AppState>,
    preset_id: String,
) -> Result<Rule, UiError> {
    Ok(state.rules.create_from_preset(&preset_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_update(
    state: State<'_, AppState>,
    id: String,
    draft: RuleDraft,
) -> Result<Rule, UiError> {
    Ok(state.rules.update(&id, draft).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_delete(state: State<'_, AppState>, id: String) -> Result<(), UiError> {
    Ok(state.rules.delete(&id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_reorder(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<Vec<Rule>, UiError> {
    Ok(state.rules.reorder(ordered_ids).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<Rule, UiError> {
    Ok(state.rules.set_enabled(&id, enabled).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_duplicate(state: State<'_, AppState>, id: String) -> Result<Rule, UiError> {
    Ok(state.rules.duplicate(&id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_validate(
    state: State<'_, AppState>,
    draft: RuleDraft,
    current_rule_id: Option<String>,
) -> Result<Vec<ValidationIssue>, UiError> {
    Ok(state.rules.validate(&draft, current_rule_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rules_presets() -> Result<Vec<RuleTemplate>, UiError> {
    Ok(starter_templates())
}

#[tauri::command]
#[specta::specta]
pub async fn template_preview(
    input: TemplatePreviewInput,
) -> Result<TemplatePreviewOutput, UiError> {
    let clock = SystemClock;
    let facts = input.sample.to_facts(clock.now_ms());
    let res = if input.is_filename {
        render_filename_template(&input.template, &facts, input.date_source, &clock)
    } else {
        render_subpath_template(&input.template, &facts, input.date_source, &clock)
    };
    Ok(match res {
        Ok(rendered) => TemplatePreviewOutput {
            valid: true,
            rendered: Some(rendered),
            error: None,
        },
        Err(e) => TemplatePreviewOutput {
            valid: false,
            rendered: None,
            error: Some(e.to_string()),
        },
    })
}

#[tauri::command]
#[specta::specta]
pub async fn rules_test_folder(
    state: State<'_, AppState>,
    draft: RuleDraft,
    folder_id: String,
) -> Result<RuleTestReport, UiError> {
    let clock = SystemClock;
    Ok(state
        .rules
        .test_draft_on_folder(&draft, &folder_id, &clock)
        .await?)
}

/// Opens the native folder picker to select and approve a destination folder.
#[tauri::command]
#[specta::specta]
pub async fn rules_pick_destination_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, UiError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose a destination folder for this rule")
        .set_can_create_directories(true);
    if let Ok(home) = app.path().home_dir() {
        dialog = dialog.set_directory(home);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
    });
    let Some(path) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let approved = state.rules.add_destination_root(&path).await?;
    state.refresh_authority().await?;
    Ok(Some(approved))
}
