//! Six-step onboarding workflow (`P5-T08`; `FR-001..009`, `R-ONB-01`).
//!
//! Scans user-selected folders and generates a dry-run preview using starter templates
//! without performing any filesystem writes (`FR-007`). Persists progress in `settings`
//! so onboarding can be resumed midway or re-run from Settings (`FR-009`).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::categories::CategoryService;
use crate::database::{repo_folders, repo_rules, repo_settings, Db};
use crate::engine::activity::load_path_authority;
use crate::engine::classifier::Classifier;
use crate::engine::fs_view::RealFs;
use crate::engine::paths::RootPolicy;
use crate::engine::planner::{
    build_execution_plan, PlanCandidate, PlanCategoryCount, PlanContext, PlanEnv, PlanItem,
    PlanScope, PlanSummary, PreviewStore,
};
use crate::engine::reconcile::FolderScanSpec;
use crate::engine::scanner::{scan, ScanOptions};
use crate::error::{AppError, AppResult};
use crate::rules::models::{Rule, RuleOrigin};
use crate::rules::presets::starter_templates;
use crate::rules::RuleService;
use crate::settings::SettingsService;
use crate::utils::clock::Clock;

pub const ONBOARDING_SETTINGS_KEY: &str = "onboarding";
pub const MAX_ONBOARDING_STEP: u32 = 6;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationPreference {
    ByType,
    ByDate,
    #[default]
    ByCategory,
    CustomRules,
    KeepOriginalStructure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    /// Current step index `1..=6` (`FR-009` resume support).
    pub current_step: u32,
    pub completed: bool,
    pub organization_preference: OrganizationPreference,
    pub selected_preset_ids: Vec<String>,
    pub automation_confirmed: bool,
    #[specta(type = Option<f64>)]
    pub completed_at: Option<i64>,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            current_step: 1,
            completed: false,
            organization_preference: OrganizationPreference::ByCategory,
            selected_preset_ids: vec!["pdfs".into(), "screenshots".into(), "invoices".into()],
            automation_confirmed: false,
            completed_at: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStepUpdate {
    pub current_step: u32,
    pub organization_preference: Option<OrganizationPreference>,
    pub selected_preset_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingScanReport {
    pub folders_scanned: u32,
    pub total_files: u32,
    pub total_folders: u32,
    #[specta(type = f64)]
    pub total_bytes: u64,
    pub by_category: Vec<PlanCategoryCount>,
    pub plan_summary: PlanSummary,
    pub sample_items: Vec<PlanItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingCompleteReport {
    pub state: OnboardingState,
    pub rules_created: u32,
    pub automation_enabled: bool,
}

#[derive(Clone)]
pub struct OnboardingService {
    db: Db,
    root_policy: RootPolicy,
    clock: Arc<dyn Clock>,
    preview_store: PreviewStore,
}

impl OnboardingService {
    pub fn new(
        db: Db,
        root_policy: RootPolicy,
        clock: Arc<dyn Clock>,
        preview_store: PreviewStore,
    ) -> Self {
        Self {
            db,
            root_policy,
            clock,
            preview_store,
        }
    }

    pub async fn get_state(&self) -> AppResult<OnboardingState> {
        let rows = repo_settings::load_all(&self.db).await?;
        let state = rows
            .get(ONBOARDING_SETTINGS_KEY)
            .and_then(|raw| serde_json::from_str::<OnboardingState>(raw).ok())
            .map(|mut s| {
                s.current_step = s.current_step.clamp(1, MAX_ONBOARDING_STEP);
                s
            })
            .unwrap_or_default();
        Ok(state)
    }

    async fn save_state(&self, state: &OnboardingState) -> AppResult<()> {
        let raw = serde_json::to_string(state)?;
        repo_settings::save_all(&self.db, &[(ONBOARDING_SETTINGS_KEY, raw)]).await
    }

    pub async fn set_step(&self, update: OnboardingStepUpdate) -> AppResult<OnboardingState> {
        let mut state = self.get_state().await?;
        state.current_step = update.current_step.clamp(1, MAX_ONBOARDING_STEP);
        if let Some(pref) = update.organization_preference {
            state.organization_preference = pref;
        }
        if let Some(presets) = update.selected_preset_ids {
            state.selected_preset_ids = presets;
        }
        self.save_state(&state).await?;
        Ok(state)
    }

    /// Scans registered monitored folders and builds a dry-run preview against
    /// selected starter presets with **zero** filesystem writes (`FR-006`, `FR-007`).
    pub async fn scan_and_preview(
        &self,
        selected_preset_ids: Option<Vec<String>>,
    ) -> AppResult<OnboardingScanReport> {
        let state = self.get_state().await?;
        let chosen_ids: HashSet<String> = selected_preset_ids
            .unwrap_or(state.selected_preset_ids)
            .into_iter()
            .collect();

        let categories = CategoryService::new(self.db.clone()).list().await?;
        let classifier = std::sync::Arc::new(Classifier::new(categories));
        let folders = repo_folders::list(&self.db).await?;
        let monitoring = SettingsService::new(self.db.clone())
            .get()
            .await?
            .monitoring;

        let cls_scan = classifier.clone();
        let (folders_scanned, total_files, total_folders, total_bytes, cat_totals, candidates) =
            tokio::task::spawn_blocking(move || {
                let mut folders_scanned: u32 = 0;
                let mut total_files: u32 = 0;
                let mut total_folders: u32 = 0;
                let mut total_bytes: u64 = 0;
                let mut cat_totals: BTreeMap<String, (u32, u64)> = BTreeMap::new();
                let mut candidates: Vec<PlanCandidate> = Vec::new();

                for folder in &folders {
                    let spec = FolderScanSpec::from_folder(folder, &monitoring);
                    let opts = ScanOptions {
                        recursive: spec.recursive,
                        rules: spec.rules,
                    };
                    if let Ok(summary) = scan(&spec.path, &opts, &cls_scan, |batch| {
                        for sf in batch {
                            let e = cat_totals.entry(sf.category.clone()).or_insert((0, 0));
                            e.0 = e.0.saturating_add(1);
                            e.1 = e.1.saturating_add(sf.facts.size);
                            candidates.push(PlanCandidate {
                                folder_id: folder.id.clone(),
                                folder_path: spec.path.clone(),
                                folder_name: folder.name.clone(),
                                path: sf.facts.path,
                                category_hint: Some(sf.category),
                                mime_hint: sf.mime,
                                content_hash: None,
                                is_duplicate: false,
                            });
                        }
                        Ok(())
                    }) {
                        folders_scanned = folders_scanned.saturating_add(1);
                        total_files = total_files
                            .saturating_add(u32::try_from(summary.files).unwrap_or(u32::MAX));
                        total_folders = total_folders
                            .saturating_add(u32::try_from(summary.folders).unwrap_or(u32::MAX));
                        total_bytes = total_bytes.saturating_add(summary.bytes);
                    }
                }
                (
                    folders_scanned,
                    total_files,
                    total_folders,
                    total_bytes,
                    cat_totals,
                    candidates,
                )
            })
            .await
            .map_err(|e| AppError::Internal(format!("onboarding scan failed: {e}")))?;

        let mut by_category: Vec<PlanCategoryCount> = cat_totals
            .into_iter()
            .map(|(category, (count, bytes))| PlanCategoryCount {
                category,
                count,
                bytes,
            })
            .collect();
        by_category.sort_by(|a, b| {
            b.bytes
                .cmp(&a.bytes)
                .then_with(|| a.category.cmp(&b.category))
        });

        let now_ms = self.clock.now_ms();
        let preview_rules: Vec<Rule> = starter_templates()
            .into_iter()
            .enumerate()
            .filter(|(_, t)| chosen_ids.contains(&t.id))
            .map(|(idx, t)| Rule {
                id: format!("preview-preset-{}", t.id),
                name: t.draft.name,
                description: t.draft.description,
                enabled: true, // Preview mode evaluates selected starters as active without mutating FS/DB.
                priority: i32::try_from(idx + 1).unwrap_or(1),
                source_folders: t.draft.source_folders,
                conditions: t.draft.conditions,
                actions: t.draft.actions,
                date_source: t.draft.date_source,
                execution_mode: t.draft.execution_mode,
                origin: RuleOrigin::Preset,
                last_run_at: None,
                stat_matched: 0,
                stat_succeeded: 0,
                stat_failed: 0,
                created_at: now_ms,
                updated_at: now_ms,
            })
            .collect();

        let authority = load_path_authority(&self.db).await?;
        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &authority,
                classifier: &classifier,
                clock: self.clock.as_ref(),
            },
            &preview_rules,
            &candidates,
            &PlanScope::default(),
            &PlanEnv::default(),
        );
        let sample_items: Vec<PlanItem> = plan.items.iter().take(100).cloned().collect();
        let plan_summary = self.preview_store.insert(plan);

        Ok(OnboardingScanReport {
            folders_scanned,
            total_files,
            total_folders,
            total_bytes,
            by_category,
            plan_summary,
            sample_items,
        })
    }

    /// Finalizes onboarding (`FR-008`), saving starter rules (disabled unless explicitly confirmed)
    /// and still performing **zero** filesystem writes (`FR-007`).
    pub async fn complete(
        &self,
        selected_preset_ids: Vec<String>,
        confirm_enable_automation: bool,
    ) -> AppResult<OnboardingCompleteReport> {
        let rules_svc = RuleService::new(self.db.clone(), self.root_policy.clone());
        let existing_names: HashSet<String> = rules_svc
            .list()
            .await?
            .into_iter()
            .map(|r| r.name.to_ascii_lowercase())
            .collect();

        let templates_by_id: BTreeMap<String, String> = starter_templates()
            .into_iter()
            .map(|t| (t.id, t.draft.name))
            .collect();

        let mut rules_created: u32 = 0;
        for preset_id in &selected_preset_ids {
            let Some(preset_name) = templates_by_id.get(preset_id) else {
                continue;
            };
            if existing_names.contains(&preset_name.to_ascii_lowercase()) {
                continue;
            }
            let created = rules_svc.create_from_preset(preset_id).await?;
            if confirm_enable_automation {
                let _ = rules_svc.set_enabled(&created.id, true).await?;
            }
            rules_created = rules_created.saturating_add(1);
        }

        // If user re-ran onboarding and confirmed automation for previously created starter rules too:
        if confirm_enable_automation {
            let wanted_names: HashSet<String> = selected_preset_ids
                .iter()
                .filter_map(|id| templates_by_id.get(id))
                .map(|n| n.to_ascii_lowercase())
                .collect();
            for existing in repo_rules::list(&self.db).await? {
                if !existing.enabled
                    && existing.origin == RuleOrigin::Preset
                    && wanted_names.contains(&existing.name.to_ascii_lowercase())
                {
                    let _ = repo_rules::set_enabled(&self.db, &existing.id, true).await?;
                }
            }
        }

        let mut state = self.get_state().await?;
        state.current_step = MAX_ONBOARDING_STEP;
        state.completed = true;
        state.selected_preset_ids = selected_preset_ids;
        state.automation_confirmed = confirm_enable_automation;
        state.completed_at = Some(self.clock.now_ms());
        self.save_state(&state).await?;

        Ok(OnboardingCompleteReport {
            state,
            rules_created,
            automation_enabled: confirm_enable_automation,
        })
    }

    pub async fn reset(&self) -> AppResult<OnboardingState> {
        let mut state = self.get_state().await?;
        state.completed = false;
        state.current_step = 1;
        state.automation_confirmed = false;
        self.save_state(&state).await?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::CategoryService;
    use crate::database::{self, repo_rules};
    use crate::folders::FolderService;
    use crate::test_support::{snapshot_tree, tempdir, write_file};
    use crate::utils::clock::FixedClock;

    #[tokio::test]
    async fn r_onb_01_onboarding_flow_performs_zero_filesystem_writes() {
        let (_d, root) = tempdir();
        let dl = root.join("Downloads");
        write_file(&dl.join("invoice_2026.pdf"), b"%PDF-1.7 invoice");
        write_file(&dl.join("Screenshot 2026-06-01.png"), b"\x89PNG\r\n\x1a\n");
        write_file(&dl.join("photo.jpg"), b"\xff\xd8\xff\xe0JFIF");
        let before = snapshot_tree(&root);

        let db = database::open_in_memory().await.expect("db");
        CategoryService::new(db.clone())
            .ensure_builtins()
            .await
            .expect("seed categories");

        let policy = RootPolicy {
            home: root.clone(),
            app_data_dir: root.join(".app_data"),
        };
        let folders = FolderService::new(db.clone(), policy.clone());
        folders.add(&dl, true).await.expect("add folder");

        let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
        let preview_store = PreviewStore::new();
        let onb = OnboardingService::new(db.clone(), policy, clock, preview_store.clone());

        // Step 1 -> 5 persistence & resume support (`FR-009`).
        let s1 = onb.get_state().await.expect("initial");
        assert!(!s1.completed);
        assert_eq!(s1.current_step, 1);

        let s3 = onb
            .set_step(OnboardingStepUpdate {
                current_step: 4,
                organization_preference: Some(OrganizationPreference::ByType),
                selected_preset_ids: Some(vec!["pdfs".into(), "screenshots".into()]),
            })
            .await
            .expect("set step");
        assert_eq!(s3.current_step, 4);
        assert_eq!(s3.organization_preference, OrganizationPreference::ByType);

        // Scan + dry-run preview (`FR-006`, `FR-007`).
        let scan_rep = onb
            .scan_and_preview(Some(vec!["pdfs".into(), "screenshots".into()]))
            .await
            .expect("scan_and_preview");
        assert_eq!(scan_rep.total_files, 3);
        assert!(scan_rep.plan_summary.files_matched >= 2);
        assert!(preview_store.get(&scan_rep.plan_summary.plan_id).is_some());

        // Step 6 completion without confirming automation -> rules created DISABLED (`FR-005`, `FR-008`).
        let comp_disabled = onb
            .complete(vec!["pdfs".into()], false)
            .await
            .expect("complete without automation confirm");
        assert!(comp_disabled.state.completed);
        assert!(!comp_disabled.automation_enabled);
        let rules = repo_rules::list(&db).await.expect("rules");
        assert_eq!(rules.len(), 1);
        assert!(
            !rules[0].enabled,
            "unconfirmed onboarding must leave rules disabled"
        );

        // Re-run onboarding (`FR-009`) and complete WITH explicit confirmation (`FR-008`).
        let reset = onb.reset().await.expect("reset");
        assert!(!reset.completed);
        let comp_enabled = onb
            .complete(vec!["pdfs".into(), "screenshots".into()], true)
            .await
            .expect("complete confirmed");
        assert!(comp_enabled.state.completed);
        assert!(comp_enabled.automation_enabled);

        let after = snapshot_tree(&root);
        assert_eq!(
            before, after,
            "entire onboarding flow must perform zero writes inside user folders"
        );
    }
}
