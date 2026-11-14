//! Schema definitions for versioned JSON configuration backup and import (`P6-T05`, FR-190).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::categories::Category;
use crate::database::repo_exclusions::ExclusionKind;
use crate::folders::FolderSettings;
use crate::rules::models::{Action, ConditionNode, DateSource, ExecutionMode, RuleOrigin};
use crate::settings::Settings;

/// Current configuration bundle schema version (`FR-190`).
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFolder {
    pub path: String,
    pub name: String,
    pub enabled: bool,
    pub recursive: bool,
    pub settings: FolderSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportedExclusion {
    /// `None` for global exclusions; `Some(folder_path)` for per-folder exclusions.
    pub folder_path: Option<String>,
    pub path: String,
    pub exclusion_type: ExclusionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportedRule {
    pub name: String,
    pub description: Option<String>,
    /// Exported for clarity; ALWAYS forced `false` on import (`FR-192`).
    pub enabled: bool,
    pub priority: i32,
    #[serde(default)]
    pub source_folders: Vec<String>,
    pub conditions: ConditionNode,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub date_source: DateSource,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default)]
    pub origin: RuleOrigin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBundle {
    pub schema_version: u32,
    #[specta(type = f64)]
    pub exported_at: i64,
    pub app_version: String,
    pub settings: Settings,
    pub folders: Vec<ExportedFolder>,
    pub categories: Vec<Category>,
    pub exclusions: Vec<ExportedExclusion>,
    pub rules: Vec<ExportedRule>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConfigImportMode {
    #[default]
    Merge,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDiffItem {
    pub section: String,
    pub current: String,
    pub incoming: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfigImportPreview {
    pub schema_version: u32,
    #[specta(type = f64)]
    pub exported_at: i64,
    pub settings_changes: Vec<SettingsDiffItem>,
    pub folders_to_add: Vec<String>,
    pub folders_to_update: Vec<String>,
    pub folders_unchanged: Vec<String>,
    pub categories_to_add: Vec<String>,
    pub categories_to_update: Vec<String>,
    pub exclusions_to_add: Vec<String>,
    pub exclusions_unchanged: Vec<String>,
    pub rules_to_add: Vec<String>,
    pub rules_to_update: Vec<String>,
    pub forced_disabled_rule_count: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfigImportReport {
    pub mode: ConfigImportMode,
    pub folders_imported: u32,
    pub categories_imported: u32,
    pub exclusions_imported: u32,
    pub rules_imported_disabled: u32,
    pub warnings: Vec<String>,
}
