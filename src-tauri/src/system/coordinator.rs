//! Sleep/wake and external volume mount/unmount orchestration (`FR-022`, `FR-023`, `P7-T03`).

use std::path::{Path, PathBuf};

pub use ff_macos_events::{start_workspace_observers, WorkspaceObserverGuard, WorkspaceSystemEvent};

use crate::engine::monitor::{MonitorService, RuntimeState};
use crate::engine::paths::nfc;
use crate::engine::scheduler::SchedulerService;
use crate::folders::FolderService;

fn normalize_for_mount_match(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let norm = nfc(&resolved);
    if norm.starts_with("/private/var") || norm.starts_with("/private/tmp") {
        if let Ok(stripped) = norm.strip_prefix("/private") {
            return PathBuf::from("/").join(stripped);
        }
    }
    norm
}

fn path_matches_mount(folder_path: &Path, mount_point: &Path) -> bool {
    let norm_folder = normalize_for_mount_match(folder_path);
    let norm_mount = normalize_for_mount_match(mount_point);
    norm_folder.starts_with(&norm_mount)
}

/// Applies one macOS `NSWorkspace` system event to monitoring and scheduling services (`FR-022`, `FR-023`).
pub async fn handle_workspace_event(
    event: WorkspaceSystemEvent,
    folders: &FolderService,
    monitor: &MonitorService,
    scheduler: &SchedulerService,
) {
    match event {
        WorkspaceSystemEvent::WillSleep => {
            tracing::info!(target: "folderflow::system", "macOS entering sleep");
        }
        WorkspaceSystemEvent::DidWake => {
            tracing::info!(target: "folderflow::system", "macOS woke from sleep; reconciling folders and due schedules");
            monitor.rescan_all().await;
            if let Err(err) = scheduler.tick_once().await {
                tracing::warn!(
                    target: "folderflow::system",
                    error = %err,
                    "post-wake scheduler tick failed"
                );
            }
        }
        WorkspaceSystemEvent::VolumeUnmounted { mount_point } => {
            tracing::info!(
                target: "folderflow::system",
                mount = %mount_point.display(),
                "volume unmounted; rechecking matching monitored folders"
            );
            let Ok(all_folders) = folders.list().await else {
                return;
            };
            let mount_buf = PathBuf::from(&mount_point);
            for folder in all_folders.into_iter().filter(|f| f.enabled) {
                if path_matches_mount(Path::new(&folder.path), &mount_buf) {
                    monitor.recheck(&folder.id).await;
                }
            }
        }
        WorkspaceSystemEvent::VolumeMounted { mount_point } => {
            tracing::info!(
                target: "folderflow::system",
                mount = %mount_point.display(),
                "volume mounted; resuming and reconciling matching monitored folders"
            );
            let Ok(all_folders) = folders.list().await else {
                return;
            };
            let runtime = monitor.status().await;
            let unavailable_ids: std::collections::HashSet<String> = runtime
                .folders
                .into_iter()
                .filter(|rt| rt.state == RuntimeState::Unavailable)
                .map(|rt| rt.folder_id)
                .collect();

            let mount_buf = PathBuf::from(&mount_point);
            for folder in all_folders.into_iter().filter(|f| f.enabled) {
                if path_matches_mount(Path::new(&folder.path), &mount_buf)
                    || unavailable_ids.contains(&folder.id)
                {
                    monitor.rescan(&folder.id).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc;

    use super::*;
    use crate::categories::CategoryService;
    use crate::database::{self, repo_files};
    use crate::engine::classifier::Classifier;
    use crate::engine::monitor::{MonitorDeps, MonitorObserver, MonitorTiming, RawEvent, WatcherFactory};
    use crate::engine::operations::Ledger;
    use crate::engine::organizer::Organizer;
    use crate::engine::paths::{PathAuthority, RootPolicy};
    use crate::engine::pipeline::{AutomationPipeline, DEFAULT_PIPELINE_QUEUE_CAPACITY};
    use crate::engine::safe_fs::SafeFs;
    use crate::engine::scheduler::SchedulerService;
    use crate::folders::FolderService;
    use crate::rules::models::{
        Action, Condition, ConditionNode, DateSource, ExecutionMode, RuleDraft, RuleOrigin,
        TextFieldKind, TextOp,
    };
    use crate::rules::RuleService;
    use crate::settings::MonitoringSettings;
    use crate::test_support;
    use crate::utils::clock::SystemClock;

    struct PipelineForwardingObserver(Arc<AutomationPipeline>);
    impl MonitorObserver for PipelineForwardingObserver {
        fn on_status(&self, status: &crate::engine::monitor::FolderRuntime) {
            self.0.on_status(status);
        }
        fn on_stable(&self, folder_id: &str, file: &crate::engine::scanner::ScannedFile) {
            self.0.on_stable(folder_id, file);
        }
        fn on_removed(&self, folder_id: &str, path: &Path) {
            self.0.on_removed(folder_id, path);
        }
    }

    fn noop_watcher_factory() -> WatcherFactory {
        Arc::new(
            |_root: &Path, _rec: bool, tx: mpsc::Sender<RawEvent>, _overflow: Arc<AtomicBool>| {
                // Hold sender inside dummy watcher so channel stays open until stopped.
                Ok(Box::new(tx) as Box<dyn Send>)
            },
        )
    }

    fn fast_timing() -> MonitorTiming {
        MonitorTiming {
            debounce: Duration::from_millis(10),
            stability_interval: Duration::from_millis(10),
            stability_checks: 1,
            reconcile_interval: Duration::from_secs(3600),
            health_interval: Duration::from_secs(3600),
            backoff_min: Duration::from_millis(20),
            backoff_max: Duration::from_millis(50),
            stop_timeout: Duration::from_millis(500),
            max_pending: 1000,
            max_tracked: 1000,
        }
    }

    async fn setup_system_harness(root: &Path) -> (
        crate::database::Db,
        FolderService,
        Arc<MonitorService>,
        Arc<SchedulerService>,
        Arc<AutomationPipeline>,
        PathBuf,
    ) {
        let db = database::open(&root.join("app.db")).await.expect("db");
        let cats = CategoryService::new(db.clone());
        cats.ensure_builtins().await.expect("builtins");
        let classifier = Arc::new(Classifier::new(cats.list().await.expect("cats")));

        let home = root.join("home");
        let app_data = root.join("appdata");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&app_data).expect("appdata");
        let policy = RootPolicy {
            home: home.clone(),
            app_data_dir: app_data,
        };
        let folders = FolderService::new(db.clone(), policy);
        let safe_fs = Arc::new(SafeFs::production());
        let clock = Arc::new(SystemClock);
        let organizer = Organizer::new(Ledger::new(db.clone()), safe_fs, clock.clone());
        let pipeline = Arc::new(AutomationPipeline::new(
            db.clone(),
            organizer.clone(),
            classifier.clone(),
            PathAuthority::default(),
            DEFAULT_PIPELINE_QUEUE_CAPACITY,
        ));
        let scheduler = Arc::new(SchedulerService::new(
            db.clone(),
            organizer,
            classifier.clone(),
            PathAuthority::default(),
            clock,
        ));

        let mut deps = MonitorDeps::new(
            db.clone(),
            classifier,
            MonitoringSettings::default(),
            Arc::new(PipelineForwardingObserver(pipeline.clone())),
        );
        deps.watcher_factory = noop_watcher_factory();
        deps.timing = fast_timing();
        let monitor = Arc::new(MonitorService::new(deps));
        (db, folders, monitor, scheduler, pipeline, home)
    }

    async fn wait_for_folder_state(
        monitor: &MonitorService,
        folder_id: &str,
        expected: RuntimeState,
    ) {
        for _ in 0..80 {
            let st = monitor.status().await;
            if st
                .folders
                .iter()
                .any(|f| f.folder_id == folder_id && f.state == expected)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "timed out waiting for folder {folder_id} to reach {expected:?}: {:?}",
            monitor.status().await
        );
    }

    #[tokio::test]
    async fn r_sys_01_did_wake_triggers_reconcile_and_detects_missed_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (db, folders, monitor, scheduler, _pipeline, home) =
            setup_system_harness(dir.path()).await;

        let src = home.join("Inbox");
        std::fs::create_dir_all(&src).expect("src");
        let folder = folders.add(&src, false).await.expect("add folder");
        monitor.start(folder.clone()).await;
        wait_for_folder_state(&monitor, &folder.id, RuntimeState::Watching).await;

        // Baseline scan sees empty folder. Add a file while watcher isn't delivering OS events (simulates sleep).
        let missed = src.join("missed_during_sleep.pdf");
        test_support::write_file(&missed, b"%PDF-1.4\nsleep wake");

        handle_workspace_event(WorkspaceSystemEvent::DidWake, &folders, &monitor, &scheduler).await;

        // Diff reconcile + 1-check fast stability picks up the file created during sleep.
        let src_str = src.to_string_lossy().into_owned();
        let mut tracked = false;
        for _ in 0..80 {
            let facts = repo_files::list_facts_for_folder(&db, &folder.id, &src_str, "Inbox")
                .await
                .expect("list facts");
            if facts.iter().any(|p| p.path.ends_with("missed_during_sleep.pdf")) {
                tracked = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(tracked, "DidWake must reconcile missed files");
        monitor.shutdown().await;
    }

    #[tokio::test]
    async fn r_sys_02_volume_unmount_marks_external_folder_unavailable_and_mount_resumes_and_reconciles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (db, folders, monitor, scheduler, pipeline, home) =
            setup_system_harness(dir.path()).await;

        // Simulated external mount inside test root (macOS tests bypass `/Volumes` root using RootPolicy::home parent).
        let volume_root = home.join("SimVolume");
        let folder_path = volume_root.join("Photos");
        let dest_dir = home.join("OrganizedPhotos");
        std::fs::create_dir_all(&folder_path).expect("folder_path");
        std::fs::create_dir_all(&dest_dir).expect("dest_dir");

        let folder = folders.add(&folder_path, false).await.expect("add folder");
        let rule_svc = RuleService::new(
            db.clone(),
            RootPolicy {
                home: home.clone(),
                app_data_dir: dir.path().join("appdata"),
            },
        );
        let approved_dest = rule_svc
            .add_destination_root(&dest_dir)
            .await
            .expect("approve dest");
        rule_svc
            .create(
                RuleDraft {
                    name: "Sort Wake Pics".into(),
                    description: None,
                    enabled: true,
                    source_folders: vec![folder.id.clone()],
                    conditions: ConditionNode::All {
                        children: vec![ConditionNode::Leaf {
                            condition: Condition::Text {
                                field: TextFieldKind::Extension,
                                op: TextOp::Equals,
                                value: "png".into(),
                                case_sensitive: false,
                            },
                        }],
                    },
                    actions: vec![Action::Move {
                        destination: approved_dest,
                    }],
                    date_source: DateSource::Modified,
                    execution_mode: ExecutionMode::FirstMatch,
                },
                RuleOrigin::User,
            )
            .await
            .expect("rule");
        pipeline.set_authority(
            crate::engine::activity::load_path_authority(&db)
                .await
                .expect("authority"),
        );

        monitor.start(folder.clone()).await;
        wait_for_folder_state(&monitor, &folder.id, RuntimeState::Watching).await;

        // Unmount the simulated external volume -> folder immediately enters Unavailable.
        test_support::remove_dir_all(&volume_root);
        handle_workspace_event(
            WorkspaceSystemEvent::VolumeUnmounted {
                mount_point: volume_root.clone(),
            },
            &folders,
            &monitor,
            &scheduler,
        )
        .await;
        wait_for_folder_state(&monitor, &folder.id, RuntimeState::Unavailable).await;

        // Re-mount the simulated volume with a newly appeared file -> immediately resumes & reconciles.
        std::fs::create_dir_all(&folder_path).expect("remount sim volume");
        let arrival = folder_path.join("holiday.png");
        test_support::write_file(&arrival, b"\x89PNG\r\n\x1a\nmock");
        handle_workspace_event(
            WorkspaceSystemEvent::VolumeMounted {
                mount_point: volume_root,
            },
            &folders,
            &monitor,
            &scheduler,
        )
        .await;
        wait_for_folder_state(&monitor, &folder.id, RuntimeState::Watching).await;

        for _ in 0..80 {
            let _ = pipeline.drain_and_reconcile().await;
            if dest_dir.join("holiday.png").exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            dest_dir.join("holiday.png").exists(),
            "VolumeMounted must resume monitoring, reconcile missed files, and run pipeline"
        );
        monitor.shutdown().await;
    }

    #[test]
    fn r_sys_03_macos_workspace_observer_starts_and_drops_cleanly() {
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let guard = start_workspace_observers(move |_ev| {
            flag.store(true, Ordering::SeqCst);
        });
        drop(guard);
        assert!(!called.load(Ordering::SeqCst));
    }
}
