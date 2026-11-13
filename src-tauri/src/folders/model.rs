use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderHealth {
    Unknown,
    Healthy,
    Paused,
    Unavailable,
    PermissionDenied,
    Error,
}

impl FolderHealth {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Healthy => "healthy",
            Self::Paused => "paused",
            Self::Unavailable => "unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::Error => "error",
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "healthy" => Self::Healthy,
            "paused" => Self::Paused,
            "unavailable" => Self::Unavailable,
            "permission_denied" => Self::PermissionDenied,
            "error" => Self::Error,
            _ => Self::Unknown,
        }
    }
}

/// Per-folder options (FR-011). Stored in `monitored_folders.settings_json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct FolderSettings {
    /// If non-empty, only these extensions are processed.
    pub include_extensions: Vec<String>,
    pub exclude_extensions: Vec<String>,
    /// Absolute paths of subfolders to skip (must be inside the folder).
    pub excluded_subfolders: Vec<String>,
    /// Overrides the global stability interval for this folder.
    pub stability_interval_secs: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MonitoredFolder {
    pub id: String,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub recursive: bool,
    pub settings: FolderSettings,
    pub health: FolderHealth,
    pub last_error: Option<String>,
    pub file_count: u32,
    /// Sum of tracked file sizes in bytes.
    #[specta(type = f64)]
    pub total_bytes: i64,
    /// Unix epoch milliseconds.
    #[specta(type = Option<f64>)]
    pub last_scanned_at: Option<i64>,
    #[specta(type = f64)]
    pub created_at: i64,
}

/// Editable fields. `None` = unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub recursive: Option<bool>,
    pub settings: Option<FolderSettings>,
}

/// Standard macOS folders offered in onboarding (FR-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum KnownFolder {
    Downloads,
    Desktop,
    Documents,
    Pictures,
    Movies,
    Music,
}

/// Result of probing whether FolderFlow can read a folder (FR-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess {
    Granted,
    Denied,
    Missing,
    NotAFolder,
}
