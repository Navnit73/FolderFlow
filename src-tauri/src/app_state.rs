//! Process-wide state shared by commands (and later the tray and background engine).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tracing_appender::non_blocking::WorkerGuard;

use crate::categories::CategoryService;
use crate::database::{self, Db};
use crate::engine::activity::{load_path_authority, ActivityService};
use crate::engine::classifier::Classifier;
use crate::engine::monitor::{MonitorDeps, MonitorService};
use crate::engine::paths::RootPolicy;
use crate::engine::pipeline::{AutomationPipeline, DEFAULT_PIPELINE_QUEUE_CAPACITY};
use crate::engine::planner::PreviewStore;
use crate::engine::recovery::recover_startup;
use crate::engine::safe_fs::SafeFs;
use crate::engine::scheduler::SchedulerService;
use crate::error::{AppError, AppResult};
use crate::events::{CompositeObserver, TauriNotificationSink, TauriObserver};
use crate::folders::FolderService;
use crate::logging;
use crate::notifications::{
    CompositeNotificationSink, MacOsNativeNotifier, NotificationConfig, NotificationService,
    NotificationSink,
};
use crate::onboarding::OnboardingService;
use crate::rules::RuleService;
use crate::security::{LicenseService, MacOsKeychain, UnconfiguredOnlineProvider};
use crate::settings::{LogLevel, Settings, SettingsService};
use crate::system::LoginItemService;
use crate::utils::clock::{Clock, SystemClock};

pub const DB_FILE_NAME: &str = "folderflow.db";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
}

pub struct AppState {
    pub db: Db,
    pub settings: SettingsService,
    pub categories: CategoryService,
    pub folders: FolderService,
    pub rules: RuleService,
    pub activity: ActivityService,
    pub monitor: Arc<MonitorService>,
    pub pipeline: Arc<AutomationPipeline>,
    pub scheduler: Arc<SchedulerService>,
    pub notifications: Arc<NotificationService>,
    pub preview_store: PreviewStore,
    pub onboarding: OnboardingService,
    pub license: Arc<LicenseService>,
    pub login_item: LoginItemService,
    pub quit_requested: Arc<AtomicBool>,
    pub clock: Arc<dyn Clock>,
    pub paths: AppPaths,
    pub root_policy: RootPolicy,
    _log_guard: Mutex<Option<WorkerGuard>>,
}

impl AppState {
    /// Reloads authorized source and destination roots from SQLite and propagates them
    /// to long-lived automation services (`pipeline`, `scheduler`).
    pub async fn refresh_authority(&self) -> AppResult<()> {
        let auth = load_path_authority(&self.db).await?;
        self.pipeline.set_authority(auth.clone());
        self.scheduler.set_authority(auth);
        Ok(())
    }

    /// Rebuilds the live file classifier from SQLite categories and propagates it
    /// to `monitor`, `pipeline`, and `scheduler` (`FR-170`).
    pub async fn refresh_classifier(&self) -> AppResult<()> {
        let cats = self.categories.list().await?;
        let classifier = Arc::new(Classifier::new(cats));
        self.monitor.set_classifier(classifier.clone());
        self.pipeline.set_classifier(classifier.clone());
        self.scheduler.set_classifier(classifier);
        Ok(())
    }

    /// Pushes updated settings into all live runtime services without requiring a restart.
    pub fn sync_settings(&self, s: &Settings) {
        self.monitor.set_settings(s.monitoring.clone());
        self.pipeline
            .set_conflict_policy(s.organization.conflict_policy);
        self.scheduler
            .set_conflict_policy(s.organization.conflict_policy);
        self.notifications
            .update_config(NotificationConfig::from_app_settings(s));
    }

    /// Reloads settings, paths, classifier, and folder watchers after a config import.
    pub async fn refresh_after_config_import(&self) -> AppResult<()> {
        let current = self.settings.get().await?;
        self.sync_settings(&current);
        self.refresh_authority().await?;
        self.refresh_classifier().await?;

        let desired = self.folders.list().await?;
        let desired_enabled: std::collections::HashSet<&str> = desired
            .iter()
            .filter(|f| f.enabled)
            .map(|f| f.id.as_str())
            .collect();
        for rt in self.monitor.status().await.folders {
            if !desired_enabled.contains(rt.folder_id.as_str()) {
                self.monitor.stop(&rt.folder_id).await;
            }
        }
        for folder in desired.into_iter().filter(|f| f.enabled) {
            self.monitor.start(folder).await;
        }
        Ok(())
    }

    pub async fn init(app: &AppHandle) -> AppResult<Self> {
        let resolver = app.path();
        let internal = |what: &str, e: tauri::Error| AppError::Internal(format!("{what}: {e}"));
        let data_dir = resolver
            .app_data_dir()
            .map_err(|e| internal("app data dir", e))?;
        let log_dir = resolver
            .app_log_dir()
            .map_err(|e| internal("app log dir", e))?;
        let home = resolver.home_dir().map_err(|e| internal("home dir", e))?;
        let paths = AppPaths {
            db_path: data_dir.join(DB_FILE_NAME),
            data_dir,
            log_dir,
        };

        let db = database::open(&paths.db_path).await?;
        let settings = SettingsService::new(db.clone());
        let current = settings.get().await?;

        let guard = logging::init(
            &paths.log_dir,
            current.privacy.log_retention_days,
            current.privacy.log_level == LogLevel::Verbose,
        )?;
        tracing::info!(target: "folderflow::app", version = env!("CARGO_PKG_VERSION"), "FolderFlow started");

        let categories = CategoryService::new(db.clone());
        categories.ensure_builtins().await?;
        let classifier = Arc::new(Classifier::new(categories.list().await?));

        let root_policy = RootPolicy {
            home,
            app_data_dir: paths.data_dir.clone(),
        };
        let folders = FolderService::new(db.clone(), root_policy.clone());
        let rules = RuleService::new(db.clone(), root_policy.clone());

        let safe_fs = Arc::new(SafeFs::production());
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let notification_sink: Arc<dyn NotificationSink> = Arc::new(CompositeNotificationSink {
            ui_sink: Arc::new(TauriNotificationSink { app: app.clone() }),
            native: Arc::new(MacOsNativeNotifier::new()),
        });
        let notifications = Arc::new(NotificationService::new(
            clock.clone(),
            notification_sink,
            NotificationConfig::from_app_settings(&current),
        ));
        let license = Arc::new(LicenseService::new(
            Arc::new(MacOsKeychain::new()),
            Arc::new(UnconfiguredOnlineProvider),
            clock.clone(),
        )?);
        let login_item = LoginItemService::production();
        let quit_requested = Arc::new(AtomicBool::new(false));
        let activity = ActivityService::new(db.clone(), safe_fs.clone(), clock.clone());
        let preview_store = PreviewStore::new();
        let onboarding = OnboardingService::new(
            db.clone(),
            root_policy.clone(),
            clock.clone(),
            preview_store.clone(),
        );

        // Run startup recovery BEFORE starting folder watchers (`SAFETY_MODEL.md` § Startup recovery).
        let auth = activity.authority().await?;
        let scan_roots: Vec<PathBuf> = auth.roots().iter().map(|r| r.path.clone()).collect();
        let recovery_report = recover_startup(
            activity.organizer().ledger(),
            safe_fs.as_ref(),
            clock.as_ref(),
            &scan_roots,
        )
        .await?;
        if !recovery_report.completed.is_empty()
            || !recovery_report.rolled_back.is_empty()
            || !recovery_report.failed.is_empty()
            || recovery_report.orphan_partials_trashed > 0
        {
            tracing::warn!(
                target: "folderflow::recovery",
                completed = recovery_report.completed.len(),
                rolled_back = recovery_report.rolled_back.len(),
                failed = recovery_report.failed.len(),
                orphans_trashed = recovery_report.orphan_partials_trashed,
                "startup recovery resolved interrupted operations"
            );
        }

        let pipeline = Arc::new(AutomationPipeline::new(
            db.clone(),
            activity.organizer().clone(),
            classifier.clone(),
            auth.clone(),
            DEFAULT_PIPELINE_QUEUE_CAPACITY,
        ));
        pipeline.set_conflict_policy(current.organization.conflict_policy);
        pipeline.set_notifier(notifications.clone());

        let scheduler = Arc::new(SchedulerService::new(
            db.clone(),
            activity.organizer().clone(),
            classifier.clone(),
            auth,
            clock.clone(),
        ));
        scheduler.set_conflict_policy(current.organization.conflict_policy);
        scheduler.set_notifier(notifications.clone());

        let observer = Arc::new(CompositeObserver {
            tauri: TauriObserver::new(app.clone(), notifications.clone()),
            pipeline: pipeline.clone(),
        });
        let monitor = Arc::new(MonitorService::new(MonitorDeps::new(
            db.clone(),
            classifier,
            current.monitoring.clone(),
            observer,
        )));
        if !current.general.start_monitoring_automatically {
            monitor.pause_all().await;
            pipeline.set_paused(true);
        }
        for folder in folders.list().await?.into_iter().filter(|f| f.enabled) {
            monitor.start(folder).await;
        }

        // Background automation pipeline worker woken by stabilized file events.
        let pipeline_bg = pipeline.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                pipeline_bg.notified().await;
                if let Err(e) = pipeline_bg.drain_and_reconcile().await {
                    tracing::warn!(
                        target: "folderflow::pipeline",
                        error = %e,
                        "automation pipeline drain error"
                    );
                }
            }
        });

        // Background scheduler loop: run `OnLaunch` schedules once, then tick every 15s.
        let scheduler_bg = scheduler.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = scheduler_bg.run_on_launch().await {
                tracing::warn!(
                    target: "folderflow::scheduler",
                    error = %e,
                    "on_launch scheduler pass failed"
                );
            }
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                if let Err(e) = scheduler_bg.tick_once().await {
                    tracing::warn!(
                        target: "folderflow::scheduler",
                        error = %e,
                        "scheduler tick failed"
                    );
                }
            }
        });

        // Background notification batch flusher: ticks every 2s to emit due batched summaries.
        let notif_bg = notifications.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let _ = notif_bg.flush_due();
            }
        });

        Ok(Self {
            db,
            settings,
            categories,
            folders,
            rules,
            activity,
            monitor,
            pipeline,
            scheduler,
            notifications,
            preview_store,
            onboarding,
            license,
            login_item,
            quit_requested,
            clock,
            paths,
            root_policy,
            _log_guard: Mutex::new(Some(guard)),
        })
    }
}
