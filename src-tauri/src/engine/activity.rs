//! Activity log, retry, CSV export, Finder reveal resolution, and review-queue service (`P4-T10`).

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::database::repo_operations::{
    self, ActivityFilter, ActivityPage, OperationView, PageRequest, ReviewItemView,
};
use crate::database::{repo_folders, repo_rules, Db};
use crate::engine::conflicts::ConflictPolicy;
use crate::engine::operations::{Ledger, OpStatus};
use crate::engine::organizer::{OrganizeRequest, Organizer};
use crate::engine::paths::{ApprovedRoot, PathAuthority, RootKind};
use crate::engine::safe_fs::Fs;
use crate::engine::undo::{UndoReport, UndoService, UndoTarget};
use crate::error::{AppError, AppResult};
use crate::utils::clock::Clock;

/// Which path of an operation to reveal in Finder (`FR-163`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RevealTarget {
    Source,
    Destination,
}

/// User decision for a conflict item in `review_queue` (`FR-093`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    /// Keep existing destination file and place incoming file as `name (N).ext`.
    KeepBoth,
    /// Leave incoming file at source and mark review item resolved.
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResolveReport {
    pub review_id: String,
    pub decision: ReviewDecision,
    pub operation: Option<OperationView>,
}

#[derive(Clone)]
pub struct ActivityService {
    db: Db,
    organizer: Organizer,
    undo: UndoService,
}

impl ActivityService {
    pub fn new(db: Db, fs: Arc<dyn Fs>, clock: Arc<dyn Clock>) -> Self {
        let ledger = Ledger::new(db.clone());
        let organizer = Organizer::new(ledger, fs, clock);
        let undo = UndoService::new(organizer.clone());
        Self {
            db,
            organizer,
            undo,
        }
    }

    pub fn organizer(&self) -> &Organizer {
        &self.organizer
    }

    pub fn undo_service(&self) -> &UndoService {
        &self.undo
    }

    /// Builds a [`PathAuthority`] from currently registered monitored folders and destination roots.
    pub async fn authority(&self) -> AppResult<PathAuthority> {
        load_path_authority(&self.db).await
    }

    pub async fn list(&self, filter: ActivityFilter, page: PageRequest) -> AppResult<ActivityPage> {
        repo_operations::list_page(&self.db, &filter, page).await
    }

    pub async fn undo(&self, target: UndoTarget) -> AppResult<UndoReport> {
        let auth = self.authority().await?;
        self.undo.undo(target, &auth).await
    }

    pub async fn retry(&self, op_id: &str) -> AppResult<OperationView> {
        let row = self
            .organizer
            .ledger()
            .get(op_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Operation {op_id}")))?;

        if !matches!(row.status, OpStatus::Failed | OpStatus::RolledBack) {
            return Err(AppError::validation(
                "opId",
                format!(
                    "Only failed or rolled-back operations can be retried (current status: {}).",
                    row.status
                ),
            ));
        }

        let auth = self.authority().await?;
        let mut req = OrganizeRequest::new(
            row.operation_type,
            PathBuf::from(&row.source_path),
            PathBuf::from(&row.destination_path),
        );
        req.rule_id = row.rule_id.clone();
        req.conflict_policy = ConflictPolicy::AutoRename;
        req.reversible = row.reversible;

        match self.organizer.execute(req, &auth).await? {
            crate::engine::organizer::OrganizeOutcome::Completed(completed) => {
                Ok(OperationView::from(completed))
            }
            crate::engine::organizer::OrganizeOutcome::SkippedConflict(skipped) => {
                Ok(OperationView::from(skipped))
            }
            crate::engine::organizer::OrganizeOutcome::QueuedForReview { operation, .. } => {
                Ok(OperationView::from(*operation))
            }
            crate::engine::organizer::OrganizeOutcome::NoOpSamePath => Ok(OperationView::from(row)),
        }
    }

    pub async fn export_csv(&self, filter: ActivityFilter) -> AppResult<String> {
        let mut out = String::from(
            "id,started_at,completed_at,operation_type,status,original_name,new_name,source_path,destination_path,rule_name,size,content_hash,error_code,error_message\n",
        );

        // Paginate in 500-row chunks so CSV exports are never truncated at 500 rows (`BUG-020`).
        const PAGE_SIZE: u32 = 500;
        const MAX_EXPORT_ROWS: u32 = 50_000;
        let mut offset: u32 = 0;

        while offset < MAX_EXPORT_ROWS {
            let page = repo_operations::list_page(
                &self.db,
                &filter,
                PageRequest {
                    limit: PAGE_SIZE,
                    offset,
                },
            )
            .await?;

            let fetched = u32::try_from(page.items.len()).unwrap_or(0);
            if fetched == 0 {
                break;
            }

            for item in page.items {
                let cols = [
                    csv_escape(&item.id),
                    item.started_at.to_string(),
                    item.completed_at.map(|v| v.to_string()).unwrap_or_default(),
                    csv_escape(item.operation_type.as_str()),
                    csv_escape(item.status.as_str()),
                    csv_escape(&item.original_name),
                    csv_escape(&item.new_name),
                    csv_escape(&item.source_path),
                    csv_escape(&item.destination_path),
                    csv_escape(item.rule_name.as_deref().unwrap_or("")),
                    item.size.map(|v| v.to_string()).unwrap_or_default(),
                    csv_escape(item.content_hash.as_deref().unwrap_or("")),
                    csv_escape(item.error_code.as_deref().unwrap_or("")),
                    csv_escape(item.error_message.as_deref().unwrap_or("")),
                ];
                out.push_str(&cols.join(","));
                out.push('\n');
            }

            offset = offset.saturating_add(fetched);
            if offset >= page.total {
                break;
            }
        }

        Ok(out)
    }

    pub async fn clear(&self, before_ms: Option<i64>) -> AppResult<u32> {
        repo_operations::clear_history(&self.db, before_ms).await
    }

    pub async fn resolve_reveal_path(
        &self,
        op_id: &str,
        which: RevealTarget,
    ) -> AppResult<PathBuf> {
        let row = self
            .organizer
            .ledger()
            .get(op_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Operation {op_id}")))?;
        let raw = match which {
            RevealTarget::Source => &row.source_path,
            RevealTarget::Destination => &row.destination_path,
        };
        let path = PathBuf::from(raw);
        let auth = self.authority().await?;
        let authorized = auth.authorize(&path)?;
        if !self.organizer.fs().exists_no_follow(&authorized.path) {
            return Err(AppError::NotFound("File".into()));
        }
        Ok(authorized.path)
    }

    pub async fn review_list(&self) -> AppResult<Vec<ReviewItemView>> {
        repo_operations::list_pending_reviews(&self.db).await
    }

    pub async fn review_resolve(
        &self,
        id: &str,
        decision: ReviewDecision,
    ) -> AppResult<ReviewResolveReport> {
        let item = repo_operations::get_review_item(&self.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Review item {id}")))?;
        if item.resolved_at.is_some() {
            return Err(AppError::validation(
                "id",
                "This conflict has already been resolved.",
            ));
        }

        let mut executed_op = None;
        if decision == ReviewDecision::KeepBoth {
            if let Some(proposed) = &item.proposed {
                let auth = self.authority().await?;
                let mut req = OrganizeRequest::new(
                    proposed.operation_type,
                    PathBuf::from(&proposed.source_path),
                    PathBuf::from(&proposed.destination_path),
                );
                req.rule_id = item.rule_id.clone();
                req.conflict_policy = ConflictPolicy::KeepBoth;
                if let Some(done) = self.organizer.execute(req, &auth).await?.into_completed() {
                    executed_op = Some(OperationView::from(done));
                }
            }
        }

        repo_operations::mark_review_resolved(&self.db, id, self.organizer.clock().now_ms())
            .await?;

        Ok(ReviewResolveReport {
            review_id: id.to_owned(),
            decision,
            operation: executed_op,
        })
    }
}

/// Loads all approved roots (`monitored_folders` + `destination_roots`) into a [`PathAuthority`].
pub async fn load_path_authority(db: &Db) -> AppResult<PathAuthority> {
    let folders = repo_folders::list(db).await?;
    let dest_roots = repo_rules::list_destination_roots(db).await?;

    let mut roots = Vec::with_capacity(folders.len() + dest_roots.len());
    for f in folders {
        roots.push(ApprovedRoot {
            path: PathBuf::from(f.path),
            kind: RootKind::Monitored,
        });
    }
    for dr in dest_roots {
        roots.push(ApprovedRoot {
            path: PathBuf::from(dr),
            kind: RootKind::Destination,
        });
    }
    Ok(PathAuthority::new(roots))
}

use crate::utils::csv_escape;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::engine::operations::OpKind;
    use crate::engine::safe_fs::SafeFs;
    use crate::test_support::{tempdir, write_file};
    use crate::utils::clock::FixedClock;

    #[tokio::test]
    async fn activity_service_exports_csv_retries_and_resolves_reviews() {
        let (_d, root) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        repo_folders::insert(&db, "f1", "Root", root.to_str().expect("utf8"), true)
            .await
            .expect("insert folder");

        let fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
        let svc = ActivityService::new(db.clone(), fs.clone(), clock);
        let auth = svc.authority().await.expect("auth");

        let src = root.join("=cmd.txt");
        let dst = root.join("Sorted/=cmd.txt");
        write_file(&src, b"payload-1");
        write_file(&dst, b"existing");

        // 1. Trigger an `Ask` conflict to populate `review_queue` and a `ROLLED_BACK` row.
        let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
        req.conflict_policy = ConflictPolicy::Ask;
        svc.organizer().execute(req, &auth).await.expect("ask");

        let reviews = svc.review_list().await.expect("reviews");
        assert_eq!(reviews.len(), 1);

        // 2. Resolve the review with `KeepBoth` -> moves `src` to `Sorted/=cmd (1).txt`.
        let resolved = svc
            .review_resolve(&reviews[0].id, ReviewDecision::KeepBoth)
            .await
            .expect("resolve");
        assert!(resolved.operation.is_some());
        assert!(svc.review_list().await.expect("reviews").is_empty());

        // 3. Export CSV and verify formula-injection prefix `'` on `=cmd.txt`.
        let csv = svc
            .export_csv(ActivityFilter::default())
            .await
            .expect("csv");
        assert!(
            csv.contains("'=cmd.txt"),
            "CSV export must neutralize formula injection: {csv}"
        );
    }

    #[tokio::test]
    async fn bug_020_export_csv_paginates_beyond_500_rows() {
        use crate::database::repo_operations::NewOperationRow;
        use crate::engine::operations::{Ledger, OpStatus};

        let (_d, root) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        let ledger = Ledger::new(db.clone());
        let fs = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
        let svc = ActivityService::new(db.clone(), fs, clock);

        for i in 0..520u32 {
            let id = format!("op-bulk-{i:04}");
            ledger
                .record_intent(&NewOperationRow {
                    id: id.clone(),
                    batch_id: None,
                    file_record_id: None,
                    operation_type: OpKind::Move,
                    source_path: format!("/tmp/src-{i}.txt"),
                    destination_path: format!("/tmp/dst-{i}.txt"),
                    staging_path: None,
                    original_name: format!("src-{i}.txt"),
                    new_name: format!("dst-{i}.txt"),
                    dev: None,
                    ino: None,
                    size: Some(128),
                    mtime_ns: None,
                    content_hash: None,
                    cross_volume: false,
                    rule_id: None,
                    reversible: true,
                    started_at: 1_700_000_000_000 + i64::from(i),
                })
                .await
                .expect("insert");
            let _ = sqlx::query("UPDATE operations SET status = ? WHERE id = ?")
                .bind(OpStatus::Completed.as_str())
                .bind(&id)
                .execute(&db)
                .await
                .expect("complete");
        }

        let csv = svc
            .export_csv(ActivityFilter::default())
            .await
            .expect("export_csv");
        // Header + 520 data rows = 521 non-empty lines.
        let line_count = csv.lines().count();
        assert_eq!(
            line_count, 521,
            "expected 1 header + 520 rows, got {line_count}"
        );
    }
}
