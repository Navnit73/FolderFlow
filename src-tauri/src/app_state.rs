//! Process-wide state shared by commands (and later the tray and background engine).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager};
use tracing_appender::non_blocking::WorkerGuard;

use crate::categories::CategoryService;
use crate::database::{self, Db};
use crate::engine::classifier::Classifier;
use crate::engine::monitor::{MonitorDeps, MonitorService};
use crate::engine::paths::RootPolicy;
use crate::error::{AppError, AppResult};
use crate::events::TauriObserver;
use crate::folders::FolderService;
use crate::logging;
use crate::rules::RuleService;
use crate::settings::{LogLevel, SettingsService};

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
    pub monitor: Arc<MonitorService>,
    pub paths: AppPaths,
    _log_guard: Mutex<Option<WorkerGuard>>,
}

impl AppState {
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
        let rules = RuleService::new(db.clone(), root_policy);

        let monitor = Arc::new(MonitorService::new(MonitorDeps::new(
            db.clone(),
            classifier,
            current.monitoring.clone(),
            Arc::new(TauriObserver { app: app.clone() }),
        )));
        if !current.general.start_monitoring_automatically {
            monitor.pause_all().await;
        }
        for folder in folders.list().await?.into_iter().filter(|f| f.enabled) {
            monitor.start(folder).await;
        }

        Ok(Self {
            db,
            settings,
            categories,
            folders,
            rules,
            monitor,
            paths,
            _log_guard: Mutex::new(Some(guard)),
        })
    }
}
