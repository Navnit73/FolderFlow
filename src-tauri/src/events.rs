//! Typed Rust → UI events (docs/ARCHITECTURE.md §6 "Events").

use std::path::Path;

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

use crate::engine::monitor::{FolderRuntime, MonitorObserver};
use crate::engine::scanner::ScannedFile;

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

pub struct TauriObserver {
    pub app: AppHandle,
}

impl MonitorObserver for TauriObserver {
    fn on_status(&self, status: &FolderRuntime) {
        if let Err(e) = FolderStatusEvent(status.clone()).emit(&self.app) {
            tracing::debug!(target: "folderflow::events", error = %e, "emit failed");
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
