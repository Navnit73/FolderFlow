//! Typed Rust → UI events (docs/ARCHITECTURE.md §6 "Events").

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

use crate::engine::monitor::{FolderRuntime, MonitorObserver, RuntimeState};
use crate::engine::pipeline::AutomationPipeline;
use crate::engine::scanner::ScannedFile;
use crate::notifications::{
    NotificationInput, NotificationRecord, NotificationService, NotificationSink,
};

/// A folder's runtime state changed (watching, paused, unavailable, …).
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
pub struct FolderStatusEvent(pub FolderRuntime);

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FileActivityKind {
    Stable,
    Removed,
}

/// A file became stable or disappeared in a monitored folder (no paths sent).
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct FolderActivityEvent {
    pub folder_id: String,
    pub kind: FileActivityKind,
}

/// Progress update emitted during duplicate scanning (`dup://progress` in ARCHITECTURE.md §6).
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct DupProgressEvent {
    pub hashed: u32,
    pub total: u32,
    #[specta(type = f64)]
    pub bytes: u64,
}

/// Batched / throttled user notification delivered to the frontend shell.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
pub struct AppNotificationEvent(pub NotificationRecord);

/// Menu bar tray navigation event emitted when the user opens a view from the tray (`P7-T01`).
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct TrayNavigateEvent {
    pub path: String,
}

pub struct TauriNotificationSink {
    pub app: AppHandle,
}

impl NotificationSink for TauriNotificationSink {
    fn emit(&self, record: &NotificationRecord) {
        if let Err(e) = AppNotificationEvent(record.clone()).emit(&self.app) {
            tracing::debug!(target: "folderflow::events", error = %e, "notification emit failed");
        }
    }
}

pub struct TauriObserver {
    pub app: AppHandle,
    pub notifications: Arc<NotificationService>,
    last_states: Mutex<HashMap<String, RuntimeState>>,
}

impl TauriObserver {
    pub fn new(app: AppHandle, notifications: Arc<NotificationService>) -> Self {
        Self {
            app,
            notifications,
            last_states: Mutex::new(HashMap::new()),
        }
    }
}

impl MonitorObserver for TauriObserver {
    fn on_status(&self, status: &FolderRuntime) {
        if let Err(e) = FolderStatusEvent(status.clone()).emit(&self.app) {
            tracing::debug!(target: "folderflow::events", error = %e, "emit failed");
        }
        let prev = {
            let mut guard = self.last_states.lock().unwrap_or_else(|e| e.into_inner());
            guard.insert(status.folder_id.clone(), status.state)
        };
        if prev != Some(status.state) {
            let msg = status
                .last_error
                .as_deref()
                .unwrap_or("Check monitored folder access.");
            match status.state {
                RuntimeState::PermissionDenied => {
                    self.notifications
                        .notify(NotificationInput::permission_issue(
                            Some(status.folder_id.clone()),
                            msg,
                            None,
                        ));
                }
                RuntimeState::Unavailable => {
                    self.notifications
                        .notify(NotificationInput::monitoring_stopped(
                            Some(status.folder_id.clone()),
                            msg,
                            None,
                        ));
                }
                RuntimeState::Error => {
                    self.notifications
                        .notify(NotificationInput::monitoring_stopped(
                            Some(status.folder_id.clone()),
                            msg,
                            None,
                        ));
                }
                RuntimeState::Starting
                | RuntimeState::Scanning
                | RuntimeState::Watching
                | RuntimeState::Paused
                | RuntimeState::Stopped => {}
            }
            let app = self.app.clone();
            tauri::async_runtime::spawn(async move {
                crate::system::tray::refresh_tray_async(app).await;
            });
        }
    }
    fn on_stable(&self, folder_id: &str, _file: &ScannedFile) {
        let _ = FolderActivityEvent {
            folder_id: folder_id.to_owned(),
            kind: FileActivityKind::Stable,
        }
        .emit(&self.app);
    }
    fn on_removed(&self, folder_id: &str, _path: &Path) {
        let _ = FolderActivityEvent {
            folder_id: folder_id.to_owned(),
            kind: FileActivityKind::Removed,
        }
        .emit(&self.app);
    }
}

/// Broadcasts monitor callbacks to both the UI event emitter and the automation pipeline.
pub struct CompositeObserver {
    pub tauri: TauriObserver,
    pub pipeline: Arc<AutomationPipeline>,
}

impl MonitorObserver for CompositeObserver {
    fn on_status(&self, status: &FolderRuntime) {
        self.tauri.on_status(status);
        self.pipeline.on_status(status);
    }

    fn on_stable(&self, folder_id: &str, file: &ScannedFile) {
        self.tauri.on_stable(folder_id, file);
        self.pipeline.on_stable(folder_id, file);
    }

    fn on_removed(&self, folder_id: &str, path: &Path) {
        self.tauri.on_removed(folder_id, path);
        self.pipeline.on_removed(folder_id, path);
    }
}
