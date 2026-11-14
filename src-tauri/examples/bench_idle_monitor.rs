//! Idle CPU & RSS probe for `BENCH-07`: runs `MonitorService` over a real
//! FSEvents-watched directory with zero incoming filesystem events for a
//! configurable duration (default 10 s; pass `0` for startup baseline).

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use folderflow_lib::database::{open_in_memory, repo_categories, repo_folders};
use folderflow_lib::engine::classifier::Classifier;
use folderflow_lib::engine::monitor::{
    FolderRuntime, MonitorDeps, MonitorObserver, MonitorService, RuntimeState,
};
use folderflow_lib::engine::scanner::ScannedFile;
use folderflow_lib::settings::MonitoringSettings;

struct NoopObserver;

impl MonitorObserver for NoopObserver {
    fn on_status(&self, _status: &FolderRuntime) {}
    fn on_stable(&self, _folder_id: &str, _file: &ScannedFile) {}
    fn on_removed(&self, _folder_id: &str, _path: &Path) {}
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let idle_secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let dir = tempfile::tempdir()?;
    let root = std::fs::canonicalize(dir.path())?;
    for i in 0..100 {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(format!("note_{i:03}.txt")))?;
        f.write_all(b"bench")?;
    }

    let db = open_in_memory().await?;
    let categories = repo_categories::list(&db).await?;
    let classifier = Arc::new(Classifier::new(categories));

    repo_folders::insert(&db, "bench-f1", "Bench", &root.to_string_lossy(), true).await?;
    let folder = repo_folders::get(&db, "bench-f1")
        .await?
        .ok_or("missing inserted folder")?;

    let monitor = MonitorService::new(MonitorDeps::new(
        db,
        classifier,
        MonitoringSettings::default(),
        Arc::new(NoopObserver),
    ));
    monitor.start(folder).await;

    // Wait until initial reconcile finishes and watcher settles in Watching.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let st = monitor.status().await;
        if st
            .folders
            .iter()
            .any(|f| f.folder_id == "bench-f1" && f.state == RuntimeState::Watching)
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("monitor did not reach Watching state in time".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    if idle_secs > 0 {
        tokio::time::sleep(Duration::from_secs(idle_secs)).await;
    }

    monitor.shutdown().await;
    Ok(())
}
