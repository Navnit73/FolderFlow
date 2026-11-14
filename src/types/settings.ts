import type {
  AppInfo,
  ConflictPolicy,
  CpuPreference,
  Density,
  ExecutionMode,
  LogLevel,
  NotificationEventPrefs_Serialize,
  RuleExecutionMode,
  Settings_Serialize,
  ThemeMode,
  UiError_Serialize,
} from "@/bindings";

/** Canonical frontend shapes (the fully-populated "serialize" side of the Rust types). */
export type Settings = Settings_Serialize;
export type UiError = UiError_Serialize;
export type NotificationEventPrefs = NotificationEventPrefs_Serialize;
export type {
  AppInfo,
  ConflictPolicy,
  CpuPreference,
  Density,
  ExecutionMode,
  LogLevel,
  RuleExecutionMode,
  ThemeMode,
};
