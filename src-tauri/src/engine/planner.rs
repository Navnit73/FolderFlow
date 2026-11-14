//! Dry-run execution planner and plan executor with per-item revalidation
//! (`P5-T01`, `P5-T02`, `P5-T03`; `FR-080..087`, `NFR-004`, `SAFETY_MODEL.md` S5–S6).
//!
//! Planning receives only `&dyn ReadOnlyFs` (`fs_view.rs`), guaranteeing at compile time
//! that dry-run preview code cannot mutate the filesystem.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::database::repo_operations::{self, OperationView, ReviewProposedAction};
use crate::database::repo_rules;
use crate::engine::classifier::{Classifier, HEADER_BYTES};
use crate::engine::conflicts::ConflictPolicy;
use crate::engine::fs_view::{EntryKind, ReadOnlyFs};
use crate::engine::operations::{BatchTrigger, OpKind};
use crate::engine::organizer::{OrganizeOutcome, OrganizeRequest, Organizer};
use crate::engine::paths::{nfc, PathAuthority};
use crate::engine::reconcile::FolderScanSpec;
use crate::engine::safe_fs::EntryIdentity;
use crate::engine::scanner::{scan, ScanOptions};
use crate::error::{AppError, AppResult};
use crate::folders::MonitoredFolder;
use crate::rules::evaluator::{evaluate_rules, EvalWarning, PlannedOp};
use crate::rules::models::{FileFacts, Rule};
use crate::settings::MonitoringSettings;
use crate::utils::clock::Clock;

pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// Scope of a dry-run preview request (`FR-081`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanScope {
    pub folder_id: Option<String>,
    pub rule_id: Option<String>,
    pub file_paths: Option<Vec<String>>,
}

/// Primary operation kind for a planned item (`FR-080`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PlanOpKind {
    Move,
    Copy,
    Rename,
    MarkForReview,
}

/// Snapshot of a file's physical identity at plan creation time (`FR-085`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileIdentitySnapshot {
    #[specta(type = f64)]
    pub dev: u64,
    #[specta(type = f64)]
    pub ino: u64,
    #[specta(type = f64)]
    pub size: u64,
    #[specta(type = f64)]
    pub mtime_ns: i64,
}

/// Conflict category detected during dry-run planning (`FR-082`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanConflictKind {
    DestinationExistsOnDisk,
    DuplicateInPlan { colliding_item_ids: Vec<String> },
    DestinationUnauthorized,
    PermissionDenied,
    DestinationUnavailable,
    InsufficientDiskSpace,
    ConflictingRules,
}

/// A single conflict or warning attached to a [`PlanItem`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanConflict {
    pub kind: PlanConflictKind,
    pub message: String,
}

/// Status badge of a planned item (`FR-080`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PlanItemStatus {
    Ready,
    Conflict,
    NeedsReview,
    Blocked,
}

/// One file item inside an [`ExecutionPlan`] (`FR-080`, `FR-085`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub id: String,
    pub folder_id: String,
    pub source_path: String,
    pub current_location: String,
    pub original_name: String,
    pub proposed_destination: String,
    pub proposed_name: String,
    #[specta(type = f64)]
    pub size: u64,
    pub category: String,
    pub op: PlanOpKind,
    pub rule_id: String,
    pub rule_name: String,
    pub matched_conditions: Vec<String>,
    pub planned_ops: Vec<PlannedOp>,
    pub conflicts: Vec<PlanConflict>,
    pub status: PlanItemStatus,
    pub cross_volume: bool,
    pub identity: FileIdentitySnapshot,
}

impl PlanItem {
    pub fn has_destination_exists_on_disk(&self) -> bool {
        self.conflicts
            .iter()
            .any(|c| matches!(c.kind, PlanConflictKind::DestinationExistsOnDisk))
    }

    pub fn is_blocked(&self) -> bool {
        self.status == PlanItemStatus::Blocked
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanCategoryCount {
    pub category: String,
    pub count: u32,
    #[specta(type = f64)]
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanRuleCount {
    pub rule_id: String,
    pub rule_name: String,
    pub count: u32,
}

/// Aggregate summary of an [`ExecutionPlan`] (`FR-083`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub plan_id: String,
    pub schema_version: u32,
    #[specta(type = f64)]
    pub created_at: i64,
    pub files_evaluated: u32,
    pub files_matched: u32,
    pub files_with_conflicts: u32,
    #[specta(type = f64)]
    pub total_bytes_affected: u64,
    #[specta(type = f64)]
    pub cross_volume_bytes: u64,
    pub by_category: Vec<PlanCategoryCount>,
    pub by_rule: Vec<PlanRuleCount>,
}

/// Immutable, versioned execution plan (`FR-085`).
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub summary: PlanSummary,
    pub items: Arc<[PlanItem]>,
}

/// Candidate input file fed into [`build_execution_plan`].
#[derive(Debug, Clone)]
pub struct PlanCandidate {
    pub folder_id: String,
    pub folder_path: PathBuf,
    pub folder_name: String,
    pub path: PathBuf,
    pub category_hint: Option<String>,
    pub mime_hint: Option<String>,
    pub content_hash: Option<String>,
    pub is_duplicate: bool,
}

/// Optional planner environment hints for testing/simulation (e.g. free bytes or simulated dev IDs).
#[derive(Debug, Clone, Default)]
pub struct PlanEnv {
    /// Overrides available free bytes for a destination root or ancestor path.
    pub free_bytes_by_prefix: HashMap<PathBuf, u64>,
    /// Overrides device ID (`dev`) for paths under a prefix (to simulate cross-volume plans in tests).
    pub dev_by_prefix: HashMap<PathBuf, u64>,
    /// Paths whose destination parent should be treated as unreadable (`EACCES`).
    pub permission_denied_prefixes: Vec<PathBuf>,
}

/// Filter for querying [`PlanItem`] rows in [`PreviewService::items`] (`FR-081`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemFilter {
    pub search: Option<String>,
    pub category: Option<String>,
    pub rule_id: Option<String>,
    pub only_conflicts: Option<bool>,
}

/// Paginated response of [`PlanItem`]s.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemsPage {
    pub items: Vec<PlanItem>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

/// Reason a plan item was skipped as stale during execution (`FR-086`, `SAFETY_MODEL.md` S6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StaleReason {
    SourceMissing,
    IdentityChanged,
    ModifiedSincePlan,
    DestinationOccupiedSincePlan,
    BlockedByConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkippedPlanItem {
    pub item_id: String,
    pub source_path: String,
    pub reason: StaleReason,
    pub message: String,
}

/// Result of executing an [`ExecutionPlan`] (`P5-T03`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanExecutionReport {
    pub plan_id: String,
    pub batch_id: String,
    pub completed: Vec<OperationView>,
    pub queued_for_review: u32,
    pub skipped_stale: Vec<SkippedPlanItem>,
    pub failed: Vec<String>,
}

/// Bounded in-memory store of active [`ExecutionPlan`]s (`FR-085`).
#[derive(Clone, Default)]
pub struct PreviewStore {
    inner: Arc<RwLock<PreviewStoreInner>>,
}

#[derive(Default)]
struct PreviewStoreInner {
    order: VecDeque<String>,
    plans: HashMap<String, ExecutionPlan>,
}

const MAX_STORED_PLANS: usize = 32;

impl PreviewStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, plan: ExecutionPlan) -> PlanSummary {
        let summary = plan.summary.clone();
        let mut guard = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let id = summary.plan_id.clone();
        if !guard.plans.contains_key(&id) {
            guard.order.push_back(id.clone());
            while guard.order.len() > MAX_STORED_PLANS {
                if let Some(evicted) = guard.order.pop_front() {
                    guard.plans.remove(&evicted);
                }
            }
        }
        guard.plans.insert(id, plan);
        summary
    }

    pub fn get(&self, plan_id: &str) -> Option<ExecutionPlan> {
        let guard = self.inner.read().unwrap_or_else(|e| e.into_inner());
        guard.plans.get(plan_id).cloned()
    }

    pub fn discard(&self, plan_id: &str) -> bool {
        let mut guard = self.inner.write().unwrap_or_else(|e| e.into_inner());
        guard.order.retain(|id| id != plan_id);
        guard.plans.remove(plan_id).is_some()
    }

    /// Removes targeted items that were executed or queued for review so a subsequent
    /// `plan_execute` call on the same plan cannot double-execute completed work (`BUG-024c`).
    pub fn prune_executed(
        &self,
        plan_id: &str,
        selected_ids: Option<&[String]>,
        report: &PlanExecutionReport,
    ) {
        let stale_ids: HashSet<&str> = report
            .skipped_stale
            .iter()
            .map(|s| s.item_id.as_str())
            .collect();
        let targeted: Option<HashSet<&str>> =
            selected_ids.map(|ids| ids.iter().map(String::as_str).collect());

        let mut guard = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if let Some(plan) = guard.plans.get_mut(plan_id) {
            let remaining: Vec<PlanItem> = plan
                .items
                .iter()
                .filter(|item| {
                    if item.status == PlanItemStatus::Blocked {
                        return true;
                    }
                    let was_targeted = targeted
                        .as_ref()
                        .is_none_or(|set| set.contains(item.id.as_str()));
                    if !was_targeted || stale_ids.contains(item.id.as_str()) {
                        return true;
                    }
                    let failed_prefix = format!("{}: ", item.source_path);
                    report
                        .failed
                        .iter()
                        .any(|msg| msg.starts_with(&failed_prefix))
                })
                .cloned()
                .collect();
            plan.items = Arc::from(remaining);
        }
    }

    pub fn query_items(
        &self,
        plan_id: &str,
        filter: &PlanItemFilter,
        limit: u32,
        offset: u32,
    ) -> AppResult<PlanItemsPage> {
        let plan = self
            .get(plan_id)
            .ok_or_else(|| AppError::NotFound(format!("Plan {plan_id}")))?;
        let needle = filter
            .search
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        let cat_filter = filter
            .category
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        let rule_filter = filter
            .rule_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let only_conflicts = filter.only_conflicts.unwrap_or(false);

        let filtered: Vec<PlanItem> = plan
            .items
            .iter()
            .filter(|item| {
                if only_conflicts && item.conflicts.is_empty() {
                    return false;
                }
                if let Some(cat) = &cat_filter {
                    if !item.category.eq_ignore_ascii_case(cat) {
                        return false;
                    }
                }
                if let Some(rid) = rule_filter {
                    if item.rule_id != rid {
                        return false;
                    }
                }
                if let Some(q) = &needle {
                    let hay_orig = item.original_name.to_ascii_lowercase();
                    let hay_prop = item.proposed_destination.to_ascii_lowercase();
                    let hay_rule = item.rule_name.to_ascii_lowercase();
                    if !hay_orig.contains(q) && !hay_prop.contains(q) && !hay_rule.contains(q) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        let total = u32::try_from(filtered.len()).unwrap_or(u32::MAX);
        let clamped_limit = limit.clamp(1, 500);
        let items = filtered
            .into_iter()
            .skip(offset as usize)
            .take(clamped_limit as usize)
            .collect();

        Ok(PlanItemsPage {
            items,
            total,
            limit: clamped_limit,
            offset,
        })
    }
}

fn find_prefix_value<T: Copy>(map: &HashMap<PathBuf, T>, target: &Path) -> Option<T> {
    let target_nfc = nfc(target);
    map.iter()
        .filter(|(prefix, _)| target_nfc.starts_with(nfc(prefix)))
        .max_by_key(|(prefix, _)| prefix.components().count())
        .map(|(_, val)| *val)
}

fn deepest_existing_ancestor_dev(fs: &dyn ReadOnlyFs, path: &Path) -> Option<u64> {
    let mut cur = Some(path);
    while let Some(p) = cur {
        if let Ok(meta) = fs.stat(p) {
            return Some(meta.dev);
        }
        cur = p.parent();
    }
    None
}

fn recompute_item_status(op: PlanOpKind, conflicts: &[PlanConflict]) -> PlanItemStatus {
    if conflicts.iter().any(|c| {
        matches!(
            c.kind,
            PlanConflictKind::DestinationUnauthorized
                | PlanConflictKind::PermissionDenied
                | PlanConflictKind::DestinationUnavailable
                | PlanConflictKind::InsufficientDiskSpace
        )
    }) {
        PlanItemStatus::Blocked
    } else if !conflicts.is_empty() {
        PlanItemStatus::Conflict
    } else if op == PlanOpKind::MarkForReview {
        PlanItemStatus::NeedsReview
    } else {
        PlanItemStatus::Ready
    }
}

/// Builds an immutable [`ExecutionPlan`] using only `&dyn ReadOnlyFs` (`P5-T01`, `P5-T02`).
/// Collects [`PlanCandidate`] entries for `folder`, honoring both per-folder filters and
/// global [`MonitoringSettings`] via [`FolderScanSpec`] (`BUG-022`).
pub fn collect_folder_candidates(
    folder: &MonitoredFolder,
    monitoring: &MonitoringSettings,
    classifier: &Classifier,
    explicit_paths: Option<&HashSet<PathBuf>>,
) -> Vec<PlanCandidate> {
    let spec = FolderScanSpec::from_folder(folder, monitoring);
    let opts = ScanOptions {
        recursive: spec.recursive,
        rules: spec.rules,
    };
    let mut out = Vec::new();
    let _ = scan(&spec.path, &opts, classifier, |batch| {
        for sf in batch {
            if let Some(allow) = explicit_paths {
                if !allow.contains(&nfc(&sf.facts.path)) {
                    continue;
                }
            }
            out.push(PlanCandidate {
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
    });
    out
}

/// Borrowed engine services required by [`build_execution_plan`].
#[derive(Clone, Copy)]
pub struct PlanContext<'a> {
    pub fs: &'a dyn ReadOnlyFs,
    pub authority: &'a PathAuthority,
    pub classifier: &'a Classifier,
    pub clock: &'a dyn Clock,
}

pub fn build_execution_plan(
    ctx: &PlanContext<'_>,
    rules: &[Rule],
    candidates: &[PlanCandidate],
    scope: &PlanScope,
    env: &PlanEnv,
) -> ExecutionPlan {
    let PlanContext {
        fs,
        authority,
        classifier,
        clock,
    } = *ctx;
    let scoped_rules: Vec<Rule> = if let Some(rule_id) = &scope.rule_id {
        rules
            .iter()
            .filter(|r| &r.id == rule_id)
            .map(|r| {
                let mut cloned = r.clone();
                cloned.enabled = true;
                cloned
            })
            .collect()
    } else {
        rules.to_vec()
    };

    let explicit_paths: Option<HashSet<PathBuf>> = scope.file_paths.as_ref().map(|list| {
        list.iter()
            .map(|p| nfc(Path::new(p)))
            .collect::<HashSet<PathBuf>>()
    });

    let mut files_evaluated: u32 = 0;
    let mut files_matched: u32 = 0;
    let mut items: Vec<PlanItem> = Vec::new();

    for cand in candidates {
        if let Some(folder_id) = &scope.folder_id {
            if &cand.folder_id != folder_id {
                continue;
            }
        }
        if let Some(allow_set) = &explicit_paths {
            if !allow_set.contains(&nfc(&cand.path)) {
                continue;
            }
        }

        let Ok(meta) = fs.stat(&cand.path) else {
            continue;
        };
        if !matches!(meta.kind, EntryKind::File | EntryKind::Package) {
            continue;
        }
        files_evaluated = files_evaluated.saturating_add(1);

        let (category, mime) = if let Some(cat) = &cand.category_hint {
            (cat.clone(), cand.mime_hint.clone())
        } else {
            let is_pkg = meta.kind == EntryKind::Package;
            let header = if is_pkg {
                None
            } else {
                fs.read_header(&cand.path, HEADER_BYTES).ok()
            };
            let cls = classifier.classify(&meta.name, header.as_deref(), is_pkg);
            (cls.category, cls.mime)
        };

        let (stem, extension) = FileFacts::split_name(&meta.name);
        let created_ms = meta.created_ns.map(|ns| ns / 1_000_000);
        let modified_ms = meta.modified_ns / 1_000_000;
        let facts = FileFacts {
            path: cand.path.to_string_lossy().into_owned(),
            name: meta.name.clone(),
            stem,
            extension,
            mime,
            category: category.clone(),
            size: meta.size,
            created_ms,
            modified_ms,
            added_ms: created_ms.or(Some(modified_ms)),
            is_hidden: meta.is_hidden,
            is_duplicate: cand.is_duplicate,
            content_hash: cand.content_hash.clone(),
            source_folder_id: cand.folder_id.clone(),
            source_folder_path: cand.folder_path.to_string_lossy().into_owned(),
            source_folder_name: cand.folder_name.clone(),
        };

        let outcome = evaluate_rules(&scoped_rules, &facts, clock);
        if outcome.matched_rules.is_empty() || outcome.skipped || outcome.actions.is_empty() {
            continue;
        }

        // Determine primary operation kind and proposed destination path.
        let mut op_kind: Option<PlanOpKind> = None;
        let mut copy_dest_dir: Option<String> = None;
        for act in &outcome.actions {
            match &act.op {
                PlannedOp::MarkForReview { .. } => {
                    op_kind = Some(PlanOpKind::MarkForReview);
                    break;
                }
                PlannedOp::Move { .. } => {
                    op_kind = Some(PlanOpKind::Move);
                }
                PlannedOp::Copy { destination_dir } => {
                    if op_kind.is_none() {
                        op_kind = Some(PlanOpKind::Copy);
                        copy_dest_dir = Some(destination_dir.clone());
                    }
                }
                PlannedOp::Rename { .. } => {
                    if op_kind.is_none() {
                        op_kind = Some(PlanOpKind::Rename);
                    }
                }
                PlannedOp::CreateFolder { .. } | PlannedOp::Notify { .. } => {}
            }
        }

        let Some(op) = op_kind else {
            continue;
        };

        let proposed_name = outcome.final_filename.clone();
        let proposed_destination = match op {
            PlanOpKind::Copy => {
                let dir = copy_dest_dir.unwrap_or_else(|| outcome.final_directory.clone());
                Path::new(&dir)
                    .join(&proposed_name)
                    .to_string_lossy()
                    .into_owned()
            }
            PlanOpKind::Move | PlanOpKind::Rename | PlanOpKind::MarkForReview => {
                outcome.final_path.clone()
            }
        };

        files_matched = files_matched.saturating_add(1);

        let primary_trace = &outcome.matched_rules[0];
        let matched_conditions: Vec<String> = outcome
            .matched_rules
            .iter()
            .flat_map(|t| t.matched_leaves.iter().cloned())
            .collect();
        let planned_ops: Vec<PlannedOp> = outcome.actions.iter().map(|a| a.op.clone()).collect();

        let mut conflicts: Vec<PlanConflict> = Vec::new();

        // 1. Conflicting rules in Continue mode (`FR-082`, `r_plan_10`).
        if outcome
            .warnings
            .iter()
            .any(|w| matches!(w, EvalWarning::MoveLimitReached { .. }))
        {
            conflicts.push(PlanConflict {
                kind: PlanConflictKind::ConflictingRules,
                message: "Multiple rules matched in continue mode with conflicting move actions."
                    .into(),
            });
        }

        let dst_path = PathBuf::from(&proposed_destination);
        let src_dev = find_prefix_value(&env.dev_by_prefix, &cand.path).unwrap_or(meta.dev);
        let mut cross_volume = false;

        if op != PlanOpKind::MarkForReview {
            // 2. PathAuthority containment check (`r_plan_08`).
            match authority.authorize(&dst_path) {
                Err(err) => {
                    conflicts.push(PlanConflict {
                        kind: PlanConflictKind::DestinationUnauthorized,
                        message: err.to_string(),
                    });
                }
                Ok(auth_dst) => {
                    // 3. Check if the approved destination root itself is mounted/available (`r_plan_09`).
                    match fs.stat(&auth_dst.root) {
                        Err(e) if e.kind() == ErrorKind::NotFound => {
                            conflicts.push(PlanConflict {
                                kind: PlanConflictKind::DestinationUnavailable,
                                message: format!(
                                    "Destination volume or root folder is unavailable: {}",
                                    auth_dst.root.display()
                                ),
                            });
                        }
                        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                            conflicts.push(PlanConflict {
                                kind: PlanConflictKind::PermissionDenied,
                                message: format!(
                                    "Permission denied accessing destination root: {}",
                                    auth_dst.root.display()
                                ),
                            });
                        }
                        _ => {}
                    }

                    // 4. Permission check on destination prefix (`r_plan_08`).
                    let dst_nfc = nfc(&dst_path);
                    let sim_denied = env
                        .permission_denied_prefixes
                        .iter()
                        .any(|p| dst_nfc.starts_with(nfc(p)));
                    if sim_denied {
                        conflicts.push(PlanConflict {
                            kind: PlanConflictKind::PermissionDenied,
                            message: "Permission denied writing to destination folder.".into(),
                        });
                    } else if let Some(parent) = dst_path.parent() {
                        // Walk ancestors until reaching an existing directory so `PermissionDenied`
                        // on a locked ancestor is caught even when deeper target subdirs do not yet exist.
                        let mut cur = Some(parent);
                        while let Some(dir) = cur {
                            match fs.stat(dir) {
                                Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                                    conflicts.push(PlanConflict {
                                        kind: PlanConflictKind::PermissionDenied,
                                        message: "Permission denied writing to destination folder."
                                            .into(),
                                    });
                                    break;
                                }
                                Err(e) if e.kind() == ErrorKind::NotFound => {
                                    cur = dir.parent();
                                }
                                _ => break,
                            }
                        }
                    }

                    // 5. Cross-volume & free space checks (`r_plan_05`, `r_plan_09`).
                    let dst_dev = find_prefix_value(&env.dev_by_prefix, &dst_path)
                        .or_else(|| deepest_existing_ancestor_dev(fs, &dst_path))
                        .unwrap_or(src_dev);
                    if op == PlanOpKind::Move && dst_dev != src_dev {
                        cross_volume = true;
                    }

                    if op == PlanOpKind::Copy || cross_volume {
                        if let Some(free_bytes) =
                            find_prefix_value(&env.free_bytes_by_prefix, &dst_path)
                        {
                            if free_bytes < meta.size {
                                conflicts.push(PlanConflict {
                                    kind: PlanConflictKind::InsufficientDiskSpace,
                                    message: format!(
                                        "Insufficient free space at destination ({free_bytes} B available, {} B required).",
                                        meta.size
                                    ),
                                });
                            }
                        }
                    }

                    // 6. Existing file on disk at proposed destination (`r_plan_06`).
                    if nfc(&dst_path) != nfc(&cand.path) && fs.stat(&dst_path).is_ok() {
                        conflicts.push(PlanConflict {
                            kind: PlanConflictKind::DestinationExistsOnDisk,
                            message: "A file already exists at the proposed destination.".into(),
                        });
                    }
                }
            }
        }

        let current_location = cand
            .path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| cand.folder_path.to_string_lossy().into_owned());
        let status = recompute_item_status(op, &conflicts);

        items.push(PlanItem {
            id: format!("plan-item-{}", items.len() + 1),
            folder_id: cand.folder_id.clone(),
            source_path: cand.path.to_string_lossy().into_owned(),
            current_location,
            original_name: meta.name,
            proposed_destination,
            proposed_name,
            size: meta.size,
            category,
            op,
            rule_id: primary_trace.rule_id.clone(),
            rule_name: primary_trace.rule_name.clone(),
            matched_conditions,
            planned_ops,
            conflicts,
            status,
            cross_volume,
            identity: FileIdentitySnapshot {
                dev: meta.dev,
                ino: meta.ino,
                size: meta.size,
                mtime_ns: meta.modified_ns,
            },
        });
    }

    // 7. Intra-plan duplicate destinations (`r_plan_07`).
    let mut by_dest: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        if item.op != PlanOpKind::MarkForReview {
            by_dest
                .entry(nfc(Path::new(&item.proposed_destination)))
                .or_default()
                .push(idx);
        }
    }
    for indices in by_dest.values() {
        if indices.len() > 1 {
            let all_ids: Vec<String> = indices.iter().map(|&i| items[i].id.clone()).collect();
            for &idx in indices {
                let my_id = &items[idx].id;
                let colliding: Vec<String> =
                    all_ids.iter().filter(|id| *id != my_id).cloned().collect();
                items[idx].conflicts.push(PlanConflict {
                    kind: PlanConflictKind::DuplicateInPlan {
                        colliding_item_ids: colliding,
                    },
                    message: "Multiple files in this plan target the same destination path.".into(),
                });
                items[idx].status = recompute_item_status(items[idx].op, &items[idx].conflicts);
            }
        }
    }

    // Compute summary aggregates (`FR-083`, `r_plan_05`).
    let mut files_with_conflicts: u32 = 0;
    let mut total_bytes_affected: u64 = 0;
    let mut cross_volume_bytes: u64 = 0;
    let mut cat_map: BTreeMap<String, (u32, u64)> = BTreeMap::new();
    let mut rule_map: BTreeMap<String, (String, u32)> = BTreeMap::new();

    for item in &items {
        if !item.conflicts.is_empty() {
            files_with_conflicts = files_with_conflicts.saturating_add(1);
        }
        total_bytes_affected = total_bytes_affected.saturating_add(item.size);
        if item.cross_volume {
            cross_volume_bytes = cross_volume_bytes.saturating_add(item.size);
        }
        let cat_entry = cat_map.entry(item.category.clone()).or_insert((0, 0));
        cat_entry.0 = cat_entry.0.saturating_add(1);
        cat_entry.1 = cat_entry.1.saturating_add(item.size);

        let rule_entry = rule_map
            .entry(item.rule_id.clone())
            .or_insert_with(|| (item.rule_name.clone(), 0));
        rule_entry.1 = rule_entry.1.saturating_add(1);
    }

    let mut by_category: Vec<PlanCategoryCount> = cat_map
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

    let mut by_rule: Vec<PlanRuleCount> = rule_map
        .into_iter()
        .map(|(rule_id, (rule_name, count))| PlanRuleCount {
            rule_id,
            rule_name,
            count,
        })
        .collect();
    by_rule.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.rule_id.cmp(&b.rule_id))
    });

    let summary = PlanSummary {
        plan_id: Uuid::now_v7().to_string(),
        schema_version: PLAN_SCHEMA_VERSION,
        created_at: clock.now_ms(),
        files_evaluated,
        files_matched,
        files_with_conflicts,
        total_bytes_affected,
        cross_volume_bytes,
        by_category,
        by_rule,
    };

    ExecutionPlan {
        summary,
        items: Arc::from(items),
    }
}

/// Executes selected items from an [`ExecutionPlan`], revalidating each item's
/// `(dev, ino, size, mtime_ns)` and destination before executing (`P5-T03`).
pub async fn execute_plan(
    plan: &ExecutionPlan,
    selected_item_ids: Option<&[String]>,
    conflict_policy: ConflictPolicy,
    organizer: &Organizer,
    authority: &PathAuthority,
    trigger: BatchTrigger,
) -> AppResult<PlanExecutionReport> {
    let selected_set: Option<HashSet<&str>> =
        selected_item_ids.map(|ids| ids.iter().map(String::as_str).collect());

    let selected_items: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| {
            selected_set
                .as_ref()
                .is_none_or(|set| set.contains(item.id.as_str()))
        })
        .collect();

    let batch_id = Uuid::now_v7().to_string();
    let now_ms = organizer.clock().now_ms();
    organizer
        .ledger()
        .create_batch(
            &batch_id,
            trigger,
            Some(&plan.summary.plan_id),
            u32::try_from(selected_items.len()).unwrap_or(u32::MAX),
            now_ms,
        )
        .await?;

    let mut completed: Vec<OperationView> = Vec::new();
    let mut queued_for_review: u32 = 0;
    let mut skipped_stale: Vec<SkippedPlanItem> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    for item in selected_items {
        if item.is_blocked() {
            let reason_msg = item
                .conflicts
                .first()
                .map(|c| c.message.clone())
                .unwrap_or_else(|| "Blocked by plan conflict".into());
            skipped_stale.push(SkippedPlanItem {
                item_id: item.id.clone(),
                source_path: item.source_path.clone(),
                reason: StaleReason::BlockedByConflict,
                message: reason_msg,
            });
            continue;
        }

        let src_path = PathBuf::from(&item.source_path);
        let dst_path = PathBuf::from(&item.proposed_destination);

        // Per-item source revalidation (`FR-085`, `FR-086`, `SAFETY_MODEL.md` S6).
        let live_meta = match organizer.fs().lstat(&src_path) {
            Ok(m) => m,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                skipped_stale.push(SkippedPlanItem {
                    item_id: item.id.clone(),
                    source_path: item.source_path.clone(),
                    reason: StaleReason::SourceMissing,
                    message: "Source file no longer exists.".into(),
                });
                continue;
            }
            Err(e) => {
                failed.push(format!("{}: {e}", item.source_path));
                continue;
            }
        };

        if live_meta.dev != item.identity.dev || live_meta.ino != item.identity.ino {
            skipped_stale.push(SkippedPlanItem {
                item_id: item.id.clone(),
                source_path: item.source_path.clone(),
                reason: StaleReason::IdentityChanged,
                message:
                    "Source file was replaced by a different inode since the plan was created."
                        .into(),
            });
            continue;
        }

        if live_meta.size != item.identity.size || live_meta.mtime_ns != item.identity.mtime_ns {
            skipped_stale.push(SkippedPlanItem {
                item_id: item.id.clone(),
                source_path: item.source_path.clone(),
                reason: StaleReason::ModifiedSincePlan,
                message:
                    "Source file size or modification time changed since the plan was created."
                        .into(),
            });
            continue;
        }

        // Only bind `rule_id` if the rule exists in the SQLite `rules` table (foreign key constraint).
        let db_rule_id = repo_rules::get(organizer.ledger().db(), &item.rule_id)
            .await
            .ok()
            .flatten()
            .map(|r| r.id);

        if item.op == PlanOpKind::MarkForReview {
            let reason = item
                .planned_ops
                .iter()
                .find_map(|op| match op {
                    PlannedOp::MarkForReview { reason } => Some(reason.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Marked for review by rule".into());
            let _ = repo_operations::enqueue_review(
                organizer.ledger().db(),
                &Uuid::now_v7().to_string(),
                &item.source_path,
                &reason,
                db_rule_id.as_deref(),
                Some(&ReviewProposedAction {
                    operation_type: OpKind::Move,
                    source_path: item.source_path.clone(),
                    destination_path: item.proposed_destination.clone(),
                    original_name: item.original_name.clone(),
                    new_name: item.proposed_name.clone(),
                }),
                organizer.clock().now_ms(),
            )
            .await;
            queued_for_review = queued_for_review.saturating_add(1);
            continue;
        }

        // Destination revalidation (`r_exec_05`): if the destination was free at plan time
        // and a file appeared there before execution, honor `ConflictPolicy::Skip` by
        // reporting `DestinationOccupiedSincePlan`.
        if !item.has_destination_exists_on_disk()
            && nfc(&dst_path) != nfc(&src_path)
            && organizer.fs().lstat(&dst_path).is_ok()
            && conflict_policy == ConflictPolicy::Skip
        {
            skipped_stale.push(SkippedPlanItem {
                item_id: item.id.clone(),
                source_path: item.source_path.clone(),
                reason: StaleReason::DestinationOccupiedSincePlan,
                message: "Destination became occupied after the plan was created.".into(),
            });
            continue;
        }

        let op_kind = match item.op {
            PlanOpKind::Move => OpKind::Move,
            PlanOpKind::Copy => OpKind::Copy,
            PlanOpKind::Rename => OpKind::Rename,
            PlanOpKind::MarkForReview => unreachable!(),
        };

        let mut req = OrganizeRequest::new(op_kind, src_path, dst_path);
        req.batch_id = Some(batch_id.clone());
        req.rule_id = db_rule_id.clone();
        req.conflict_policy = conflict_policy;
        req.expected_source = Some(EntryIdentity {
            dev: item.identity.dev,
            ino: item.identity.ino,
            size: item.identity.size,
            mtime_ns: item.identity.mtime_ns,
            kind: live_meta.kind,
        });

        match organizer.execute(req, authority).await {
            Ok(OrganizeOutcome::Completed(mut row)) => {
                if row.rule_name.is_none() {
                    row.rule_name = Some(item.rule_name.clone());
                }
                if let Some(rid) = &db_rule_id {
                    let _ = repo_rules::record_stats(
                        organizer.ledger().db(),
                        rid,
                        1,
                        1,
                        0,
                        organizer.clock().now_ms(),
                    )
                    .await;
                }
                completed.push(OperationView::from(row));
            }
            Ok(OrganizeOutcome::QueuedForReview { .. }) => {
                queued_for_review = queued_for_review.saturating_add(1);
            }
            Ok(OrganizeOutcome::SkippedConflict(_)) => {
                skipped_stale.push(SkippedPlanItem {
                    item_id: item.id.clone(),
                    source_path: item.source_path.clone(),
                    reason: StaleReason::DestinationOccupiedSincePlan,
                    message: "Destination conflict skipped per policy.".into(),
                });
            }
            Ok(OrganizeOutcome::NoOpSamePath) => {}
            Err(e) => {
                if let Some(rid) = &db_rule_id {
                    let _ = repo_rules::record_stats(
                        organizer.ledger().db(),
                        rid,
                        1,
                        0,
                        1,
                        organizer.clock().now_ms(),
                    )
                    .await;
                }
                failed.push(format!("{}: {e}", item.source_path));
            }
        }
    }

    organizer
        .ledger()
        .complete_batch(&batch_id, organizer.clock().now_ms())
        .await?;

    Ok(PlanExecutionReport {
        plan_id: plan.summary.plan_id.clone(),
        batch_id,
        completed,
        queued_for_review,
        skipped_stale,
        failed,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)]

    use std::path::Path;
    use std::sync::Arc;

    use super::*;
    use crate::categories::builtin_categories;
    use crate::database;
    use crate::engine::classifier::Classifier;
    use crate::engine::fs_view::RealFs;
    use crate::engine::operations::Ledger;
    use crate::engine::paths::{ApprovedRoot, PathAuthority, RootKind};
    use crate::engine::safe_fs::SafeFs;
    use crate::rules::models::{
        Action, Condition, ConditionNode, DateSource, ExecutionMode, Rule, RuleOrigin,
        TextFieldKind, TextOp,
    };
    use crate::test_support::{mkdir, snapshot_tree, tempdir, write_file};
    use crate::utils::clock::FixedClock;

    fn make_classifier() -> Classifier {
        Classifier::new(builtin_categories())
    }

    fn make_authority(roots: &[&Path]) -> PathAuthority {
        PathAuthority::new(
            roots
                .iter()
                .map(|p| ApprovedRoot {
                    path: (*p).to_path_buf(),
                    kind: RootKind::Monitored,
                })
                .collect(),
        )
    }

    fn make_rule(
        id: &str,
        name: &str,
        priority: i32,
        conditions: ConditionNode,
        actions: Vec<Action>,
        mode: ExecutionMode,
    ) -> Rule {
        Rule {
            id: id.into(),
            name: name.into(),
            description: None,
            enabled: true,
            priority,
            source_folders: Vec::new(),
            conditions,
            actions,
            date_source: DateSource::Modified,
            execution_mode: mode,
            origin: RuleOrigin::User,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_000_000,
        }
    }

    fn leaf_text(field: TextFieldKind, op: TextOp, value: &str) -> ConditionNode {
        ConditionNode::Leaf {
            condition: Condition::Text {
                field,
                op,
                value: value.into(),
                case_sensitive: false,
            },
        }
    }

    fn all_empty() -> ConditionNode {
        ConditionNode::All {
            children: Vec::new(),
        }
    }

    fn candidate(folder_id: &str, root: &Path, rel: &str) -> PlanCandidate {
        PlanCandidate {
            folder_id: folder_id.into(),
            folder_path: root.to_path_buf(),
            folder_name: "Downloads".into(),
            path: root.join(rel),
            category_hint: None,
            mime_hint: None,
            content_hash: None,
            is_duplicate: false,
        }
    }

    #[test]
    fn r_plan_01_dry_run_plan_performs_zero_filesystem_writes() {
        let (_d, root) = tempdir();
        let f1 = root.join("invoice_2026.pdf");
        let f2 = root.join("photo.png");
        write_file(&f1, b"%PDF-1.7 sample invoice");
        write_file(&f2, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR");

        let before = snapshot_tree(&root);
        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);
        let rule = make_rule(
            "r-pdf",
            "Sort PDFs",
            1,
            ConditionNode::Leaf {
                condition: Condition::Category {
                    category_id: "documents".into(),
                    negate: false,
                },
            },
            vec![Action::Move {
                destination: root.join("Sorted/Docs").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[
                candidate("f1", &root, "invoice_2026.pdf"),
                candidate("f1", &root, "photo.png"),
            ],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        let after = snapshot_tree(&root);
        assert_eq!(before, after, "dry-run planning must perform zero writes");
        assert_eq!(plan.summary.files_evaluated, 2);
        assert_eq!(plan.summary.files_matched, 1);
    }

    #[test]
    fn r_plan_02_evaluates_rules_across_scoped_files_and_captures_identity_snapshot() {
        let (_d, root) = tempdir();
        let f1 = root.join("contract.pdf");
        write_file(&f1, b"%PDF-1.7 contract");
        let stat = RealFs.stat(&f1).expect("stat");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);
        let dest_dir = root.join("Contracts");
        let rule = make_rule(
            "r-1",
            "Contracts Rule",
            1,
            leaf_text(TextFieldKind::Name, TextOp::Contains, "contract"),
            vec![Action::Move {
                destination: dest_dir.to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[candidate("f1", &root, "contract.pdf")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        assert_eq!(plan.items.len(), 1);
        let item = &plan.items[0];
        assert_eq!(item.original_name, "contract.pdf");
        assert_eq!(
            item.proposed_destination,
            dest_dir.join("contract.pdf").to_string_lossy()
        );
        assert_eq!(item.rule_id, "r-1");
        assert_eq!(item.rule_name, "Contracts Rule");
        assert_eq!(item.op, PlanOpKind::Move);
        assert_eq!(item.identity.dev, stat.dev);
        assert_eq!(item.identity.ino, stat.ino);
        assert_eq!(item.identity.size, stat.size);
        assert_eq!(item.identity.mtime_ns, stat.modified_ns);
    }

    #[test]
    fn r_plan_03_supports_folder_scope_rule_scope_and_explicit_file_list_scope() {
        let (_d, root) = tempdir();
        let f_a = root.join("a.txt");
        let f_b = root.join("b.txt");
        write_file(&f_a, b"alpha");
        write_file(&f_b, b"beta");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);

        let r1 = make_rule(
            "r1",
            "Rule A",
            1,
            leaf_text(TextFieldKind::Name, TextOp::StartsWith, "a"),
            vec![Action::Move {
                destination: root.join("OutA").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );
        let mut r2 = make_rule(
            "r2",
            "Rule B",
            2,
            leaf_text(TextFieldKind::Name, TextOp::StartsWith, "b"),
            vec![Action::Move {
                destination: root.join("OutB").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );
        // Even if r2 is disabled, scoping explicitly to `rule_id: Some("r2")` previews r2!
        r2.enabled = false;

        let cands = vec![
            candidate("folder-1", &root, "a.txt"),
            candidate("folder-2", &root, "b.txt"),
        ];

        // 1. Folder scope = folder-1
        let p_folder = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r1.clone(), r2.clone()],
            &cands,
            &PlanScope {
                folder_id: Some("folder-1".into()),
                rule_id: None,
                file_paths: None,
            },
            &PlanEnv::default(),
        );
        assert_eq!(p_folder.items.len(), 1);
        assert_eq!(p_folder.items[0].original_name, "a.txt");

        // 2. Rule scope = r2
        let p_rule = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r1.clone(), r2.clone()],
            &cands,
            &PlanScope {
                folder_id: None,
                rule_id: Some("r2".into()),
                file_paths: None,
            },
            &PlanEnv::default(),
        );
        assert_eq!(p_rule.items.len(), 1);
        assert_eq!(p_rule.items[0].original_name, "b.txt");

        // 3. Explicit file list scope
        let p_files = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r1],
            &cands,
            &PlanScope {
                folder_id: None,
                rule_id: None,
                file_paths: Some(vec![f_a.to_string_lossy().into_owned()]),
            },
            &PlanEnv::default(),
        );
        assert_eq!(p_files.summary.files_evaluated, 1);
        assert_eq!(p_files.items.len(), 1);
    }

    #[test]
    fn r_plan_04_handles_rename_plus_move_and_copy_actions_and_review_items() {
        let (_d, root) = tempdir();
        write_file(&root.join("bill.pdf"), b"%PDF-1.7 bill");
        write_file(&root.join("backup.txt"), b"backup");
        write_file(&root.join("suspect.bin"), b"mystery");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);

        let r_move_ren = make_rule(
            "r-mr",
            "Rename and Move Bill",
            1,
            leaf_text(TextFieldKind::Name, TextOp::Equals, "bill.pdf"),
            vec![
                Action::AddPrefix {
                    prefix: "PAID_".into(),
                },
                Action::Move {
                    destination: root.join("Paid").to_string_lossy().into_owned(),
                },
            ],
            ExecutionMode::FirstMatch,
        );

        let r_copy = make_rule(
            "r-cp",
            "Copy Backup",
            2,
            leaf_text(TextFieldKind::Name, TextOp::Equals, "backup.txt"),
            vec![Action::Copy {
                destination: root.join("Copies").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let r_rev = make_rule(
            "r-rev",
            "Review Suspect",
            3,
            leaf_text(TextFieldKind::Name, TextOp::Equals, "suspect.bin"),
            vec![Action::MarkForReview {
                reason: "Verify binary file".into(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r_move_ren, r_copy, r_rev],
            &[
                candidate("f1", &root, "bill.pdf"),
                candidate("f1", &root, "backup.txt"),
                candidate("f1", &root, "suspect.bin"),
            ],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        assert_eq!(plan.items.len(), 3);
        let bill = plan
            .items
            .iter()
            .find(|i| i.original_name == "bill.pdf")
            .expect("bill");
        assert_eq!(bill.op, PlanOpKind::Move);
        assert_eq!(bill.proposed_name, "PAID_bill.pdf");
        assert_eq!(
            bill.proposed_destination,
            root.join("Paid/PAID_bill.pdf").to_string_lossy()
        );

        let backup = plan
            .items
            .iter()
            .find(|i| i.original_name == "backup.txt")
            .expect("backup");
        assert_eq!(backup.op, PlanOpKind::Copy);
        assert_eq!(
            backup.proposed_destination,
            root.join("Copies/backup.txt").to_string_lossy()
        );

        let suspect = plan
            .items
            .iter()
            .find(|i| i.original_name == "suspect.bin")
            .expect("suspect");
        assert_eq!(suspect.op, PlanOpKind::MarkForReview);
        assert_eq!(suspect.status, PlanItemStatus::NeedsReview);
    }

    #[test]
    fn r_plan_05_computes_accurate_summary_totals() {
        let (_d, root) = tempdir();
        write_file(&root.join("vol_a/doc.pdf"), &[b'A'; 1000]);
        write_file(&root.join("vol_a/pic.png"), &[b'B'; 2500]);

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);

        let rule = make_rule(
            "r-all",
            "Move All to Vol B",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("vol_b").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let mut env = PlanEnv::default();
        env.dev_by_prefix.insert(root.join("vol_a"), 10);
        env.dev_by_prefix.insert(root.join("vol_b"), 20);

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[
                candidate("f1", &root, "vol_a/doc.pdf"),
                candidate("f1", &root, "vol_a/pic.png"),
            ],
            &PlanScope::default(),
            &env,
        );

        assert_eq!(plan.summary.files_evaluated, 2);
        assert_eq!(plan.summary.files_matched, 2);
        assert_eq!(plan.summary.total_bytes_affected, 3500);
        assert_eq!(plan.summary.cross_volume_bytes, 3500);
        assert_eq!(plan.summary.by_rule.len(), 1);
        assert_eq!(plan.summary.by_rule[0].count, 2);
    }

    #[test]
    fn r_plan_06_detects_existing_destination_on_disk() {
        let (_d, root) = tempdir();
        write_file(&root.join("in/report.pdf"), b"new");
        write_file(&root.join("out/report.pdf"), b"existing");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);
        let rule = make_rule(
            "r1",
            "Move Report",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("out").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[candidate("f1", &root, "in/report.pdf")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        assert_eq!(plan.summary.files_with_conflicts, 1);
        assert_eq!(plan.items[0].status, PlanItemStatus::Conflict);
        assert!(plan.items[0].has_destination_exists_on_disk());
    }

    #[test]
    fn r_plan_07_detects_intra_plan_duplicate_destinations() {
        let (_d, root) = tempdir();
        write_file(&root.join("sub1/same.txt"), b"one");
        write_file(&root.join("sub2/same.txt"), b"two");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);
        let rule = make_rule(
            "r1",
            "Flatten to Merged",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("Merged").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[
                candidate("f1", &root, "sub1/same.txt"),
                candidate("f1", &root, "sub2/same.txt"),
            ],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        assert_eq!(plan.summary.files_with_conflicts, 2);
        for item in plan.items.iter() {
            assert!(
                item.conflicts
                    .iter()
                    .any(|c| matches!(c.kind, PlanConflictKind::DuplicateInPlan { .. })),
                "both items must report DuplicateInPlan: {item:?}"
            );
        }
    }

    #[test]
    fn r_plan_08_detects_outside_approved_root_or_permission_denied_destination() {
        let (_d, root) = tempdir();
        let (_d2, unapproved) = tempdir();
        write_file(&root.join("f1.txt"), b"one");
        write_file(&root.join("f2.txt"), b"two");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);

        let r_unauth = make_rule(
            "r-unauth",
            "Outside Root",
            1,
            leaf_text(TextFieldKind::Name, TextOp::Equals, "f1.txt"),
            vec![Action::Move {
                destination: unapproved.join("secret").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let locked_dir = root.join("Locked");
        mkdir(&locked_dir);
        let r_perm = make_rule(
            "r-perm",
            "Into Locked",
            2,
            leaf_text(TextFieldKind::Name, TextOp::Equals, "f2.txt"),
            vec![Action::Move {
                destination: locked_dir.to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let mut env = PlanEnv::default();
        env.permission_denied_prefixes.push(locked_dir);

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r_unauth, r_perm],
            &[
                candidate("f1", &root, "f1.txt"),
                candidate("f1", &root, "f2.txt"),
            ],
            &PlanScope::default(),
            &env,
        );

        let item1 = plan
            .items
            .iter()
            .find(|i| i.original_name == "f1.txt")
            .expect("f1");
        assert!(item1
            .conflicts
            .iter()
            .any(|c| matches!(c.kind, PlanConflictKind::DestinationUnauthorized)));
        assert_eq!(item1.status, PlanItemStatus::Blocked);

        let item2 = plan
            .items
            .iter()
            .find(|i| i.original_name == "f2.txt")
            .expect("f2");
        assert!(item2
            .conflicts
            .iter()
            .any(|c| matches!(c.kind, PlanConflictKind::PermissionDenied)));
        assert_eq!(item2.status, PlanItemStatus::Blocked);
    }

    #[test]
    fn r_plan_09_detects_unavailable_destination_volume_or_insufficient_space() {
        let (_d, root) = tempdir();
        let missing_vol = root.join("unmounted_drive");
        write_file(&root.join("big.bin"), &[0u8; 4096]);

        let auth = PathAuthority::new(vec![
            ApprovedRoot {
                path: root.clone(),
                kind: RootKind::Monitored,
            },
            ApprovedRoot {
                path: missing_vol.clone(),
                kind: RootKind::Destination,
            },
        ]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);

        // 1. Destination root directory does not exist (unmounted volume).
        let r_unavail = make_rule(
            "r-unavail",
            "Move to Unmounted",
            1,
            all_empty(),
            vec![Action::Move {
                destination: missing_vol.join("Archive").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan_unavail = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r_unavail],
            &[candidate("f1", &root, "big.bin")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );
        assert!(plan_unavail.items[0]
            .conflicts
            .iter()
            .any(|c| matches!(c.kind, PlanConflictKind::DestinationUnavailable)));

        // 2. Cross-volume destination with insufficient free disk space.
        let r_space = make_rule(
            "r-space",
            "Copy to Low Space",
            1,
            all_empty(),
            vec![Action::Copy {
                destination: root.join("Dest").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );
        let mut env = PlanEnv::default();
        env.free_bytes_by_prefix.insert(root.clone(), 512); // Only 512 B free!

        let plan_space = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r_space],
            &[candidate("f1", &root, "big.bin")],
            &PlanScope::default(),
            &env,
        );
        assert!(plan_space.items[0]
            .conflicts
            .iter()
            .any(|c| matches!(c.kind, PlanConflictKind::InsufficientDiskSpace)));
    }

    #[test]
    fn r_plan_11_dry_run_over_10k_files_performs_zero_writes() {
        let (_d, root) = tempdir();
        let src_dir = root.join("bulk");
        mkdir(&src_dir);
        let mut cands = Vec::with_capacity(10_000);
        for i in 0..10_000 {
            let name = format!("file_{i:05}.pdf");
            let p = src_dir.join(&name);
            std::fs::write(&p, b"%PDF-1.7\n").expect("write bulk fixture");
            cands.push(PlanCandidate {
                folder_id: "f-bulk".into(),
                folder_path: root.clone(),
                folder_name: "Downloads".into(),
                path: p,
                category_hint: Some("documents".into()),
                mime_hint: Some("application/pdf".into()),
                content_hash: None,
                is_duplicate: false,
            });
        }

        let before = snapshot_tree(&root);
        assert_eq!(before.len(), 10_002); // root + bulk + 10,000 files

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);
        let rule = make_rule(
            "r-bulk",
            "Sort Bulk PDFs",
            1,
            ConditionNode::Leaf {
                condition: Condition::Category {
                    category_id: "documents".into(),
                    negate: false,
                },
            },
            vec![Action::Move {
                destination: root.join("Sorted/Bulk").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &cands,
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        let after = snapshot_tree(&root);
        assert_eq!(
            before, after,
            "10k-file dry run must perform zero FS mutations"
        );
        assert_eq!(plan.summary.files_evaluated, 10_000);
        assert_eq!(plan.summary.files_matched, 10_000);
    }

    #[test]
    fn r_plan_10_detects_conflicting_rules_targeting_same_file_in_continue_mode() {
        let (_d, root) = tempdir();
        write_file(&root.join("shared.pdf"), b"%PDF-1.7 shared");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);

        let r1 = make_rule(
            "r1",
            "Move to Folder A",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("FolderA").to_string_lossy().into_owned(),
            }],
            ExecutionMode::Continue,
        );
        let r2 = make_rule(
            "r2",
            "Move to Folder B",
            2,
            all_empty(),
            vec![Action::Move {
                destination: root.join("FolderB").to_string_lossy().into_owned(),
            }],
            ExecutionMode::Continue,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[r1, r2],
            &[candidate("f1", &root, "shared.pdf")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        assert_eq!(plan.items.len(), 1);
        assert!(
            plan.items[0]
                .conflicts
                .iter()
                .any(|c| matches!(c.kind, PlanConflictKind::ConflictingRules)),
            "second move in Continue mode triggers MoveLimitReached -> ConflictingRules"
        );
    }

    async fn setup_executor(
        root: &Path,
    ) -> (
        Organizer,
        PathAuthority,
        Classifier,
        FixedClock,
        database::Db,
    ) {
        let db = database::open_in_memory().await.expect("db");
        let ledger = Ledger::new(db.clone());
        let fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let clock = FixedClock::utc(1_700_000_000_000);
        let org = Organizer::new(ledger, fs, Arc::new(clock.clone()));
        let auth = make_authority(&[root]);
        let cls = make_classifier();
        (org, auth, cls, clock, db)
    }

    #[tokio::test]
    async fn r_exec_01_executes_selected_plan_items_in_a_single_batch_and_skips_unselected() {
        let (_d, root) = tempdir();
        let (org, auth, cls, clock, _db) = setup_executor(&root).await;
        write_file(&root.join("one.txt"), b"111");
        write_file(&root.join("two.txt"), b"222");

        let rule = make_rule(
            "r1",
            "Move Txt",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("Sorted").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[
                candidate("f1", &root, "one.txt"),
                candidate("f1", &root, "two.txt"),
            ],
            &PlanScope::default(),
            &PlanEnv::default(),
        );
        assert_eq!(plan.items.len(), 2);

        // Select ONLY the first item (`one.txt`).
        let selected = vec![plan.items[0].id.clone()];
        let rep = execute_plan(
            &plan,
            Some(&selected),
            ConflictPolicy::AutoRename,
            &org,
            &auth,
            BatchTrigger::Preview,
        )
        .await
        .expect("execute_plan");

        assert_eq!(rep.completed.len(), 1);
        assert!(!root.join("one.txt").exists());
        assert!(root.join("Sorted/one.txt").exists());
        // `two.txt` was deselected, so it remains untouched at source!
        assert!(root.join("two.txt").exists());
        assert!(!root.join("Sorted/two.txt").exists());
    }

    #[tokio::test]
    async fn r_exec_02_skips_stale_item_when_source_inode_or_dev_changed() {
        let (_d, root) = tempdir();
        let (org, auth, cls, clock, _db) = setup_executor(&root).await;
        let f1 = root.join("swap.txt");
        write_file(&f1, b"same-len");

        let rule = make_rule(
            "r1",
            "Move Swap",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("Sorted").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[candidate("f1", &root, "swap.txt")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        // Replace `swap.txt` with a brand-new inode (unlink + recreate with identical length).
        let replacement = root.join("replacement.txt");
        write_file(&replacement, b"diff-ino");
        std::fs::remove_file(&f1).expect("remove original");
        std::fs::rename(&replacement, &f1).expect("swap inode");

        let rep = execute_plan(
            &plan,
            None,
            ConflictPolicy::AutoRename,
            &org,
            &auth,
            BatchTrigger::Preview,
        )
        .await
        .expect("execute_plan");

        assert!(rep.completed.is_empty());
        assert_eq!(rep.skipped_stale.len(), 1);
        assert_eq!(rep.skipped_stale[0].reason, StaleReason::IdentityChanged);
        assert!(f1.exists());
    }

    #[tokio::test]
    async fn r_exec_03_skips_stale_item_when_source_size_or_mtime_changed() {
        let (_d, root) = tempdir();
        let (org, auth, cls, clock, _db) = setup_executor(&root).await;
        let f1 = root.join("growing.txt");
        write_file(&f1, b"initial");

        let rule = make_rule(
            "r1",
            "Move Growing",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("Sorted").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[candidate("f1", &root, "growing.txt")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        // Append bytes in-place (same inode, changed size).
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&f1)
            .expect("open append");
        f.write_all(b"-more-bytes").expect("append");
        drop(f);

        let rep = execute_plan(
            &plan,
            None,
            ConflictPolicy::AutoRename,
            &org,
            &auth,
            BatchTrigger::Preview,
        )
        .await
        .expect("execute_plan");

        assert!(rep.completed.is_empty());
        assert_eq!(rep.skipped_stale.len(), 1);
        assert_eq!(rep.skipped_stale[0].reason, StaleReason::ModifiedSincePlan);
        assert!(f1.exists());
    }

    #[tokio::test]
    async fn r_exec_04_skips_stale_item_when_source_vanished() {
        let (_d, root) = tempdir();
        let (org, auth, cls, clock, _db) = setup_executor(&root).await;
        let f1 = root.join("gone.txt");
        write_file(&f1, b"here-at-plan-time");

        let rule = make_rule(
            "r1",
            "Move Gone",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("Sorted").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[candidate("f1", &root, "gone.txt")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        std::fs::remove_file(&f1).expect("delete source before execute");

        let rep = execute_plan(
            &plan,
            None,
            ConflictPolicy::AutoRename,
            &org,
            &auth,
            BatchTrigger::Preview,
        )
        .await
        .expect("execute_plan");

        assert!(rep.completed.is_empty());
        assert_eq!(rep.skipped_stale.len(), 1);
        assert_eq!(rep.skipped_stale[0].reason, StaleReason::SourceMissing);
    }

    #[tokio::test]
    async fn r_exec_05_revalidates_destination_appeared_since_plan() {
        let (_d, root) = tempdir();
        let (org, auth, cls, clock, _db) = setup_executor(&root).await;
        let src = root.join("late_conflict.txt");
        let dst = root.join("Sorted/late_conflict.txt");
        write_file(&src, b"source-bytes");

        let rule = make_rule(
            "r1",
            "Move Late",
            1,
            all_empty(),
            vec![Action::Move {
                destination: root.join("Sorted").to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        // At plan time, `Sorted/late_conflict.txt` does NOT exist.
        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[candidate("f1", &root, "late_conflict.txt")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );
        assert!(!plan.items[0].has_destination_exists_on_disk());

        // Now create `Sorted/late_conflict.txt` before execution.
        write_file(&dst, b"occupied-after-plan");

        // With `ConflictPolicy::Skip`, execution detects the newly occupied destination and skips safely.
        let rep = execute_plan(
            &plan,
            None,
            ConflictPolicy::Skip,
            &org,
            &auth,
            BatchTrigger::Preview,
        )
        .await
        .expect("execute_plan");

        assert!(rep.completed.is_empty());
        assert_eq!(rep.skipped_stale.len(), 1);
        assert_eq!(
            rep.skipped_stale[0].reason,
            StaleReason::DestinationOccupiedSincePlan
        );
        assert_eq!(std::fs::read(&src).expect("src intact"), b"source-bytes");
        assert_eq!(
            std::fs::read(&dst).expect("dst intact"),
            b"occupied-after-plan"
        );
    }

    struct AncestorDeniedFs {
        denied_root: PathBuf,
    }

    impl ReadOnlyFs for AncestorDeniedFs {
        fn stat(&self, path: &Path) -> std::io::Result<crate::engine::fs_view::FileFacts> {
            if nfc(path) == nfc(&self.denied_root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated EACCES",
                ));
            }
            if nfc(path).starts_with(nfc(&self.denied_root)) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "nested uncreated subdir",
                ));
            }
            RealFs.stat(path)
        }

        fn read_header(&self, path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
            RealFs.read_header(path, max)
        }
    }

    #[test]
    fn bug_024_plan_detects_ancestor_permission_denied() {
        let (_d, root) = tempdir();
        write_file(&root.join("doc.txt"), b"hello");
        let locked_root = root.join("LockedAncestor");
        mkdir(&locked_root);
        let deep_dst = locked_root.join("MissingChild").join("2026");

        let auth = make_authority(&[&root]);
        let cls = make_classifier();
        let clock = FixedClock::utc(1_700_000_000_000);
        let rule = make_rule(
            "r-deep-perm",
            "Deep Perm",
            1,
            leaf_text(TextFieldKind::Name, TextOp::Equals, "doc.txt"),
            vec![Action::Move {
                destination: deep_dst.to_string_lossy().into_owned(),
            }],
            ExecutionMode::FirstMatch,
        );

        let mock_fs = AncestorDeniedFs {
            denied_root: locked_root,
        };
        let plan = build_execution_plan(
            &PlanContext {
                fs: &mock_fs,
                authority: &auth,
                classifier: &cls,
                clock: &clock,
            },
            &[rule],
            &[candidate("f1", &root, "doc.txt")],
            &PlanScope::default(),
            &PlanEnv::default(),
        );

        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].status, PlanItemStatus::Blocked);
        assert!(
            plan.items[0]
                .conflicts
                .iter()
                .any(|c| c.kind == PlanConflictKind::PermissionDenied),
            "BUG-024b: expected PermissionDenied walking ancestor; got {:?}",
            plan.items[0].conflicts
        );
    }
}
