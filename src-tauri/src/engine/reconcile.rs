//! Reconciliation scans (FR-014, FR-022).
//!
//! Filesystem watcher events are never assumed to be complete. A reconcile
//! re-scans the folder, upserts what it finds, removes records of files that
//! are gone, and reports new/changed files so they enter the normal
//! stability → processing path.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::database::{repo_files, repo_folders, Db};
use crate::engine::classifier::Classifier;
use crate::engine::ignore::IgnoreRules;
use crate::engine::paths::PathError;
use crate::engine::scanner::{scan, ScanError, ScanOptions, ScanSummary, ScannedFile};
use crate::error::{AppError, AppResult};
use crate::folders::MonitoredFolder;
use crate::settings::MonitoringSettings;
use crate::utils::time::now_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileMode {
    /// First scan of a folder: record what exists, report nothing.
    Baseline,
    /// Report files that are new or changed since the last scan.
    Diff,
}

#[derive(Debug, Clone)]
pub struct FolderScanSpec {
    pub id: String,
    pub path: PathBuf,
    pub recursive: bool,
    pub rules: IgnoreRules,
}

impl FolderScanSpec {
    pub fn from_folder(f: &MonitoredFolder, m: &MonitoringSettings) -> Self {
        Self {
            id: f.id.clone(),
            path: PathBuf::from(&f.path),
            recursive: f.recursive,
            rules: IgnoreRules {
                ignore_hidden: m.ignore_hidden_files,
                ignore_temporary: m.ignore_temporary_files,
                include_extensions: f.settings.include_extensions.clone(),
                exclude_extensions: f.settings.exclude_extensions.clone(),
                excluded_dirs: f
                    .settings
                    .excluded_subfolders
                    .iter()
                    .map(PathBuf::from)
                    .collect(),
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub summary: ScanSummary,
    pub changed: u64,
    pub removed: u64,
}

impl From<ScanError> for AppError {
    fn from(e: ScanError) -> Self {
        match e {
            ScanError::NotFound => PathError::NotFound.into(),
            ScanError::PermissionDenied => PathError::PermissionDenied.into(),
            ScanError::Io(m) => PathError::Io(m).into(),
            ScanError::Sink(m) => AppError::Internal(format!("scan aborted: {m}")),
        }
    }
}

/// Scans on a blocking thread and persists batches as they arrive (bounded
/// channel → backpressure, so memory stays flat for any folder size).
pub async fn reconcile(
    db: &Db,
    spec: &FolderScanSpec,
    classifier: Arc<Classifier>,
    mode: ReconcileMode,
    on_candidate: &mut (dyn FnMut(PathBuf) + Send),
) -> AppResult<ReconcileReport> {
    let started = repo_files::next_scan_stamp(db, &spec.id, now_ms()).await?;
    let (tx, mut rx) = mpsc::channel::<Vec<ScannedFile>>(2);
    let root = spec.path.clone();
    let opts = ScanOptions {
        recursive: spec.recursive,
        rules: spec.rules.clone(),
    };
    let scan_task = tokio::task::spawn_blocking(move || {
        scan(&root, &opts, &classifier, |batch| {
            tx.blocking_send(batch)
                .map_err(|_| "persistence stopped".to_owned())
        })
    });

    let mut changed = 0u64;
    let mut persist_err = None;
    while let Some(batch) = rx.recv().await {
        match repo_files::upsert_batch(db, &spec.id, &batch, started).await {
            Ok(paths) => {
                changed += paths.len() as u64;
                if mode == ReconcileMode::Diff {
                    paths.into_iter().for_each(&mut *on_candidate);
                }
            }
            Err(e) => {
                persist_err = Some(e);
                break; // dropping rx stops the scanner
            }
        }
    }
    drop(rx);
    let scanned = scan_task
        .await
        .map_err(|e| AppError::Internal(format!("scan task: {e}")))?;
    if let Some(e) = persist_err {
        return Err(e);
    }
    let summary = scanned?;
    // Only a complete scan may remove records of unseen files. If any part of
    // the folder was unreadable, keep the old records: deleting them would
    // make those files look new once they are readable again (BUG-008).
    let removed = if summary.errors == 0 {
        repo_files::delete_not_seen(db, &spec.id, started).await?
    } else {
        tracing::warn!(target: "folderflow::reconcile", folder_id = %spec.id, errors = summary.errors,
            "incomplete scan; keeping existing records");
        0
    };
    repo_folders::record_scan(db, &spec.id, started).await?;
    if mode == ReconcileMode::Baseline {
        changed = 0;
    }
    tracing::debug!(target: "folderflow::reconcile", folder_id = %spec.id, files = summary.files, changed, removed, "reconciled");
    Ok(ReconcileReport {
        summary,
        changed,
        removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::builtin_categories;
    use crate::database;
    use crate::engine::paths::RootPolicy;
    use crate::folders::FolderService;
    use crate::test_support::{magic, mkdir, remove_file, tempdir, write_file};
    use std::path::Path;

    async fn setup(db: &Db, base: &Path) -> FolderScanSpec {
        let dl = base.join("Downloads");
        mkdir(&dl);
        let svc = FolderService::new(
            db.clone(),
            RootPolicy {
                home: base.join("home"),
                app_data_dir: base.join("appdata"),
            },
        );
        let f = match svc.list().await.expect("list").into_iter().next() {
            Some(f) => f,
            None => svc.add(&dl, true).await.expect("add"),
        };
        FolderScanSpec::from_folder(&f, &MonitoringSettings::default())
    }

    async fn run(
        db: &Db,
        spec: &FolderScanSpec,
        mode: ReconcileMode,
    ) -> (ReconcileReport, Vec<String>) {
        let mut got = Vec::new();
        let r = reconcile(
            db,
            spec,
            Arc::new(Classifier::new(builtin_categories())),
            mode,
            &mut |p: PathBuf| {
                got.push(p.file_name().expect("name").to_string_lossy().into_owned());
            },
        )
        .await
        .expect("reconcile");
        got.sort();
        (r, got)
    }

    async fn record_count(db: &Db) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM file_records")
            .fetch_one(db)
            .await
            .expect("count")
    }

    #[tokio::test]
    async fn r_recon_01_baseline_reports_nothing_then_missed_new_file_is_found() {
        let (_d, base) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        write_file(&base.join("Downloads/a.pdf"), magic::PDF);
        write_file(&base.join("Downloads/sub/b.png"), magic::PNG);
        let spec = setup(&db, &base).await;

        let (r, got) = run(&db, &spec, ReconcileMode::Baseline).await;
        assert!(got.is_empty(), "baseline must not report existing files");
        assert_eq!((r.summary.files, r.changed, r.removed), (2, 0, 0));
        assert_eq!(record_count(&db).await, 2);
        let (cat, conf, mime): (String, String, Option<String>) = sqlx::query_as(
            "SELECT category, confidence, mime FROM file_records WHERE filename = 'a.pdf'",
        )
        .fetch_one(&db)
        .await
        .expect("row");
        assert_eq!(
            (cat.as_str(), conf.as_str(), mime.as_deref()),
            ("documents", "high", Some("application/pdf"))
        );

        // A file created while no watcher event was delivered.
        write_file(&base.join("Downloads/new.zip"), magic::ZIP);
        let (r, got) = run(&db, &spec, ReconcileMode::Diff).await;
        assert_eq!(got, vec!["new.zip"]);
        assert_eq!(r.changed, 1);
        assert_eq!(record_count(&db).await, 3);
        let scanned: Option<i64> =
            sqlx::query_scalar("SELECT last_scanned_at FROM monitored_folders")
                .fetch_one(&db)
                .await
                .expect("ts");
        assert!(scanned.is_some());
    }

    #[tokio::test]
    async fn r_recon_02_changed_files_reported_deleted_records_removed_hash_invalidated() {
        let (_d, base) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        write_file(&base.join("Downloads/keep.pdf"), magic::PDF);
        write_file(&base.join("Downloads/grow.txt"), b"one");
        write_file(&base.join("Downloads/gone.txt"), b"bye");
        let spec = setup(&db, &base).await;
        run(&db, &spec, ReconcileMode::Baseline).await;
        sqlx::query("UPDATE file_records SET content_hash = 'abc', hash_size = size, hash_mtime_ns = modified_at_ns").execute(&db).await.expect("hash");

        write_file(&base.join("Downloads/grow.txt"), b"one two three");
        remove_file(&base.join("Downloads/gone.txt"));
        let (r, got) = run(&db, &spec, ReconcileMode::Diff).await;
        assert_eq!(got, vec!["grow.txt"]);
        assert_eq!((r.changed, r.removed), (1, 1));
        let hashes: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT filename, content_hash FROM file_records ORDER BY filename")
                .fetch_all(&db)
                .await
                .expect("rows");
        assert_eq!(
            hashes,
            vec![
                ("grow.txt".into(), None),
                ("keep.pdf".into(), Some("abc".into()))
            ]
        );
    }

    #[tokio::test]
    async fn r_recon_03_changes_made_while_app_was_closed_are_found_after_restart() {
        let (_d, base) = tempdir();
        let db_path = base.join("ff.db");
        write_file(&base.join("Downloads/a.pdf"), magic::PDF);
        {
            let db = database::open(&db_path).await.expect("db");
            let spec = setup(&db, &base).await;
            run(&db, &spec, ReconcileMode::Baseline).await;
            db.close().await;
        }
        write_file(&base.join("Downloads/while-closed.png"), magic::PNG);
        let db = database::open(&db_path).await.expect("reopen");
        let spec = setup(&db, &base).await;
        let (_, got) = run(&db, &spec, ReconcileMode::Diff).await;
        assert_eq!(got, vec!["while-closed.png"]);
    }

    /// BUG-013: scan stamps must always increase, even if two scans start in
    /// the same millisecond or the clock moves backwards; otherwise records of
    /// deleted files are never removed.
    #[tokio::test]
    async fn bug_013_scan_stamps_increase_even_if_the_clock_goes_backwards() {
        let (_d, base) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        write_file(&base.join("Downloads/keep.pdf"), magic::PDF);
        write_file(&base.join("Downloads/gone.txt"), b"bye");
        let spec = setup(&db, &base).await;
        run(&db, &spec, ReconcileMode::Baseline).await;
        // Simulate the previous scan having happened "in the future" (clock moved back since).
        sqlx::query("UPDATE file_records SET last_scanned_at = last_scanned_at + 3600000")
            .execute(&db)
            .await
            .expect("shift");
        remove_file(&base.join("Downloads/gone.txt"));
        let (r, _) = run(&db, &spec, ReconcileMode::Diff).await;
        assert_eq!(r.removed, 1);
        assert_eq!(record_count(&db).await, 1);
        // Back-to-back scans with nothing deleted remove nothing.
        let (r, _) = run(&db, &spec, ReconcileMode::Diff).await;
        assert_eq!(r.removed, 0);
        assert_eq!(record_count(&db).await, 1);
    }

    /// BUG-008: a subfolder that is temporarily unreadable must not have its
    /// records deleted — otherwise its old files later look "new".
    #[tokio::test]
    async fn bug_008_scan_errors_do_not_delete_records_or_resurface_old_files() {
        let (_d, base) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        write_file(&base.join("Downloads/top.pdf"), magic::PDF);
        write_file(&base.join("Downloads/sub/old.pdf"), magic::PDF);
        let spec = setup(&db, &base).await;
        run(&db, &spec, ReconcileMode::Baseline).await;

        let sub = base.join("Downloads/sub");
        crate::test_support::set_mode(&sub, 0o000);
        let (r, got) = run(&db, &spec, ReconcileMode::Diff).await;
        crate::test_support::set_mode(&sub, 0o755);
        assert!(r.summary.errors > 0);
        assert_eq!(r.removed, 0, "incomplete scan must not delete records");
        assert!(got.is_empty());
        assert_eq!(record_count(&db).await, 2);

        let (_, got) = run(&db, &spec, ReconcileMode::Diff).await;
        assert!(got.is_empty(), "old.pdf is not new: {got:?}");
    }

    #[tokio::test]
    async fn r_recon_04_spec_reflects_folder_and_global_settings() {
        let (_d, base) = tempdir();
        let db = database::open_in_memory().await.expect("db");
        let spec = setup(&db, &base).await;
        assert!(spec.recursive);
        assert!(spec.rules.ignore_hidden && spec.rules.ignore_temporary);
    }
}
