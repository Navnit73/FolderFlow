//! Dry-run execution preview & execution commands (`P5-T06`, `FR-080..087`).

use std::collections::HashSet;
use std::path::PathBuf;

use tauri::State;

use crate::app_state::AppState;
use crate::database::{repo_files, repo_folders, repo_rules};
use crate::engine::activity::load_path_authority;
use crate::engine::classifier::{Classifier, HEADER_BYTES};
use crate::engine::conflicts::ConflictPolicy;
use crate::engine::fs_view::{EntryKind, ReadOnlyFs, RealFs};
use crate::engine::operations::{BatchTrigger, OpKind};
use crate::engine::paths::nfc;
use crate::engine::planner::{
    build_execution_plan, execute_plan, PlanCandidate, PlanEnv, PlanExecutionReport,
    PlanItemFilter, PlanItemsPage, PlanScope, PlanSummary,
};
use crate::engine::scanner::{scan, ScanOptions, ScannedFile};
use crate::error::{AppError, UiError};

#[tauri::command]
#[specta::specta]
pub async fn plan_create(
    state: State<'_, AppState>,
    scope: PlanScope,
) -> Result<PlanSummary, UiError> {
    let folders = repo_folders::list(&state.db).await?;
    let categories = state.categories.list().await?;
    let classifier = Classifier::new(categories);
    let all_rules = repo_rules::list(&state.db).await?;
    let authority = load_path_authority(&state.db).await?;

    let explicit_set: Option<HashSet<PathBuf>> = scope.file_paths.as_ref().map(|paths| {
        paths
            .iter()
            .map(|p| nfc(std::path::Path::new(p)))
            .collect()
    });

    let mut candidates: Vec<PlanCandidate> = Vec::new();
    for folder in folders {
        if let Some(fid) = &scope.folder_id {
            if &folder.id != fid {
                continue;
            }
        }
        let root = PathBuf::from(&folder.path);
        let opts = ScanOptions {
            recursive: folder.recursive,
            rules: crate::engine::ignore::IgnoreRules {
                ignore_hidden: true,
                ignore_temporary: true,
                include_extensions: folder.settings.include_extensions.clone(),
                exclude_extensions: folder.settings.exclude_extensions.clone(),
                excluded_dirs: folder
                    .settings
                    .excluded_subfolders
                    .iter()
                    .map(PathBuf::from)
                    .collect(),
            },
        };
        let _ = scan(&root, &opts, &classifier, |batch| {
            for sf in batch {
                if let Some(allow) = &explicit_set {
                    if !allow.contains(&nfc(&sf.facts.path)) {
                        continue;
                    }
                }
                candidates.push(PlanCandidate {
                    folder_id: folder.id.clone(),
                    folder_path: root.clone(),
                    folder_name: folder.name.clone(),
                    path: sf.facts.path,
                    category_hint: Some(sf.category),
                    mime_hint: sf.mime,
                    content_hash: None,
                    is_duplicate: false,
                });
            }
            Ok(())
        });
    }

    let plan = build_execution_plan(
        &RealFs,
        &authority,
        &classifier,
        state.clock.as_ref(),
        &all_rules,
        &candidates,
        &scope,
        &PlanEnv::default(),
    );
    Ok(state.preview_store.insert(plan))
}

#[tauri::command]
#[specta::specta]
pub async fn plan_items(
    state: State<'_, AppState>,
    plan_id: String,
    filter: PlanItemFilter,
    limit: u32,
    offset: u32,
) -> Result<PlanItemsPage, UiError> {
    Ok(state
        .preview_store
        .query_items(&plan_id, &filter, limit, offset)?)
}

#[tauri::command]
#[specta::specta]
pub async fn plan_execute(
    state: State<'_, AppState>,
    plan_id: String,
    selected_item_ids: Option<Vec<String>>,
    conflict_policy: ConflictPolicy,
) -> Result<PlanExecutionReport, UiError> {
    let plan = state
        .preview_store
        .get(&plan_id)
        .ok_or_else(|| AppError::NotFound(format!("Plan {plan_id}")))?;
    let authority = load_path_authority(&state.db).await?;
    let report = execute_plan(
        &plan,
        selected_item_ids.as_deref(),
        conflict_policy,
        state.activity.organizer(),
        &authority,
        BatchTrigger::Preview,
    )
    .await?;

    // Sync `file_records` after successful moves/renames inside monitored folders.
    let folders = repo_folders::list(&state.db).await?;
    let classifier = Classifier::new(state.categories.list().await?);
    let now_ms = state.clock.now_ms();
    for op in &report.completed {
        if matches!(op.operation_type, OpKind::Move | OpKind::Rename) {
            let _ = repo_files::delete_path(&state.db, &op.source_path).await?;
        }
        let dst_path = PathBuf::from(&op.destination_path);
        let dst_nfc = nfc(&dst_path);
        if let Some(owner) = folders
            .iter()
            .find(|f| dst_nfc.starts_with(nfc(std::path::Path::new(&f.path))))
        {
            if let Ok(meta) = RealFs.stat(&dst_path) {
                let is_pkg = meta.kind == EntryKind::Package;
                let header = if is_pkg {
                    None
                } else {
                    RealFs.read_header(&dst_path, HEADER_BYTES).ok()
                };
                let cls = classifier.classify(&meta.name, header.as_deref(), is_pkg);
                let new_sf = ScannedFile {
                    facts: meta,
                    category: cls.category,
                    mime: cls.mime,
                    confidence: cls.confidence,
                };
                let _ = repo_files::upsert_batch(
                    &state.db,
                    &owner.id,
                    std::slice::from_ref(&new_sf),
                    now_ms,
                )
                .await?;
            }
        }
    }

    Ok(report)
}

#[tauri::command]
#[specta::specta]
pub async fn plan_discard(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<bool, UiError> {
    Ok(state.preview_store.discard(&plan_id))
}

