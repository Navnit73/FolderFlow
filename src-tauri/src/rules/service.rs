//! Rule persistence, validation, preset installation, and dry-run folder testing service (P3-T07, FR-057).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::database::{repo_categories, repo_files, repo_folders, repo_rules, Db};
use crate::engine::paths::{validate_new_root, RootPolicy};
use crate::error::{AppError, AppResult};
use crate::rules::evaluator::{evaluate_rules, EvaluatedAction};
use crate::rules::models::{ExecutionMode, Rule, RuleDraft, RuleOrigin};
use crate::rules::presets::find_starter_template;
use crate::rules::validate::{
    ensure_valid_rule, validate_rule_draft, MonitoredFolderRef, ValidationContext, ValidationIssue,
};
use crate::utils::clock::Clock;
use crate::utils::time::now_ms;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuleTestMatchItem {
    pub path: String,
    pub original_name: String,
    pub final_filename: String,
    pub final_directory: String,
    pub actions: Vec<EvaluatedAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuleTestReport {
    pub folder_id: String,
    pub folder_name: String,
    pub evaluated_files: u32,
    pub matched_files: u32,
    pub sample_matches: Vec<RuleTestMatchItem>,
}

#[derive(Clone)]
pub struct RuleService {
    db: Db,
    policy: RootPolicy,
}

impl RuleService {
    pub fn new(db: Db, policy: RootPolicy) -> Self {
        Self { db, policy }
    }

    pub async fn validation_context(
        &self,
        current_rule_id: Option<String>,
    ) -> AppResult<ValidationContext> {
        let folders = repo_folders::list(&self.db).await?;
        let dest_roots = repo_rules::list_destination_roots(&self.db).await?;
        let existing_rules = repo_rules::list(&self.db).await?;

        let mut approved_roots: Vec<PathBuf> =
            folders.iter().map(|f| PathBuf::from(&f.path)).collect();
        for dr in dest_roots {
            let p = PathBuf::from(dr);
            if !approved_roots.contains(&p) {
                approved_roots.push(p);
            }
        }

        let monitored_folders = folders
            .into_iter()
            .map(|f| MonitoredFolderRef {
                id: f.id,
                path: PathBuf::from(f.path),
                recursive: f.recursive,
                excluded_subfolders: f
                    .settings
                    .excluded_subfolders
                    .into_iter()
                    .map(PathBuf::from)
                    .collect(),
            })
            .collect();

        let mut rule_links = std::collections::HashMap::new();
        for r in existing_rules {
            let targets: Vec<String> = r
                .actions
                .into_iter()
                .filter_map(|a| match a {
                    crate::rules::models::Action::ApplyAnotherRule { rule_id } => {
                        Some(rule_id.trim().to_owned())
                    }
                    _ => None,
                })
                .collect();
            rule_links.insert(r.id, targets);
        }

        Ok(ValidationContext {
            current_rule_id,
            monitored_folders,
            approved_roots,
            rule_links,
        })
    }

    pub async fn validate(
        &self,
        draft: &RuleDraft,
        current_rule_id: Option<String>,
    ) -> AppResult<Vec<ValidationIssue>> {
        let ctx = self.validation_context(current_rule_id).await?;
        Ok(validate_rule_draft(draft, &ctx))
    }

    pub async fn list(&self) -> AppResult<Vec<Rule>> {
        repo_rules::list(&self.db).await
    }

    pub async fn get(&self, id: &str) -> AppResult<Rule> {
        repo_rules::get(&self.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Rule".into()))
    }

    pub async fn create(&self, draft: RuleDraft, origin: RuleOrigin) -> AppResult<Rule> {
        let ctx = self.validation_context(None).await?;
        ensure_valid_rule(&draft, &ctx)?;

        let now = now_ms();
        let priority = repo_rules::next_priority(&self.db).await?;
        let enabled = match origin {
            RuleOrigin::User => draft.enabled,
            RuleOrigin::Preset | RuleOrigin::Import => false,
        };
        let rule = Rule {
            id: Uuid::now_v7().to_string(),
            name: draft.name.trim().to_owned(),
            description: draft
                .description
                .map(|d| d.trim().to_owned())
                .filter(|d| !d.is_empty()),
            enabled,
            priority,
            source_folders: draft.source_folders,
            conditions: draft.conditions,
            actions: draft.actions,
            date_source: draft.date_source,
            execution_mode: draft.execution_mode,
            origin,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: now,
            updated_at: now,
        };
        repo_rules::insert(&self.db, &rule).await?;
        Ok(rule)
    }

    pub async fn create_from_preset(&self, preset_id: &str) -> AppResult<Rule> {
        let tpl = find_starter_template(preset_id)
            .ok_or_else(|| AppError::NotFound("Rule preset".into()))?;
        self.create(tpl.draft, RuleOrigin::Preset).await
    }

    pub async fn smart_catalog(&self) -> AppResult<crate::rules::SmartOrganizerCatalog> {
        let cats = repo_categories::list(&self.db).await?;
        Ok(crate::rules::build_smart_catalog(&cats))
    }

    pub async fn create_smart_preset(
        &self,
        spec: crate::rules::SmartOrganizerSpec,
    ) -> AppResult<Rule> {
        let cats = repo_categories::list(&self.db).await?;
        let draft = crate::rules::build_smart_draft(spec, &cats)?;
        self.create(draft, RuleOrigin::Preset).await
    }

    pub async fn update(&self, id: &str, draft: RuleDraft) -> AppResult<Rule> {
        let existing = self.get(id).await?;
        let ctx = self.validation_context(Some(id.to_owned())).await?;
        ensure_valid_rule(&draft, &ctx)?;

        let updated = Rule {
            id: existing.id,
            name: draft.name.trim().to_owned(),
            description: draft
                .description
                .map(|d| d.trim().to_owned())
                .filter(|d| !d.is_empty()),
            enabled: draft.enabled,
            priority: existing.priority,
            source_folders: draft.source_folders,
            conditions: draft.conditions,
            actions: draft.actions,
            date_source: draft.date_source,
            execution_mode: draft.execution_mode,
            origin: existing.origin,
            last_run_at: existing.last_run_at,
            stat_matched: existing.stat_matched,
            stat_succeeded: existing.stat_succeeded,
            stat_failed: existing.stat_failed,
            created_at: existing.created_at,
            updated_at: now_ms(),
        };
        repo_rules::update(&self.db, &updated).await?;
        Ok(updated)
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        if !repo_rules::delete(&self.db, id).await? {
            return Err(AppError::NotFound("Rule".into()));
        }
        Ok(())
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<Rule> {
        if !repo_rules::set_enabled(&self.db, id, enabled).await? {
            return Err(AppError::NotFound("Rule".into()));
        }
        self.get(id).await
    }

    pub async fn reorder(&self, ordered_ids: Vec<String>) -> AppResult<Vec<Rule>> {
        repo_rules::reorder(&self.db, &ordered_ids).await?;
        self.list().await
    }

    pub async fn duplicate(&self, id: &str) -> AppResult<Rule> {
        let existing = self.get(id).await?;
        let base_trimmed: String = existing.name.trim().chars().take(115).collect();
        let copy_name = format!("{base_trimmed} Copy");
        let draft = RuleDraft {
            name: copy_name,
            description: existing.description,
            enabled: false,
            source_folders: existing.source_folders,
            conditions: existing.conditions,
            actions: existing.actions,
            date_source: existing.date_source,
            execution_mode: existing.execution_mode,
        };
        self.create(draft, RuleOrigin::User).await
    }

    pub async fn record_stats(
        &self,
        id: &str,
        matched_delta: i64,
        succeeded_delta: i64,
        failed_delta: i64,
        ran_at_ms: i64,
    ) -> AppResult<Rule> {
        if !repo_rules::record_stats(
            &self.db,
            id,
            matched_delta,
            succeeded_delta,
            failed_delta,
            ran_at_ms,
        )
        .await?
        {
            return Err(AppError::NotFound("Rule".into()));
        }
        self.get(id).await
    }

    pub async fn add_destination_root(&self, raw_path: &Path) -> AppResult<String> {
        let canonical = validate_new_root(raw_path, &self.policy)?;
        let path_str = canonical.to_string_lossy().into_owned();
        repo_rules::upsert_destination_root(&self.db, &Uuid::now_v7().to_string(), &path_str)
            .await?;
        Ok(path_str)
    }

    pub async fn test_draft_on_folder(
        &self,
        draft: &RuleDraft,
        folder_id: &str,
        clock: &dyn Clock,
    ) -> AppResult<RuleTestReport> {
        let folder = repo_folders::get(&self.db, folder_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Monitored folder".into()))?;
        let files =
            repo_files::list_facts_for_folder(&self.db, &folder.id, &folder.path, &folder.name)
                .await?;

        let temp_rule = Rule {
            id: "draft-preview".into(),
            name: if draft.name.trim().is_empty() {
                "Draft rule".into()
            } else {
                draft.name.trim().to_owned()
            },
            description: draft.description.clone(),
            enabled: true,
            priority: 1,
            source_folders: Vec::new(),
            conditions: draft.conditions.clone(),
            actions: draft.actions.clone(),
            date_source: draft.date_source,
            execution_mode: ExecutionMode::FirstMatch,
            origin: RuleOrigin::User,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: 0,
            updated_at: 0,
        };

        let mut eval_pool: Vec<Rule> = repo_rules::list(&self.db)
            .await?
            .into_iter()
            .map(|mut r| {
                r.enabled = false;
                r
            })
            .collect();
        eval_pool.push(temp_rule);

        let evaluated_files = u32::try_from(files.len()).unwrap_or(u32::MAX);
        let mut matched_files: u32 = 0;
        let mut sample_matches = Vec::new();

        for file in &files {
            let outcome = evaluate_rules(&eval_pool, file, clock);
            if !outcome.matched_rules.is_empty() && !outcome.skipped {
                matched_files = matched_files.saturating_add(1);
                if sample_matches.len() < 100 {
                    sample_matches.push(RuleTestMatchItem {
                        path: file.path.clone(),
                        original_name: file.name.clone(),
                        final_filename: outcome.final_filename,
                        final_directory: outcome.final_directory,
                        actions: outcome.actions,
                    });
                }
            }
        }

        Ok(RuleTestReport {
            folder_id: folder.id,
            folder_name: folder.name,
            evaluated_files,
            matched_files,
            sample_matches,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::open_in_memory;
    use crate::rules::models::{Action, Condition, ConditionNode, DateSource, ExecutionMode};
    use crate::test_support::tempdir;

    async fn setup() -> (RuleService, Db) {
        let db = open_in_memory().await.expect("in-memory db");
        let (_d, base) = tempdir();
        let policy = RootPolicy {
            home: base.join("home"),
            app_data_dir: base.join("appdata"),
        };
        (RuleService::new(db.clone(), policy), db)
    }

    fn draft(name: &str) -> RuleDraft {
        RuleDraft {
            name: name.into(),
            description: Some(format!("Description for {name}")),
            enabled: false,
            source_folders: Vec::new(),
            conditions: ConditionNode::All {
                children: vec![ConditionNode::Leaf {
                    condition: Condition::ExtensionGroup {
                        extensions: vec!["pdf".into()],
                    },
                }],
            },
            actions: vec![Action::MoveIntoDynamicFolder {
                root: None,
                subpath_template: "PDFs/{year}".into(),
            }],
            date_source: DateSource::Modified,
            execution_mode: ExecutionMode::FirstMatch,
        }
    }

    #[tokio::test]
    async fn r_rule_04_crud_and_duplicate_and_preset_installation() {
        let (svc, _db) = setup().await;

        let r1 = svc
            .create(draft("PDF Organizer"), RuleOrigin::User)
            .await
            .expect("create r1");
        assert_eq!(r1.name, "PDF Organizer");
        assert_eq!(r1.priority, 1);
        assert!(!r1.enabled);

        let fetched = svc.get(&r1.id).await.expect("get r1");
        assert_eq!(fetched, r1);

        let mut updated_draft = draft("PDF Organizer Renamed");
        updated_draft.enabled = true;
        let updated = svc.update(&r1.id, updated_draft).await.expect("update");
        assert_eq!(updated.name, "PDF Organizer Renamed");
        assert!(updated.enabled);
        assert_eq!(updated.priority, 1);

        let dup = svc.duplicate(&r1.id).await.expect("duplicate");
        assert_eq!(dup.name, "PDF Organizer Renamed Copy");
        assert!(!dup.enabled, "duplicated rule must start disabled");
        assert_eq!(dup.priority, 2);

        let preset = svc.create_from_preset("invoices").await.expect("preset");
        assert_eq!(preset.origin, RuleOrigin::Preset);
        assert!(!preset.enabled);
        assert_eq!(preset.priority, 3);

        let all = svc.list().await.expect("list");
        assert_eq!(all.len(), 3);

        svc.delete(&dup.id).await.expect("delete");
        assert!(svc.get(&dup.id).await.is_err());
    }

    #[tokio::test]
    async fn r_rule_05_priority_reorder_set_enabled_and_validation_on_save() {
        let (svc, _db) = setup().await;

        let a = svc
            .create(draft("Rule A"), RuleOrigin::User)
            .await
            .expect("a");
        let b = svc
            .create(draft("Rule B"), RuleOrigin::User)
            .await
            .expect("b");
        let c = svc
            .create(draft("Rule C"), RuleOrigin::User)
            .await
            .expect("c");

        let reordered = svc
            .reorder(vec![c.id.clone(), a.id.clone(), b.id.clone()])
            .await
            .expect("reorder");
        assert_eq!(reordered[0].id, c.id);
        assert_eq!(reordered[0].priority, 1);
        assert_eq!(reordered[1].id, a.id);
        assert_eq!(reordered[1].priority, 2);
        assert_eq!(reordered[2].id, b.id);
        assert_eq!(reordered[2].priority, 3);

        let toggled = svc.set_enabled(&c.id, true).await.expect("set_enabled");
        assert!(toggled.enabled);

        // Invalid draft (empty name) must be rejected on create and update
        let mut bad = draft("");
        bad.name = "   ".into();
        assert!(svc.create(bad.clone(), RuleOrigin::User).await.is_err());
        assert!(svc.update(&a.id, bad).await.is_err());
    }

    #[tokio::test]
    async fn r_rule_06_stats_columns_persist_and_increment() {
        let (svc, _db) = setup().await;
        let r = svc
            .create(draft("Stats Rule"), RuleOrigin::User)
            .await
            .expect("create");
        assert_eq!(r.stat_matched, 0);
        assert_eq!(r.stat_succeeded, 0);
        assert_eq!(r.stat_failed, 0);
        assert_eq!(r.last_run_at, None);

        let after1 = svc
            .record_stats(&r.id, 10, 8, 2, 1_772_942_400_000)
            .await
            .expect("stats 1");
        assert_eq!(after1.stat_matched, 10);
        assert_eq!(after1.stat_succeeded, 8);
        assert_eq!(after1.stat_failed, 2);
        assert_eq!(after1.last_run_at, Some(1_772_942_400_000));

        let after2 = svc
            .record_stats(&r.id, 5, 5, 0, 1_772_950_000_000)
            .await
            .expect("stats 2");
        assert_eq!(after2.stat_matched, 15);
        assert_eq!(after2.stat_succeeded, 13);
        assert_eq!(after2.stat_failed, 2);
        assert_eq!(after2.last_run_at, Some(1_772_950_000_000));
    }

    #[tokio::test]
    async fn bug_014_duplicate_multibyte_utf8_name_does_not_panic_and_keeps_copy_suffix() {
        let (svc, _db) = setup().await;
        // 59 'é' characters = 59 chars, 118 bytes; adding 'é' at 60 = 120 bytes, plus " Copy" = 125 bytes.
        // Byte index 120 falls inside a 2-byte UTF-8 sequence if we use 59 'a' + 31 'é' (121 bytes, 90 chars).
        let name = format!("{}{}", "a".repeat(59), "é".repeat(31));
        assert_eq!(name.chars().count(), 90);
        assert_eq!(name.len(), 121);
        let r = svc
            .create(draft(&name), RuleOrigin::User)
            .await
            .expect("create multibyte rule name");
        let dup = svc
            .duplicate(&r.id)
            .await
            .expect("duplicate multibyte rule");
        assert!(dup.name.ends_with(" Copy"));
        assert!(dup.name.chars().count() <= 120);
    }

    #[tokio::test]
    async fn bug_016_multihop_apply_another_rule_cycle_rejected_at_save_and_resolved_in_folder_test(
    ) {
        let (svc, _db) = setup().await;
        let a = svc
            .create(draft("Rule A"), RuleOrigin::User)
            .await
            .expect("create A");
        let mut b_draft = draft("Rule B");
        b_draft.actions = vec![Action::ApplyAnotherRule {
            rule_id: a.id.clone(),
        }];
        let b = svc
            .create(b_draft, RuleOrigin::User)
            .await
            .expect("create B -> A");

        // Updating Rule A to apply Rule B creates a 2-hop cycle (A -> B -> A) and must be rejected!
        let mut a_update = draft("Rule A");
        a_update.actions = vec![Action::ApplyAnotherRule {
            rule_id: b.id.clone(),
        }];
        assert!(
            svc.update(&a.id, a_update).await.is_err(),
            "expected multi-hop cycle A -> B -> A to be rejected at save time"
        );
    }
}
