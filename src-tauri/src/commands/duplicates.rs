//! Duplicate finder, review actions, CSV/JSON export, and Finder reveal commands (`P6-T08`).

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::app_state::AppState;
use crate::commands::rules::pick_and_approve_destination_folder;
use crate::database::repo_duplicates::{self, DuplicateGroupSummary, DuplicateGroupsResponse};
use crate::duplicates::actions::{
    apply_dup_action, export_duplicates, resolve_reveal_duplicate_path, DupAction, DupActionReport,
    DupExportFormat,
};
use crate::duplicates::finder::{scan_duplicates, DuplicateScanOptions, DuplicateScanReport};
use crate::engine::classifier::Classifier;
use crate::error::{AppError, UiError};
use crate::events::DupProgressEvent;
use crate::notifications::NotificationInput;
use crate::system;

pub(crate) fn dup_notification_counts(report: &DuplicateScanReport) -> Option<(u32, u32)> {
    let active = report.groups.totals.active_groups;
    if active == 0 {
        return None;
    }
    Some((active, report.groups.totals.total_duplicate_copies))
}

#[tauri::command]
#[specta::specta]
pub async fn dup_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    opts: DuplicateScanOptions,
) -> Result<DuplicateScanReport, UiError> {
    let cats = state.categories.list().await?;
    let classifier = Arc::new(Classifier::new(cats));
    let monitoring = state.settings.get().await?.monitoring;
    let progress_app = app.clone();
    let progress = Arc::new(move |hashed: u32, total: u32, bytes: u64| {
        let _ = DupProgressEvent {
            hashed,
            total,
            bytes,
        }
        .emit(&progress_app);
    });

    let report = scan_duplicates(
        &state.db,
        classifier,
        &monitoring,
        opts,
        state.clock.now_ms(),
        Some(progress),
    )
    .await?;

    if let Some((groups_found, extra_copies)) = dup_notification_counts(&report) {
        state
            .notifications
            .notify(NotificationInput::duplicates_found(
                groups_found,
                extra_copies,
            ));
    }
    Ok(report)
}

#[tauri::command]
#[specta::specta]
pub async fn dup_groups(
    state: State<'_, AppState>,
    include_ignored: bool,
    search: Option<String>,
) -> Result<DuplicateGroupsResponse, UiError> {
    Ok(repo_duplicates::list_groups(&state.db, include_ignored, search.as_deref()).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn dup_group_details(
    state: State<'_, AppState>,
    group_id: String,
) -> Result<DuplicateGroupSummary, UiError> {
    repo_duplicates::get_group_details(&state.db, &group_id)
        .await?
        .ok_or_else(|| UiError::from(AppError::NotFound("Duplicate group".into())))
}

#[tauri::command]
#[specta::specta]
pub async fn dup_action(
    state: State<'_, AppState>,
    action: DupAction,
) -> Result<DupActionReport, UiError> {
    let auth = state.activity.authority().await?;
    let should_refresh = matches!(action, DupAction::MoveToReview { .. });
    let report = apply_dup_action(state.activity.organizer(), &auth, action).await?;
    if should_refresh {
        state.monitor.refresh_exclusions().await;
    }
    Ok(report)
}

#[tauri::command]
#[specta::specta]
pub async fn dup_export(
    state: State<'_, AppState>,
    format: DupExportFormat,
    include_ignored: bool,
) -> Result<String, UiError> {
    Ok(export_duplicates(&state.db, format, include_ignored).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn dup_reveal_file(
    state: State<'_, AppState>,
    file_record_id: String,
) -> Result<bool, UiError> {
    let auth = state.activity.authority().await?;
    let path = resolve_reveal_duplicate_path(&state.db, &auth, &file_record_id).await?;
    Ok(system::reveal_in_finder(&path))
}

#[tauri::command]
#[specta::specta]
pub async fn dup_pick_review_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, UiError> {
    pick_and_approve_destination_folder(app, state, "Choose a review folder for duplicate files")
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repo_duplicates::{DuplicateGroupsResponse, DuplicateSummaryTotals};

    #[test]
    fn bug_028_dup_scan_notification_counts_extra_copies_and_skips_ignored_only() {
        let ignored_only = DuplicateScanReport {
            files_scanned: 2,
            unique_size_skipped: 0,
            partial_hashed: 2,
            full_hashed: 2,
            cache_hits: 0,
            groups_found: 0,
            total_wasted_bytes: 0,
            groups: DuplicateGroupsResponse {
                groups: vec![],
                totals: DuplicateSummaryTotals {
                    total_groups: 1,
                    active_groups: 0,
                    ignored_groups: 1,
                    total_wasted_bytes: 0,
                    total_duplicate_copies: 0,
                },
            },
        };
        assert_eq!(dup_notification_counts(&ignored_only), None);

        let active = DuplicateScanReport {
            groups_found: 2,
            total_wasted_bytes: 1024,
            groups: DuplicateGroupsResponse {
                groups: vec![],
                totals: DuplicateSummaryTotals {
                    total_groups: 3,
                    active_groups: 2,
                    ignored_groups: 1,
                    total_wasted_bytes: 1024,
                    total_duplicate_copies: 5,
                },
            },
            ..ignored_only
        };
        assert_eq!(dup_notification_counts(&active), Some((2, 5)));
    }
}
