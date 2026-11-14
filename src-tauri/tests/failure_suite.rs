//! Spec §24.3 Failure Suite (`R-FAIL-01..12`, `P8-T02`, `TESTING_STRATEGY.md`).
//!
//! Verifies every filesystem, database, config, watcher, and concurrency failure
//! mode obeys `SAFETY_MODEL.md` S1–S11: zero data loss, zero overwrite, atomic
//! recovery, strict error reporting, and bounded backpressure.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use folderflow_lib::categories::builtin_categories;
use folderflow_lib::config_io::{apply_config_import, preview_config_import, ConfigImportMode};
use folderflow_lib::database::repo_operations::NewOperationRow;
use folderflow_lib::database::{self, repo_folders, repo_rules, Db};
use folderflow_lib::engine::classifier::{Classifier, Confidence};
use folderflow_lib::engine::conflicts::ConflictPolicy;
use folderflow_lib::engine::debounce::Debouncer;
use folderflow_lib::engine::fs_view::{EntryKind, FileFacts, ReadOnlyFs, RealFs};
use folderflow_lib::engine::monitor::{
    FolderRuntime, MonitorDeps, MonitorObserver, MonitorService, MonitorTiming, RawEvent,
    RuntimeState, WatcherFactory,
};
use folderflow_lib::engine::operations::{Ledger, OpKind, OpStatus};
use folderflow_lib::engine::organizer::{OrganizeOutcome, OrganizeRequest, Organizer};
use folderflow_lib::engine::paths::{ApprovedRoot, PathAuthority, RootKind, RootPolicy};
use folderflow_lib::engine::pipeline::{AutomationPipeline, EnqueueResult};
use folderflow_lib::engine::recovery::recover_startup;
use folderflow_lib::engine::safe_fs::fault::{FaultKind, FaultyFs, FsOpKind};
use folderflow_lib::engine::safe_fs::{Fs, SafeFs};
use folderflow_lib::engine::scanner::ScannedFile;
use folderflow_lib::engine::stability::{Observation, StabilityEvent, StabilityTracker};
use folderflow_lib::folders::{FolderHealth, FolderSettings, MonitoredFolder};
use folderflow_lib::settings::MonitoringSettings;
use folderflow_lib::utils::clock::{Clock, FixedClock};
use tempfile::TempDir;

fn write_new_file(path: &Path, data: &[u8]) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).expect("create parent dirs");
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("open new fixture file");
    f.write_all(data).expect("write fixture bytes");
    f.flush().expect("flush fixture");
}

#[allow(clippy::disallowed_methods)]
fn overwrite_fixture(path: &Path, data: &[u8]) {
    // Test-only helper used to simulate external writes / active downloads / corruption.
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).expect("create parent dirs");
    }
    std::fs::write(path, data).expect("overwrite fixture");
}

#[allow(clippy::disallowed_methods)]
fn remove_fixture_file(path: &Path) {
    // Test-only helper simulating an external user deletion before execution.
    std::fs::remove_file(path).expect("remove fixture file");
}

struct ModeGuard {
    path: PathBuf,
    orig_mode: u32,
}

impl ModeGuard {
    #[allow(clippy::disallowed_methods)]
    fn set(path: &Path, mode: u32) -> Self {
        let orig_mode = std::fs::metadata(path)
            .expect("stat dir before chmod")
            .permissions()
            .mode();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod dir");
        Self {
            path: path.to_path_buf(),
            orig_mode,
        }
    }
}

impl Drop for ModeGuard {
    #[allow(clippy::disallowed_methods)]
    fn drop(&mut self) {
        let _ =
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.orig_mode));
    }
}

async fn setup_roots(
    root: &Path,
) -> (
    Db,
    PathBuf,
    PathBuf,
    PathAuthority,
    Arc<SafeFs>,
    Arc<FixedClock>,
) {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let src_dir = root.join("src");
    let dst_dir = root.join("dst");
    let trash_dir = root.join(".Trash");
    std::fs::create_dir_all(&src_dir).expect("mkdir src");
    std::fs::create_dir_all(&dst_dir).expect("mkdir dst");
    std::fs::create_dir_all(&trash_dir).expect("mkdir trash");

    let db = database::open_in_memory().await.expect("open db");
    repo_folders::insert(&db, "f1", "Src", &src_dir.to_string_lossy(), false)
        .await
        .expect("insert folder");
    repo_rules::upsert_destination_root(&db, "dr1", &dst_dir.to_string_lossy())
        .await
        .expect("insert dest root");

    let authority = PathAuthority::new(vec![
        ApprovedRoot {
            path: src_dir.clone(),
            kind: RootKind::Monitored,
        },
        ApprovedRoot {
            path: dst_dir.clone(),
            kind: RootKind::Destination,
        },
    ]);
    let safe_fs = Arc::new(SafeFs::with_trash_dir(trash_dir));
    let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
    (db, src_dir, dst_dir, authority, safe_fs, clock)
}

#[derive(Default)]
struct RecordingObserver {
    states: Mutex<Vec<FolderRuntime>>,
    stable_paths: Mutex<Vec<PathBuf>>,
}

impl RecordingObserver {
    fn last_state(&self) -> Option<RuntimeState> {
        self.states
            .lock()
            .expect("lock states")
            .last()
            .map(|r| r.state)
    }

    fn last_error(&self) -> Option<String> {
        self.states
            .lock()
            .expect("lock states")
            .last()
            .and_then(|r| r.last_error.clone())
    }
}

impl MonitorObserver for RecordingObserver {
    fn on_status(&self, status: &FolderRuntime) {
        self.states
            .lock()
            .expect("lock states")
            .push(status.clone());
    }

    fn on_stable(&self, _folder_id: &str, file: &ScannedFile) {
        self.stable_paths
            .lock()
            .expect("lock stable")
            .push(PathBuf::from(&file.facts.path));
    }

    fn on_removed(&self, _folder_id: &str, _path: &Path) {}
}

#[tokio::test]
async fn r_fail_01_interrupt_move_preserves_source_and_recovers_cleanly() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    let src = src_dir.join("tax_return.pdf");
    let dst = dst_dir.join("tax_return.pdf");
    write_new_file(&src, b"critical-tax-document");

    // 1. Cross-volume staged move interrupted mid-copy (EIO partial copy).
    let faulty = Arc::new(FaultyFs::new(safe_fs.clone()));
    faulty.simulate_cross_volume(src_dir.clone(), dst_dir.clone());
    faulty.fail_next_op(
        FsOpKind::CopyExcl,
        FaultKind::PartialCopy {
            bytes: 8,
            errno: libc::EIO,
        },
    );

    let ledger = Ledger::new(db.clone());
    let organizer = Organizer::new(ledger.clone(), faulty, clock.clone());
    let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
    req.op_id = Some("fail-01-xv".into());

    let err = organizer
        .execute(req, &authority)
        .await
        .expect_err("interrupted cross-volume move must error");
    assert!(matches!(err, folderflow_lib::error::AppError::Io(_)));
    assert_eq!(
        std::fs::read(&src).expect("src intact"),
        b"critical-tax-document"
    );
    assert!(
        !dst.exists(),
        "partial staging must never become destination"
    );
    let row = ledger.get("fail-01-xv").await.expect("get").expect("row");
    assert_eq!(row.status, OpStatus::RolledBack);

    // 2. Same-volume move interrupted mid-rename leaves IN_PROGRESS row -> startup recovery rolls back safely.
    let crashed_op = NewOperationRow {
        id: "fail-01-crash".into(),
        batch_id: None,
        file_record_id: None,
        operation_type: OpKind::Move,
        source_path: src.to_string_lossy().into_owned(),
        destination_path: dst.to_string_lossy().into_owned(),
        staging_path: None,
        original_name: "tax_return.pdf".into(),
        new_name: "tax_return.pdf".into(),
        dev: None,
        ino: None,
        size: None,
        mtime_ns: None,
        content_hash: None,
        cross_volume: false,
        rule_id: None,
        reversible: true,
        started_at: clock.now_ms(),
    };
    ledger.record_intent(&crashed_op).await.expect("intent");
    sqlx::query("UPDATE operations SET status = 'IN_PROGRESS' WHERE id = ?")
        .bind("fail-01-crash")
        .execute(&db)
        .await
        .expect("force IN_PROGRESS");

    let rec = recover_startup(
        &ledger,
        safe_fs.as_ref(),
        clock.as_ref(),
        &[src_dir, dst_dir],
    )
    .await
    .expect("recover startup");
    assert!(rec.rolled_back.contains(&"fail-01-crash".to_owned()));
    assert!(src.exists());
}

#[tokio::test]
async fn r_fail_02_interrupt_copy_trashes_partial_staging_and_leaves_source_untouched() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    let src = src_dir.join("master.zip");
    let dst = dst_dir.join("master.zip");
    write_new_file(&src, b"zip-payload-1234567890");

    let faulty = Arc::new(FaultyFs::new(safe_fs.clone()));
    faulty.fail_next_op(
        FsOpKind::CopyExcl,
        FaultKind::PartialCopy {
            bytes: 6,
            errno: libc::EIO,
        },
    );

    let ledger = Ledger::new(db.clone());
    let organizer = Organizer::new(ledger.clone(), faulty, clock.clone());
    let mut req = OrganizeRequest::new(OpKind::Copy, src.clone(), dst.clone());
    req.op_id = Some("fail-02-copy".into());

    let _ = organizer
        .execute(req, &authority)
        .await
        .expect_err("interrupted copy fails cleanly");

    assert_eq!(
        std::fs::read(&src).expect("src intact"),
        b"zip-payload-1234567890"
    );
    assert!(
        !dst.exists(),
        "incomplete copy must never appear at final destination"
    );
    let row = ledger.get("fail-02-copy").await.expect("get").expect("row");
    assert_eq!(row.status, OpStatus::RolledBack);

    // Also simulate crash mid-IN_PROGRESS copy with an orphan staging file on disk.
    let partial = dst_dir.join(".folderflow-fail-02-crash.partial");
    write_new_file(&partial, b"partial-bytes");
    ledger
        .record_intent(&NewOperationRow {
            id: "fail-02-crash".into(),
            batch_id: None,
            file_record_id: None,
            operation_type: OpKind::Copy,
            source_path: src.to_string_lossy().into_owned(),
            destination_path: dst.to_string_lossy().into_owned(),
            staging_path: Some(partial.to_string_lossy().into_owned()),
            original_name: "master.zip".into(),
            new_name: "master.zip".into(),
            dev: None,
            ino: None,
            size: None,
            mtime_ns: None,
            content_hash: None,
            cross_volume: true,
            rule_id: None,
            reversible: true,
            started_at: clock.now_ms(),
        })
        .await
        .expect("intent");
    sqlx::query("UPDATE operations SET status = 'IN_PROGRESS' WHERE id = ?")
        .bind("fail-02-crash")
        .execute(&db)
        .await
        .expect("force IN_PROGRESS");

    let rec = recover_startup(
        &ledger,
        safe_fs.as_ref(),
        clock.as_ref(),
        &[src_dir, dst_dir],
    )
    .await
    .expect("recover startup");
    assert!(rec.rolled_back.contains(&"fail-02-crash".to_owned()));
    assert!(!partial.exists(), "partial staging must be moved to Trash");
    assert!(!dst.exists());
    assert!(src.exists());
}

#[tokio::test]
async fn r_fail_03_corrupted_db_backed_up_and_ledger_failure_prevents_fs_mutation() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("folderflow.db");
    let junk = b"NOT A SQLITE DATABASE - CORRUPT BYTES";
    write_new_file(&db_path, junk);
    write_new_file(&dir.path().join("folderflow.db-wal"), b"corrupt-wal");

    // 1. Corrupted database file is backed up (not overwritten/deleted) and cleanly recreated.
    let fresh_db = database::open(&db_path)
        .await
        .expect("open recovers corrupt db");
    let backups: Vec<_> = std::fs::read_dir(dir.path())
        .expect("readdir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("folderflow.db.corrupt-") && n.ends_with(".bak"))
        .collect();
    assert_eq!(
        backups.len(),
        1,
        "expected exactly 1 backup file: {backups:?}"
    );
    assert_eq!(
        std::fs::read(dir.path().join(&backups[0])).expect("read backup"),
        junk
    );
    assert!(repo_folders::list(&fresh_db).await.is_ok());

    // 2. Closed/unavailable DB aborts write-ahead ledger before ANY mutating FS call executes (S4).
    let src_dir = dir.path().join("src");
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir_all(&src_dir).expect("mkdir");
    std::fs::create_dir_all(&dst_dir).expect("mkdir");
    let src = src_dir.join("ledger_guard.txt");
    let dst = dst_dir.join("ledger_guard.txt");
    write_new_file(&src, b"untouched");

    let authority = PathAuthority::new(vec![
        ApprovedRoot {
            path: src_dir,
            kind: RootKind::Monitored,
        },
        ApprovedRoot {
            path: dst_dir,
            kind: RootKind::Destination,
        },
    ]);
    let faulty = Arc::new(FaultyFs::new(Arc::new(SafeFs::with_trash_dir(
        dir.path().join(".Trash"),
    ))));
    let clock = Arc::new(FixedClock::utc(1_700_000_000_000));
    fresh_db.close().await;

    let organizer = Organizer::new(Ledger::new(fresh_db), faulty.clone(), clock);
    let err = organizer
        .execute(
            OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
            &authority,
        )
        .await
        .expect_err("closed DB must fail before mutating FS");
    assert!(!err.to_string().is_empty());
    assert_eq!(
        faulty.step_count(),
        0,
        "S4 invariant: zero mutating Fs ops allowed when ledger write fails"
    );
    assert!(src.exists() && !dst.exists());
}

#[tokio::test]
async fn r_fail_04_destination_unavailable_leaves_source_intact() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, _dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    let src = src_dir.join("report.pdf");
    write_new_file(&src, b"important-report");

    // 1. Unapproved external destination is rejected cleanly by PathAuthority.
    let external_dst = dir.path().join("unmounted_drive/report.pdf");
    let ledger = Ledger::new(db.clone());
    let organizer = Organizer::new(ledger.clone(), safe_fs.clone(), clock.clone());
    let err = organizer
        .execute(
            OrganizeRequest::new(OpKind::Move, src.clone(), external_dst.clone()),
            &authority,
        )
        .await
        .expect_err("unauthorized destination must fail");
    assert!(!err.to_string().is_empty());
    assert!(src.exists());
    assert!(!external_dst.exists());

    // 2. Destination mkdir failure (e.g. ENOENT / read-only volume) rolls back cleanly.
    let faulty = Arc::new(FaultyFs::new(safe_fs));
    faulty.fail_next_op(FsOpKind::MkdirP, FaultKind::Errno(libc::EROFS));
    let organizer2 = Organizer::new(ledger, faulty, clock);
    let dst = dir.path().join("dst/offline/report.pdf");
    let _ = organizer2
        .execute(
            OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
            &authority,
        )
        .await
        .expect_err("EROFS destination must fail");
    assert_eq!(
        std::fs::read(&src).expect("src intact"),
        b"important-report"
    );
    assert!(!dst.exists());
}

#[tokio::test]
async fn r_fail_05_source_vanishes_before_execution_and_in_stability_tracker() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    let src = src_dir.join("ephemeral.txt");
    let dst = dst_dir.join("ephemeral.txt");
    write_new_file(&src, b"ephemeral");

    // Capture S6 expected identity, then delete source before execution.
    let pre = safe_fs.lstat(&src).expect("lstat src");
    remove_fixture_file(&src);

    let ledger = Ledger::new(db);
    let organizer = Organizer::new(ledger.clone(), safe_fs, clock);
    let mut req = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
    req.op_id = Some("fail-05-vanished".into());
    req.expected_source = Some(pre);

    let err = organizer
        .execute(req, &authority)
        .await
        .expect_err("vanished source must return clean error");
    assert!(!err.to_string().is_empty());
    assert!(!dst.exists());
    let row = ledger
        .get("fail-05-vanished")
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, OpStatus::Failed);

    // StabilityTracker also emits Removed on Missing observation without panicking.
    let mut tracker = StabilityTracker::new(Duration::from_millis(10), 2, 16);
    let t0 = Instant::now();
    assert!(tracker.track(src.clone(), t0));
    assert_eq!(
        tracker.observe(&src, Observation::Missing, t0 + Duration::from_millis(20)),
        Some(StabilityEvent::Removed(src))
    );
}

#[tokio::test]
async fn r_fail_06_destination_appears_mid_execution_never_overwrites() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    let src = src_dir.join("contract.pdf");
    let dst = dst_dir.join("contract.pdf");
    write_new_file(&src, b"incoming-contract");

    // Race injector: right when MkdirP executes (after preflight, right before RenameExcl),
    // another process writes an unrelated file at `dst`.
    let faulty = Arc::new(FaultyFs::new(safe_fs.clone()));
    let race_dst = dst.clone();
    let injected = Arc::new(AtomicBool::new(false));
    let injected_flag = injected.clone();
    faulty.set_on_mutation(move |op, _step| {
        if op == FsOpKind::MkdirP && !injected_flag.swap(true, Ordering::SeqCst) {
            write_new_file(&race_dst, b"racing-existing-file");
        }
    });

    // Under Skip policy, `RENAME_EXCL` detects the racing destination and leaves BOTH intact.
    let organizer = Organizer::new(Ledger::new(db.clone()), faulty.clone(), clock.clone());
    let mut req_skip = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
    req_skip.conflict_policy = ConflictPolicy::Skip;
    let outcome = organizer
        .execute(req_skip, &authority)
        .await
        .expect("skip outcome");
    assert!(matches!(outcome, OrganizeOutcome::SkippedConflict(_)));
    assert_eq!(
        std::fs::read(&dst).expect("dst preserved"),
        b"racing-existing-file"
    );
    assert_eq!(
        std::fs::read(&src).expect("src preserved"),
        b"incoming-contract"
    );

    // Under AutoRename policy against a second racing file, it claims `contract (1).pdf` safely.
    let src2 = src_dir.join("contract2.pdf");
    let dst2 = dst_dir.join("contract2.pdf");
    write_new_file(&src2, b"second-contract");
    let injected2 = Arc::new(AtomicBool::new(false));
    let race_dst2 = dst2.clone();
    faulty.set_on_mutation(move |op, _step| {
        if op == FsOpKind::MkdirP && !injected2.swap(true, Ordering::SeqCst) {
            write_new_file(&race_dst2, b"winner-at-dst2");
        }
    });
    let mut req_rename = OrganizeRequest::new(OpKind::Move, src2.clone(), dst2.clone());
    req_rename.conflict_policy = ConflictPolicy::AutoRename;
    let done = organizer
        .execute(req_rename, &authority)
        .await
        .expect("autorename outcome")
        .into_completed()
        .expect("completed");
    assert_eq!(
        std::fs::read(&dst2).expect("dst2 untouched"),
        b"winner-at-dst2"
    );
    assert_eq!(
        std::fs::read(&done.destination_path).expect("renamed destination"),
        b"second-contract"
    );
    assert_ne!(done.destination_path, dst2.to_string_lossy());
}

#[tokio::test]
async fn r_fail_07_enospc_disk_full_aborts_cleanly_without_loss() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    let src = src_dir.join("video.mov");
    let dst = dst_dir.join("video.mov");
    write_new_file(&src, b"movie-stream-bytes");

    let ledger = Ledger::new(db);
    let faulty = Arc::new(FaultyFs::new(safe_fs));

    // 1. Pre-flight cross-volume free-space check catches insufficient disk (< 2x file size).
    faulty.simulate_cross_volume(src_dir.clone(), dst_dir.clone());
    faulty.set_free_bytes(Some(10));
    let organizer = Organizer::new(ledger.clone(), faulty.clone(), clock.clone());
    let mut req1 = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
    req1.op_id = Some("fail-07-preflight".into());
    let err1 = organizer
        .execute(req1, &authority)
        .await
        .expect_err("low free space must abort");
    assert!(!err1.to_string().is_empty());
    assert!(src.exists() && !dst.exists());

    // 2. Runtime ENOSPC during CopyExcl cleans up partial staging and preserves source.
    faulty.set_free_bytes(None);
    faulty.fail_next_op(
        FsOpKind::CopyExcl,
        FaultKind::PartialCopy {
            bytes: 4,
            errno: libc::ENOSPC,
        },
    );
    let mut req2 = OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone());
    req2.op_id = Some("fail-07-runtime".into());
    let err2 = organizer
        .execute(req2, &authority)
        .await
        .expect_err("runtime ENOSPC must abort");
    assert!(
        err2.to_string().to_ascii_lowercase().contains("space")
            || matches!(err2, folderflow_lib::error::AppError::Io(_))
    );
    assert_eq!(
        std::fs::read(&src).expect("src preserved"),
        b"movie-stream-bytes"
    );
    assert!(!dst.exists());
    let row = ledger
        .get("fail-07-runtime")
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, OpStatus::RolledBack);
}

#[tokio::test]
async fn r_fail_08_eacces_permission_denied_handled_safely() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    let locked_dst_dir = dst_dir.join("locked");
    std::fs::create_dir_all(&locked_dst_dir).expect("mkdir locked");
    let src = src_dir.join("secret.pdf");
    let dst = locked_dst_dir.join("secret.pdf");
    write_new_file(&src, b"confidential");

    // Fault-injected EACCES during RenameExcl
    let faulty = Arc::new(FaultyFs::new(safe_fs.clone()));
    faulty.fail_next_op(FsOpKind::RenameExcl, FaultKind::Errno(libc::EACCES));
    let organizer = Organizer::new(Ledger::new(db.clone()), faulty, clock);
    let err = organizer
        .execute(
            OrganizeRequest::new(OpKind::Move, src.clone(), dst.clone()),
            &authority,
        )
        .await
        .expect_err("EACCES must fail");
    assert!(matches!(
        err,
        folderflow_lib::error::AppError::Io(ref e) if e.kind() == ErrorKind::PermissionDenied
    ));
    assert!(src.exists() && !dst.exists());

    // Real OS 0o555 read-only directory test (restored automatically on drop)
    let _guard = ModeGuard::set(&locked_dst_dir, 0o555);
    let organizer_real = Organizer::new(
        Ledger::new(db),
        safe_fs,
        Arc::new(FixedClock::utc(1_700_000_000_000)),
    );
    let real_err = organizer_real
        .execute(
            OrganizeRequest::new(OpKind::Move, src.clone(), dst),
            &authority,
        )
        .await;
    assert!(real_err.is_err());
    assert_eq!(std::fs::read(&src).expect("src intact"), b"confidential");
}

#[tokio::test]
async fn r_fail_09_corrupted_or_unsafe_config_rejected_without_mutating_db() {
    let dir = TempDir::new().expect("tempdir");
    let db = database::open_in_memory().await.expect("db");
    let monitored = dir.path().join("Downloads");
    std::fs::create_dir_all(&monitored).expect("mkdir Downloads");
    repo_folders::insert(
        &db,
        "existing-f1",
        "Downloads",
        &monitored.to_string_lossy(),
        false,
    )
    .await
    .expect("insert folder");

    let policy = RootPolicy {
        home: dir.path().join("home"),
        app_data_dir: dir.path().join("appdata"),
    };

    // 1. Malformed JSON syntax
    let err_json = preview_config_import(&db, &policy, "{not valid json!!").await;
    assert!(err_json.is_err());

    // 2. Unsupported schema version
    let bad_schema = serde_json::json!({
        "schemaVersion": 999,
        "exportedAtMs": 1_700_000_000_000i64,
        "settings": {},
        "categories": [],
        "folders": [],
        "exclusions": [],
        "rules": []
    })
    .to_string();
    assert!(preview_config_import(&db, &policy, &bad_schema)
        .await
        .is_err());

    // 3. Forbidden system folder root `/System` rejected on Replace import
    let bad_root = serde_json::json!({
        "schemaVersion": 1,
        "exportedAtMs": 1_700_000_000_000i64,
        "settings": folderflow_lib::settings::Settings::default(),
        "categories": [],
        "folders": [{
            "id": "bad",
            "name": "System",
            "path": "/System",
            "enabled": true,
            "recursive": false,
            "settings": folderflow_lib::folders::FolderSettings::default()
        }],
        "exclusions": [],
        "rules": []
    })
    .to_string();
    let import_err = apply_config_import(
        &db,
        &policy,
        &bad_root,
        ConfigImportMode::Replace,
        1_700_000_000_000,
    )
    .await;
    assert!(import_err.is_err());

    // Verify existing DB state was completely preserved (transaction safety).
    let folders_after = repo_folders::list(&db).await.expect("list folders");
    assert_eq!(folders_after.len(), 1);
    assert_eq!(folders_after[0].id, "existing-f1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_fail_10_watcher_failure_surfaces_error_and_recovers() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path().join("WatchTarget");
    std::fs::create_dir_all(&root).expect("mkdir target");

    let db = database::open_in_memory().await.expect("db");
    repo_folders::insert(
        &db,
        "f-watch",
        "WatchTarget",
        &root.to_string_lossy(),
        false,
    )
    .await
    .expect("insert folder");
    let folder = MonitoredFolder {
        id: "f-watch".into(),
        name: "WatchTarget".into(),
        path: root.to_string_lossy().into_owned(),
        enabled: true,
        recursive: false,
        settings: FolderSettings::default(),
        health: FolderHealth::Unknown,
        last_error: None,
        file_count: 0,
        total_bytes: 0,
        last_scanned_at: None,
        created_at: 1_700_000_000_000,
    };

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_cb = attempts.clone();
    let runtime_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<RawEvent>>>> =
        Arc::new(Mutex::new(None));
    let tx_slot = runtime_tx.clone();

    let factory: WatcherFactory = Arc::new(move |_path, _rec, tx, _overflow| {
        let n = attempts_cb.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            return Err("simulated FSEvents start failure".into());
        }
        *tx_slot.lock().expect("lock tx") = Some(tx);
        Ok(Box::new(()) as Box<dyn Send>)
    });

    let observer = Arc::new(RecordingObserver::default());
    let mut deps = MonitorDeps::new(
        db.clone(),
        Arc::new(Classifier::new(builtin_categories())),
        MonitoringSettings::default(),
        observer.clone(),
    );
    deps.watcher_factory = factory;
    deps.timing = MonitorTiming {
        debounce: Duration::from_millis(15),
        stability_interval: Duration::from_millis(15),
        stability_checks: 2,
        reconcile_interval: Duration::from_secs(60),
        health_interval: Duration::from_millis(50),
        max_pending: 128,
        max_tracked: 128,
        backoff_min: Duration::from_millis(25),
        backoff_max: Duration::from_millis(60),
        stop_timeout: Duration::from_secs(2),
    };

    let mon = MonitorService::new(deps);
    mon.start(folder).await;

    // Wait for automatic retry after first failure -> Watching.
    let deadline = Instant::now() + Duration::from_secs(3);
    while observer.last_state() != Some(RuntimeState::Watching) {
        assert!(
            Instant::now() < deadline,
            "did not recover to Watching: {:?}",
            observer.last_error()
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
    }

    // Now inject a runtime watcher error event and verify clean Error state + subsequent recovery.
    let tx_opt = runtime_tx.lock().expect("lock tx").clone();
    if let Some(tx) = tx_opt {
        tx.send(RawEvent::Error("FSEvents stream dropped".into()))
            .await
            .expect("send error");
    }
    let err_deadline = Instant::now() + Duration::from_secs(3);
    while attempts.load(Ordering::SeqCst) < 3 && Instant::now() < err_deadline {
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(
        attempts.load(Ordering::SeqCst) >= 3,
        "expected watcher restart after runtime error"
    );
    mon.shutdown().await;
}

#[tokio::test]
async fn r_fail_11_duplicate_and_flood_events_apply_nonblocking_backpressure() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, _dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    // 1. Debouncer coalesces 500 rapid duplicate events for the same path into one emission.
    let mut debouncer = Debouncer::new(Duration::from_millis(25), 64);
    let t0 = Instant::now();
    let dup_path = src_dir.join("burst.pdf");
    for i in 0..500u64 {
        let _ = debouncer.push(dup_path.clone(), t0 + Duration::from_micros(i * 10));
    }
    let ready = debouncer.drain_ready(t0 + Duration::from_millis(50));
    assert_eq!(
        ready,
        vec![dup_path.clone()],
        "500 duplicate events must coalesce to 1"
    );

    // 2. Bounded AutomationPipeline drops overflow non-blockingly and marks folder dirty.
    let pipeline = AutomationPipeline::new(
        db,
        Organizer::new(
            Ledger::new(database::open_in_memory().await.expect("db")),
            safe_fs,
            clock,
        ),
        Arc::new(Classifier::new(builtin_categories())),
        authority,
        4,
    );

    let fact = ScannedFile {
        facts: FileFacts {
            path: dup_path,
            name: "burst.pdf".into(),
            extension: Some("pdf".into()),
            size: 100,
            kind: EntryKind::File,
            created_ns: Some(1_700_000_000_000_000_000),
            modified_ns: 1_700_000_000_000_000_000,
            is_hidden: false,
            dev: 1,
            ino: 42,
        },
        category: "documents".into(),
        mime: Some("application/pdf".into()),
        confidence: Confidence::High,
    };

    let mut overflowed = 0usize;
    for _ in 0..50 {
        if pipeline.enqueue("f1", fact.clone()) == EnqueueResult::OverflowMarkedDirty {
            overflowed += 1;
        }
    }
    assert_eq!(
        overflowed, 46,
        "capacity=4 should queue 4 and overflow 46 without blocking"
    );
    assert_eq!(pipeline.dropped_overflow_count(), 46);
    assert!(pipeline.is_folder_dirty("f1"));

    // Silence unused-import warning for ReadOnlyFs if needed by exercising RealFs metadata read.
    let _ = &RealFs;
}

#[tokio::test]
async fn r_fail_12_rapid_changes_on_active_file_deferred_and_stale_aborts() {
    let dir = TempDir::new().expect("tempdir");
    let (db, src_dir, dst_dir, authority, safe_fs, clock) = setup_roots(dir.path()).await;

    // 1. StabilityTracker never fires while size/mtime keeps changing (S9), then emits once settled.
    let active = src_dir.join("downloading.bin");
    write_new_file(&active, b"chunk1");
    let mut tracker = StabilityTracker::new(Duration::from_millis(10), 3, 32);
    let mut now = Instant::now();
    assert!(tracker.track(active.clone(), now));

    for chunk in [
        b"chunk1-2".as_slice(),
        b"chunk1-2-3".as_slice(),
        b"chunk1-2-3-4".as_slice(),
    ] {
        overwrite_fixture(&active, chunk);
        let facts = RealFs.stat(&active).expect("stat active");
        now += Duration::from_millis(15);
        assert_eq!(
            tracker.observe(&active, Observation::Present(facts), now),
            None,
            "actively changing file must not become Stable"
        );
    }

    let settled = RealFs.stat(&active).expect("stat settled");
    let mut events = Vec::new();
    for _ in 0..3 {
        now += Duration::from_millis(15);
        if let Some(ev) = tracker.observe(&active, Observation::Present(settled.clone()), now) {
            events.push(ev);
        }
    }
    assert_eq!(
        events.len(),
        1,
        "settled file should emit Stable exactly once"
    );

    // 2. Organizer S6 stale-identity guard aborts if file is modified after planning.
    let snapshot = safe_fs.lstat(&active).expect("lstat before write");
    overwrite_fixture(&active, b"modified-after-plan-snapshot");

    let ledger = Ledger::new(db);
    let organizer = Organizer::new(ledger.clone(), safe_fs, clock);
    let mut req = OrganizeRequest::new(
        OpKind::Move,
        active.clone(),
        dst_dir.join("downloading.bin"),
    );
    req.op_id = Some("fail-12-stale".into());
    req.expected_source = Some(snapshot);

    let err = organizer
        .execute(req, &authority)
        .await
        .expect_err("S6 identity mismatch must abort");
    assert!(!err.to_string().is_empty());
    assert_eq!(
        std::fs::read(&active).expect("active preserved"),
        b"modified-after-plan-snapshot"
    );
    assert!(!dst_dir.join("downloading.bin").exists());
    let row = ledger
        .get("fail-12-stale")
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, OpStatus::Failed);

    let _unused: HashMap<String, String> = HashMap::new();
}
