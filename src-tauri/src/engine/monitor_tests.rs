//! Integration tests for the folder monitor: real FSEvents, temp dirs only.
//! Timing is shortened; every assertion waits with a bounded timeout because
//! watcher delivery is asynchronous (TESTING_STRATEGY §4).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::monitor::*;
use crate::categories::builtin_categories;
use crate::database::{self, Db};
use crate::engine::classifier::Classifier;
use crate::engine::paths::RootPolicy;
use crate::engine::scanner::ScannedFile;
use crate::folders::{FolderHealth, FolderService, MonitoredFolder};
use crate::settings::MonitoringSettings;
use crate::test_support::{magic, mkdir, remove_dir_all, remove_file, rename, tempdir, write_file};

#[derive(Default)]
struct Recorder {
    stable: Mutex<Vec<(PathBuf, String, Instant)>>,
    removed: Mutex<Vec<PathBuf>>,
    states: Mutex<Vec<RuntimeState>>,
}

impl MonitorObserver for Recorder {
    fn on_status(&self, s: &FolderRuntime) {
        let mut v = self.states.lock().expect("lock");
        if v.last() != Some(&s.state) {
            v.push(s.state);
        }
    }
    fn on_stable(&self, _folder_id: &str, f: &ScannedFile) {
        self.stable.lock().expect("lock").push((
            f.facts.path.clone(),
            f.category.clone(),
            Instant::now(),
        ));
    }
    fn on_removed(&self, _folder_id: &str, path: &Path) {
        self.removed.lock().expect("lock").push(path.to_path_buf());
    }
}

impl Recorder {
    fn stable_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .stable
            .lock()
            .expect("lock")
            .iter()
            .map(|(p, _, _)| p.file_name().expect("name").to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
    fn removed_names(&self) -> Vec<String> {
        self.removed
            .lock()
            .expect("lock")
            .iter()
            .map(|p| p.file_name().expect("name").to_string_lossy().into_owned())
            .collect()
    }
    fn last_state(&self) -> Option<RuntimeState> {
        self.states.lock().expect("lock").last().copied()
    }
}

fn fast_timing() -> MonitorTiming {
    MonitorTiming {
        debounce: Duration::from_millis(80),
        stability_interval: Duration::from_millis(120),
        stability_checks: 2,
        reconcile_interval: Duration::from_secs(3600),
        health_interval: Duration::from_millis(150),
        max_pending: 10_000,
        max_tracked: 10_000,
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(400),
        stop_timeout: Duration::from_millis(200),
    }
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for: {what}");
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    db: Db,
    folder: MonitoredFolder,
    rec: Arc<Recorder>,
    mon: MonitorService,
}

async fn env_with(recursive: bool, timing: MonitorTiming, factory: Option<WatcherFactory>) -> Env {
    let (tmp, base) = tempdir();
    let root = base.join("Inbox");
    mkdir(&root);
    let db = database::open_in_memory().await.expect("db");
    let folders = FolderService::new(
        db.clone(),
        RootPolicy {
            home: base.join("home"),
            app_data_dir: base.join("appdata"),
        },
    );
    let folder = folders.add(&root, recursive).await.expect("add");
    let rec = Arc::new(Recorder::default());
    let mut deps = MonitorDeps::new(
        db.clone(),
        Arc::new(Classifier::new(builtin_categories())),
        MonitoringSettings::default(),
        rec.clone(),
    );
    deps.timing = timing;
    if let Some(f) = factory {
        deps.watcher_factory = f;
    }
    let mon = MonitorService::new(deps);
    Env {
        _tmp: tmp,
        root,
        db,
        folder,
        rec,
        mon,
    }
}

async fn started(recursive: bool) -> Env {
    let e = env_with(recursive, fast_timing(), None).await;
    e.mon.start(e.folder.clone()).await;
    let rec = e.rec.clone();
    wait_until("watching", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    // FSEvents can take a moment to begin delivering after the stream starts.
    tokio::time::sleep(Duration::from_millis(300)).await;
    e
}

async fn health(db: &Db, id: &str) -> String {
    sqlx::query_scalar("SELECT health FROM monitored_folders WHERE id = ?1")
        .bind(id)
        .fetch_one(db)
        .await
        .expect("health")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_watch_01_new_file_becomes_stable_and_is_classified() {
    let e = started(false).await;
    write_file(&e.root.join("invoice.pdf"), magic::PDF);
    let rec = e.rec.clone();
    wait_until("stable invoice.pdf", || {
        rec.stable_names() == vec!["invoice.pdf"]
    })
    .await;
    assert_eq!(e.rec.stable.lock().expect("lock")[0].1, "documents");
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM file_records WHERE filename = 'invoice.pdf'")
            .fetch_one(&e.db)
            .await
            .expect("n");
    assert_eq!(n, 1);
    assert_eq!(health(&e.db, &e.folder.id).await, "healthy");
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_watch_02_modified_file_is_reported_again() {
    let e = started(false).await;
    write_file(&e.root.join("notes.txt"), b"v1");
    let rec = e.rec.clone();
    wait_until("first stable", || rec.stable_names().len() == 1).await;
    write_file(&e.root.join("notes.txt"), b"version two");
    wait_until("second stable", || rec.stable_names().len() == 2).await;
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_watch_03_rename_reports_removed_old_and_stable_new() {
    let e = started(false).await;
    write_file(&e.root.join("a.txt"), b"x");
    let rec = e.rec.clone();
    wait_until("a stable", || rec.stable_names() == vec!["a.txt"]).await;
    rename(&e.root.join("a.txt"), &e.root.join("b.txt"));
    wait_until("b stable + a removed", || {
        rec.stable_names().contains(&"b.txt".to_string())
            && rec.removed_names().contains(&"a.txt".to_string())
    })
    .await;
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_watch_04_delete_is_reported_and_record_removed() {
    let e = started(false).await;
    write_file(&e.root.join("gone.txt"), b"x");
    let rec = e.rec.clone();
    wait_until("stable", || rec.stable_names().len() == 1).await;
    remove_file(&e.root.join("gone.txt"));
    wait_until("removed", || rec.removed_names() == vec!["gone.txt"]).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_records")
        .fetch_one(&e.db)
        .await
        .expect("n");
    assert_eq!(n, 0);
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_watch_05_new_directory_contents_found_when_recursive_ignored_dirs_skipped() {
    let e = started(true).await;
    write_file(&e.root.join("Project/deep/report.pdf"), magic::PDF);
    write_file(&e.root.join("node_modules/pkg/index.js"), b"x");
    write_file(&e.root.join("Tool.app/Contents/Info.plist"), b"x");
    let rec = e.rec.clone();
    wait_until("nested file + package stable", || {
        let n = rec.stable_names();
        n.contains(&"report.pdf".to_string()) && n.contains(&"Tool.app".to_string())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let names = e.rec.stable_names();
    assert!(
        !names.iter().any(|n| n == "index.js" || n == "Info.plist"),
        "{names:?}"
    );
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_watch_06_non_recursive_ignores_subfolder_files() {
    let e = started(false).await;
    write_file(&e.root.join("sub/inner.pdf"), magic::PDF);
    write_file(&e.root.join("top.pdf"), magic::PDF);
    let rec = e.rec.clone();
    wait_until("top stable", || {
        rec.stable_names().contains(&"top.pdf".to_string())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(e.rec.stable_names(), vec!["top.pdf"]);
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_stab_05_growing_download_reported_once_after_writes_stop_partial_name_never() {
    let e = started(false).await;
    let partial = e.root.join("movie.mp4.crdownload");
    let mut last_write = Instant::now();
    for i in 0..12 {
        write_file(&partial, &vec![0u8; 1000 * (i + 1)]);
        last_write = Instant::now();
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    rename(&partial, &e.root.join("movie.mp4"));
    let renamed_at = Instant::now();
    let rec = e.rec.clone();
    wait_until("movie stable", || rec.stable_names() == vec!["movie.mp4"]).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let stable = e.rec.stable.lock().expect("lock").clone();
    assert_eq!(stable.len(), 1, "reported exactly once");
    assert!(stable[0].2 >= renamed_at && stable[0].2 > last_write);
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_sup_01_watcher_start_failure_is_retried_with_backoff() {
    let attempts = Arc::new(AtomicU32::new(0));
    let a = attempts.clone();
    let real = default_watcher_factory();
    let flaky: WatcherFactory = Arc::new(move |root: &Path, recursive, tx, overflow| {
        if a.fetch_add(1, Ordering::SeqCst) < 2 {
            Err("simulated watcher failure".to_string())
        } else {
            real(root, recursive, tx, overflow)
        }
    });
    let e = env_with(false, fast_timing(), Some(flaky)).await;
    e.mon.start(e.folder.clone()).await;
    let rec = e.rec.clone();
    wait_until("watching after retries", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    assert!(attempts.load(Ordering::SeqCst) >= 3);
    assert!(e
        .rec
        .states
        .lock()
        .expect("lock")
        .contains(&RuntimeState::Error));
    let errors: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM watcher_errors")
        .fetch_one(&e.db)
        .await
        .expect("n");
    assert!(errors >= 2, "watcher errors are recorded");
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_sup_02_03_deleted_folder_becomes_unavailable_and_recovers_when_recreated() {
    let e = started(false).await;
    remove_dir_all(&e.root);
    let rec = e.rec.clone();
    wait_until("unavailable", || {
        rec.last_state() == Some(RuntimeState::Unavailable)
    })
    .await;
    assert_eq!(health(&e.db, &e.folder.id).await, "unavailable");
    mkdir(&e.root);
    wait_until("watching again", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    write_file(&e.root.join("after.pdf"), magic::PDF);
    wait_until("after.pdf stable", || {
        rec.stable_names().contains(&"after.pdf".to_string())
    })
    .await;
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_sup_04_unmount_like_move_away_and_permission_revocation_recover() {
    let e = started(false).await;
    let parked = e.root.with_file_name("Parked");
    rename(&e.root, &parked); // simulates a drive disappearing
    let rec = e.rec.clone();
    wait_until("unavailable", || {
        rec.last_state() == Some(RuntimeState::Unavailable)
    })
    .await;
    rename(&parked, &e.root); // drive reconnected
    wait_until("watching", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    crate::test_support::set_mode(&e.root, 0o000);
    wait_until("permission denied", || {
        rec.last_state() == Some(RuntimeState::PermissionDenied)
    })
    .await;
    assert_eq!(health(&e.db, &e.folder.id).await, "permission_denied");
    crate::test_support::set_mode(&e.root, 0o755);
    wait_until("watching after grant", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_mon_pause_resume_picks_up_files_added_while_paused() {
    let e = started(false).await;
    e.mon.pause(&e.folder.id).await;
    let rec = e.rec.clone();
    wait_until("paused", || rec.last_state() == Some(RuntimeState::Paused)).await;
    assert_eq!(health(&e.db, &e.folder.id).await, "paused");
    write_file(&e.root.join("while-paused.pdf"), magic::PDF);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        e.rec.stable_names().is_empty(),
        "nothing processed while paused"
    );
    e.mon.resume(&e.folder.id).await;
    wait_until("picked up after resume", || {
        rec.stable_names() == vec!["while-paused.pdf"]
    })
    .await;
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_mon_global_pause_and_status() {
    let e = started(false).await;
    assert!(!e.mon.status().await.paused);
    e.mon.pause_all().await;
    let rec = e.rec.clone();
    wait_until("paused", || rec.last_state() == Some(RuntimeState::Paused)).await;
    let s = e.mon.status().await;
    assert!(s.paused);
    assert_eq!(s.folders.len(), 1);
    e.mon.resume_all().await;
    wait_until("watching", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    e.mon.stop(&e.folder.id).await;
    assert!(e.mon.status().await.folders.is_empty());
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_mon_event_overflow_falls_back_to_reconcile_and_loses_nothing() {
    let mut t = fast_timing();
    t.max_pending = 5;
    let e = env_with(false, t, None).await;
    e.mon.start(e.folder.clone()).await;
    let rec = e.rec.clone();
    wait_until("watching", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    for i in 0..60 {
        write_file(&e.root.join(format!("f{i:02}.txt")), b"x");
    }
    wait_until("all 60 stable", || rec.stable_names().len() >= 60).await;
    let mut names = e.rec.stable_names();
    names.dedup();
    assert_eq!(names.len(), 60);
    e.mon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_mon_rescan_command_reconciles() {
    let e = started(false).await;
    e.mon.rescan(&e.folder.id).await;
    let rec = e.rec.clone();
    wait_until("still watching after rescan", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    let scanned: Option<i64> = sqlx::query_scalar("SELECT last_scanned_at FROM monitored_folders")
        .fetch_one(&e.db)
        .await
        .expect("ts");
    assert!(scanned.is_some());
    let _ = FolderHealth::Healthy;
    e.mon.shutdown().await;
}

/// BUG-010: stopping a folder whose task is busy must not hang the caller,
/// and the task must be cancelled rather than left running.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bug_010_stop_is_bounded_when_the_task_is_busy() {
    let slow: WatcherFactory = Arc::new(|_root: &Path, _recursive, _tx, _overflow| {
        std::thread::sleep(Duration::from_millis(1500)); // a task stuck in blocking work
        Err("slow".to_string())
    });
    let e = env_with(false, fast_timing(), Some(slow)).await;
    e.mon.start(e.folder.clone()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let t = Instant::now();
    e.mon.stop(&e.folder.id).await;
    assert!(
        t.elapsed() < Duration::from_millis(1000),
        "stop took {:?}",
        t.elapsed()
    );
    assert!(e.mon.status().await.folders.is_empty());
}

/// Exit criterion: several folders monitored at once, each independently.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r_mon_three_folders_are_monitored_independently() {
    let (_tmp, base) = tempdir();
    let db = database::open_in_memory().await.expect("db");
    let svc = FolderService::new(
        db.clone(),
        RootPolicy {
            home: base.join("home"),
            app_data_dir: base.join("appdata"),
        },
    );
    let rec = Arc::new(Recorder::default());
    let mut deps = MonitorDeps::new(
        db.clone(),
        Arc::new(Classifier::new(builtin_categories())),
        MonitoringSettings::default(),
        rec.clone(),
    );
    deps.timing = fast_timing();
    let mon = MonitorService::new(deps);
    let mut roots = Vec::new();
    for name in ["Downloads", "Desktop", "Documents"] {
        let root = base.join(name);
        mkdir(&root);
        mon.start(svc.add(&root, false).await.expect("add")).await;
        roots.push(root);
    }
    let m = &mon;
    let deadline = Instant::now() + Duration::from_secs(15);
    while m
        .status()
        .await
        .folders
        .iter()
        .filter(|f| f.state == RuntimeState::Watching)
        .count()
        < 3
    {
        assert!(Instant::now() < deadline, "not all folders watching");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    for (i, root) in roots.iter().enumerate() {
        write_file(&root.join(format!("file{i}.pdf")), magic::PDF);
    }
    let r = rec.clone();
    wait_until("one stable file per folder", || {
        r.stable_names() == vec!["file0.pdf", "file1.pdf", "file2.pdf"]
    })
    .await;

    mon.pause(&svc.list().await.expect("list")[1].id).await;
    let s = mon.status().await;
    assert_eq!(s.folders.len(), 3);
    mon.shutdown().await;
}

/// Exit criterion + SAFETY_MODEL S5 precursor: monitoring never writes to the
/// monitored folders. Snapshot (path, size, mtime, inode) before and after a
/// full session (baseline scan, events, rescan, pause, resume, stop).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_mon_monitoring_makes_zero_filesystem_writes() {
    fn snapshot(root: &Path) -> Vec<(PathBuf, u64, i64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let mut v: Vec<_> = walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .map(|e| {
                let m = e.path().symlink_metadata().expect("meta");
                // Directory mtimes change when *we* add fixture files, so record them as 0.
                let mtime = if m.is_dir() {
                    0
                } else {
                    m.mtime_nsec() + m.mtime() * 1_000_000_000
                };
                (
                    e.path().to_path_buf(),
                    if m.is_dir() { 0 } else { m.len() },
                    mtime,
                    m.ino(),
                )
            })
            .collect();
        v.sort();
        v
    }
    let e = env_with(true, fast_timing(), None).await;
    write_file(&e.root.join("a.pdf"), magic::PDF);
    write_file(&e.root.join("sub/b.png"), magic::PNG);
    write_file(&e.root.join("Tool.app/Contents/Info.plist"), b"x");
    let before = snapshot(&e.root);

    e.mon.start(e.folder.clone()).await;
    let rec = e.rec.clone();
    wait_until("watching", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    e.mon.rescan(&e.folder.id).await;
    e.mon.pause(&e.folder.id).await;
    wait_until("paused", || rec.last_state() == Some(RuntimeState::Paused)).await;
    e.mon.resume(&e.folder.id).await;
    wait_until("watching", || {
        rec.last_state() == Some(RuntimeState::Watching)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    e.mon.shutdown().await;

    assert_eq!(
        snapshot(&e.root),
        before,
        "monitoring must not create, modify, move or delete anything"
    );
}
