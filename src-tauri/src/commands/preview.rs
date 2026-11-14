//! Dry-run execution preview & execution commands (`P5-T06`, `FR-080..087`).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::app_state::AppState;
use crate::categories::Category;
use crate::database::{repo_files, repo_folders, repo_rules, Db};
use crate::engine::activity::load_path_authority;
use crate::engine::classifier::{Classifier, HEADER_BYTES};
use crate::engine::conflicts::ConflictPolicy;
use crate::engine::fs_view::{EntryKind, ReadOnlyFs, RealFs};
use crate::engine::operations::{BatchTrigger, OpKind};
use crate::engine::paths::nfc;
use crate::engine::planner::{
    build_execution_plan, collect_folder_candidates, execute_plan, ExecutionPlan, PlanContext,
    PlanEnv, PlanExecutionReport, PlanItemFilter, PlanItemsPage, PlanScope, PlanSummary,
};
use crate::engine::scanner::ScannedFile;
use crate::error::{AppError, AppResult, UiError};
use crate::folders::MonitoredFolder;
use crate::settings::MonitoringSettings;
use crate::utils::clock::Clock;

pub(crate) async fn create_preview_plan(
    db: &Db,
    categories: Vec<Category>,
    monitoring: MonitoringSettings,
    clock: &dyn Clock,
    scope: &PlanScope,
) -> AppResult<ExecutionPlan> {
    let folders: Vec<MonitoredFolder> = repo_folders::list(db)
        .await?
        .into_iter()
        .filter(|folder| {
            if let Some(fid) = &scope.folder_id {
                &folder.id == fid
            } else {
                // Skip disabled folders for global unscoped previews (`BUG-024a`).
                folder.enabled
            }
        })
        .collect();
    let classifier = Arc::new(Classifier::new(categories));
    let all_rules = repo_rules::list(db).await?;
    let authority = load_path_authority(db).await?;

    let explicit_set: Option<HashSet<PathBuf>> = scope
        .file_paths
        .as_ref()
        .map(|paths| paths.iter().map(|p| nfc(std::path::Path::new(p))).collect());

    let cls_scan = classifier.clone();
    let candidates = tokio::task::spawn_blocking(move || {
        let mut candidates = Vec::new();
        for folder in &folders {
            let mut cands =
                collect_folder_candidates(folder, &monitoring, &cls_scan, explicit_set.as_ref());
            candidates.append(&mut cands);
        }
        candidates
    })
    .await
    .map_err(|e| AppError::Internal(format!("preview scan task failed: {e}")))?;

    Ok(build_execution_plan(
        &PlanContext {
            fs: &RealFs,
            authority: &authority,
            classifier: &classifier,
            clock,
        },
        &all_rules,
        &candidates,
        scope,
        &PlanEnv::default(),
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn plan_create(
    state: State<'_, AppState>,
    scope: PlanScope,
) -> Result<PlanSummary, UiError> {
    let categories = state.categories.list().await?;
    let monitoring = state.settings.get().await?.monitoring;
    let plan = create_preview_plan(
        &state.db,
        categories,
        monitoring,
        state.clock.as_ref(),
        &scope,
    )
    .await?;
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

    state
        .preview_store
        .prune_executed(&plan_id, selected_item_ids.as_deref(), &report);

    Ok(report)
}

#[tauri::command]
#[specta::specta]
pub async fn plan_discard(state: State<'_, AppState>, plan_id: String) -> Result<bool, UiError> {
    Ok(state.preview_store.discard(&plan_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::builtin_categories;
    use crate::database;
    use crate::engine::operations::Ledger;
    use crate::engine::organizer::Organizer;
    use crate::engine::planner::PreviewStore;
    use crate::engine::safe_fs::SafeFs;
    use crate::rules::models::{
        Action, Condition, ConditionNode, DateSource, ExecutionMode, Rule, RuleOrigin,
        TextFieldKind, TextOp,
    };
    use crate::test_support::{tempdir, write_file};
    use crate::utils::clock::FixedClock;

    #[tokio::test]
    async fn bug_024_plan_ignores_disabled_folders_and_prevents_double_execute() {
        let (_d1, root_on) = tempdir();
        let (_d2, root_off) = tempdir();
        let db = database::open_in_memory().await.expect("db");

        repo_folders::insert(&db, "f-on", "Enabled", &root_on.to_string_lossy(), true)
            .await
            .expect("insert on");
        repo_folders::insert(&db, "f-off", "Disabled", &root_off.to_string_lossy(), true)
            .await
            .expect("insert off");
        let mut off_folder = repo_folders::get(&db, "f-off")
            .await
            .expect("get off")
            .expect("exists");
        off_folder.enabled = false;
        repo_folders::update(&db, &off_folder)
            .await
            .expect("disable f-off");

        let copy_dst = root_on.join("Copied");
        let rule = Rule {
            id: "r-copy".into(),
            name: "Copy Docs".into(),
            description: None,
            enabled: true,
            priority: 1,
            source_folders: Vec::new(),
            conditions: ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::Contains,
                    value: "sample".into(),
                    case_sensitive: false,
                },
            },
            actions: vec![Action::Copy {
                destination: copy_dst.to_string_lossy().into_owned(),
            }],
            date_source: DateSource::Modified,
            execution_mode: ExecutionMode::FirstMatch,
            origin: RuleOrigin::User,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_000_000,
        };
        repo_rules::insert(&db, &rule).await.expect("insert rule");

        write_file(&root_on.join("sample_on.txt"), b"enabled");
        write_file(&root_off.join("sample_off.txt"), b"disabled");

        let clock = FixedClock::utc(1_700_000_000_000);
        let plan = create_preview_plan(
            &db,
            builtin_categories(),
            MonitoringSettings::default(),
            &clock,
            &PlanScope::default(),
        )
        .await
        .expect("create plan");

        // Disabled folder files must not enter an unscoped "All folders" plan (`BUG-024a`).
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].folder_id, "f-on");
        assert_eq!(plan.items[0].original_name, "sample_on.txt");

        let store = PreviewStore::new();
        let summary = store.insert(plan.clone());
        let selected = vec![plan.items[0].id.clone()];
        let ledger = Ledger::new(db.clone());
        let org = Organizer::new(
            ledger,
            Arc::new(SafeFs::with_trash_dir(root_on.join(".Trash"))),
            Arc::new(FixedClock::utc(1_700_000_000_000)),
        );
        let authority = load_path_authority(&db).await.expect("auth");

        let first = execute_plan(
            &store.get(&summary.plan_id).expect("stored plan"),
            Some(&selected),
            ConflictPolicy::AutoRename,
            &org,
            &authority,
            BatchTrigger::Preview,
        )
        .await
        .expect("first exec");
        assert_eq!(first.completed.len(), 1);
        store.prune_executed(&summary.plan_id, Some(&selected), &first);

        // Second execution of the same item IDs must be a no-op (no duplicate `(1)` copies).
        let second = execute_plan(
            &store.get(&summary.plan_id).expect("pruned plan"),
            Some(&selected),
            ConflictPolicy::AutoRename,
            &org,
            &authority,
            BatchTrigger::Preview,
        )
        .await
        .expect("second exec");
        assert!(second.completed.is_empty());
        assert!(copy_dst.join("sample_on.txt").exists());
        assert!(!copy_dst.join("sample_on (1).txt").exists());
    }
}
