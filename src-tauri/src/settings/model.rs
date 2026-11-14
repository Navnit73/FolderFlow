use std::collections::HashMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

// ---------------------------------------------------------------- enums

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    #[default]
    AutoRename,
    Skip,
    Ask,
    KeepBoth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleExecutionMode {
    #[default]
    FirstMatch,
    Continue,
    StopAfterMove,
    StopAfterAction,
    Sequential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CpuPreference {
    Low,
    #[default]
    Balanced,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    #[default]
    Normal,
    Verbose,
}

// ---------------------------------------------------------------- sections

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub start_monitoring_automatically: bool,
    pub start_minimized: bool,
    pub show_notifications: bool,
    pub show_menu_bar_icon: bool,
    pub confirm_bulk_operations: bool,
}
impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            start_monitoring_automatically: true,
            start_minimized: false,
            show_notifications: true,
            show_menu_bar_icon: true,
            confirm_bulk_operations: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct MonitoringSettings {
    /// Seconds between stability checks (FR-015).
    pub stability_interval_secs: u32,
    /// Consecutive unchanged checks required before a file is considered stable.
    pub stability_checks: u32,
    pub recursive_by_default: bool,
    pub reconciliation_interval_mins: u32,
    pub ignore_hidden_files: bool,
    pub ignore_temporary_files: bool,
    pub max_concurrent_ops: u32,
}
impl Default for MonitoringSettings {
    fn default() -> Self {
        Self {
            stability_interval_secs: 3,
            stability_checks: 3,
            recursive_by_default: false,
            reconciliation_interval_mins: 30,
            ignore_hidden_files: true,
            ignore_temporary_files: true,
            max_concurrent_ops: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct OrganizationSettings {
    /// Absolute path; validated against approved roots from Phase 2.
    pub default_destination: Option<String>,
    pub conflict_policy: ConflictPolicy,
    pub naming_template: String,
    pub execution_mode: RuleExecutionMode,
    pub auto_create_folders: bool,
}
impl Default for OrganizationSettings {
    fn default() -> Self {
        Self {
            default_destination: None,
            conflict_policy: ConflictPolicy::AutoRename,
            naming_template: "{original_name}".into(),
            execution_mode: RuleExecutionMode::FirstMatch,
            auto_create_folders: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct PerformanceSettings {
    pub cpu_preference: CpuPreference,
    pub background_processing: bool,
    pub hash_on_scan: bool,
    pub max_scan_concurrency: u32,
}
impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            cpu_preference: CpuPreference::Balanced,
            background_processing: true,
            hash_on_scan: false,
            max_scan_concurrency: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    pub theme: ThemeMode,
    pub density: Density,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings {
    pub redact_paths_in_logs: bool,
    pub log_level: LogLevel,
    pub log_retention_days: u32,
}
impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            redact_paths_in_logs: true,
            log_level: LogLevel::Normal,
            log_retention_days: 14,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationEventPrefs {
    pub success_summary: bool,
    pub large_batch_done: bool,
    pub failure: bool,
    pub permission_issue: bool,
    pub destination_unavailable: bool,
    pub duplicates_found: bool,
    pub monitoring_stopped: bool,
}
impl Default for NotificationEventPrefs {
    fn default() -> Self {
        Self {
            success_summary: true,
            large_batch_done: true,
            failure: true,
            permission_issue: true,
            destination_unavailable: true,
            duplicates_found: true,
            monitoring_stopped: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    pub enabled: bool,
    /// Seconds to coalesce related events into a single summary notification (FR-202).
    pub batch_window_secs: u32,
    /// Minimum seconds between emitted notifications of the same event type (FR-202).
    pub cooldown_secs: u32,
    /// Minimum files in a batch before emitting a `large_batch_done` notification (FR-200).
    pub large_batch_threshold: u32,
    pub per_type: NotificationEventPrefs,
}
impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            batch_window_secs: 10,
            cooldown_secs: 30,
            large_batch_threshold: 25,
            per_type: NotificationEventPrefs::default(),
        }
    }
}

// ---------------------------------------------------------------- root

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub monitoring: MonitoringSettings,
    pub organization: OrganizationSettings,
    pub performance: PerformanceSettings,
    pub appearance: AppearanceSettings,
    pub privacy: PrivacySettings,
    pub notifications: NotificationSettings,
}

fn check_range(field: &str, value: u32, min: u32, max: u32) -> AppResult<()> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(AppError::validation(
            field,
            format!("Must be between {min} and {max}."),
        ))
    }
}

fn section<T: DeserializeOwned + Default>(rows: &HashMap<String, String>, key: &str) -> T {
    match rows.get(key) {
        None => T::default(),
        Some(raw) => serde_json::from_str(raw).unwrap_or_else(|e| {
            tracing::warn!(target: "folderflow::settings", section = key, error = %e,
                "corrupted settings section; using defaults");
            T::default()
        }),
    }
}

impl Settings {
    pub fn validate(&self) -> AppResult<()> {
        let m = &self.monitoring;
        check_range(
            "monitoring.stabilityIntervalSecs",
            m.stability_interval_secs,
            1,
            300,
        )?;
        check_range("monitoring.stabilityChecks", m.stability_checks, 2, 10)?;
        check_range(
            "monitoring.reconciliationIntervalMins",
            m.reconciliation_interval_mins,
            5,
            1440,
        )?;
        check_range("monitoring.maxConcurrentOps", m.max_concurrent_ops, 1, 8)?;
        check_range(
            "performance.maxScanConcurrency",
            self.performance.max_scan_concurrency,
            1,
            16,
        )?;
        check_range(
            "privacy.logRetentionDays",
            self.privacy.log_retention_days,
            1,
            90,
        )?;
        check_range(
            "notifications.batchWindowSecs",
            self.notifications.batch_window_secs,
            0,
            300,
        )?;
        check_range(
            "notifications.cooldownSecs",
            self.notifications.cooldown_secs,
            0,
            3600,
        )?;
        check_range(
            "notifications.largeBatchThreshold",
            self.notifications.large_batch_threshold,
            2,
            10_000,
        )?;

        let o = &self.organization;
        let t = o.naming_template.trim();
        if t.is_empty() || t.len() > 255 {
            return Err(AppError::validation(
                "organization.namingTemplate",
                "Naming template must be 1–255 characters.",
            ));
        }
        if t.contains('/') || t.contains('\0') {
            return Err(AppError::validation(
                "organization.namingTemplate",
                "Naming template cannot contain '/' or NUL characters.",
            ));
        }
        if let Some(dest) = &o.default_destination {
            if !std::path::Path::new(dest).is_absolute() {
                return Err(AppError::validation(
                    "organization.defaultDestination",
                    "Default destination must be an absolute folder path.",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn from_sections(rows: HashMap<String, String>) -> Self {
        Self {
            general: section(&rows, "general"),
            monitoring: section(&rows, "monitoring"),
            organization: section(&rows, "organization"),
            performance: section(&rows, "performance"),
            appearance: section(&rows, "appearance"),
            privacy: section(&rows, "privacy"),
            notifications: section(&rows, "notifications"),
        }
    }

    pub(crate) fn to_sections(&self) -> AppResult<Vec<(&'static str, String)>> {
        Ok(vec![
            ("general", serde_json::to_string(&self.general)?),
            ("monitoring", serde_json::to_string(&self.monitoring)?),
            ("organization", serde_json::to_string(&self.organization)?),
            ("performance", serde_json::to_string(&self.performance)?),
            ("appearance", serde_json::to_string(&self.appearance)?),
            ("privacy", serde_json::to_string(&self.privacy)?),
            ("notifications", serde_json::to_string(&self.notifications)?),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Settings::default()
            .validate()
            .expect("defaults must validate");
    }

    #[test]
    fn naming_template_rejects_path_separators_and_relative_destination() {
        let mut s = Settings::default();
        s.organization.naming_template = "../{original_name}".into();
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.organization.default_destination = Some("Documents/Organized".into());
        assert!(s.validate().is_err());
    }

    #[test]
    fn json_uses_camel_case_and_snake_case_enums() {
        let v = serde_json::to_value(Settings::default()).expect("json");
        assert_eq!(v["monitoring"]["stabilityIntervalSecs"], 3);
        assert_eq!(v["organization"]["conflictPolicy"], "auto_rename");
        assert_eq!(v["appearance"]["theme"], "system");
    }
}
