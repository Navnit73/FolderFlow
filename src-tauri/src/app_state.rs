//! Process-wide state shared by commands (and later the tray and background engine).

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, Runtime};
use tracing_appender::non_blocking::WorkerGuard;

use crate::database::{self, Db};
use crate::error::{AppError, AppResult};
use crate::logging;
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
    pub paths: AppPaths,
    _log_guard: Mutex<Option<WorkerGuard>>,
}

impl AppState {
    pub async fn init<R: Runtime>(app: &AppHandle<R>) -> AppResult<Self> {
        let resolver = app.path();
        let data_dir = resolver
            .app_data_dir()
            .map_err(|e| AppError::Internal(format!("app data dir: {e}")))?;
        let log_dir = resolver
            .app_log_dir()
            .map_err(|e| AppError::Internal(format!("app log dir: {e}")))?;
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

        Ok(Self {
            db,
            settings,
            paths,
            _log_guard: Mutex::new(Some(guard)),
        })
    }
}
