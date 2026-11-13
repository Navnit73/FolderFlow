import type { AppInfo, Settings } from "@/types/settings";

/** Mirrors Rust `Settings::default()` (src-tauri/src/settings/model.rs). */
export function defaultSettings(): Settings {
  return {
    general: {
      launchAtLogin: false,
      startMonitoringAutomatically: true,
      startMinimized: false,
      showNotifications: true,
      showMenuBarIcon: true,
      confirmBulkOperations: true,
    },
    monitoring: {
      stabilityIntervalSecs: 3,
      stabilityChecks: 3,
      recursiveByDefault: false,
      reconciliationIntervalMins: 30,
      ignoreHiddenFiles: true,
      ignoreTemporaryFiles: true,
      maxConcurrentOps: 2,
    },
    organization: {
      defaultDestination: null,
      conflictPolicy: "auto_rename",
      namingTemplate: "{original_name}",
      executionMode: "first_match",
      autoCreateFolders: true,
    },
    performance: { cpuPreference: "balanced", backgroundProcessing: true, hashOnScan: false, maxScanConcurrency: 4 },
    appearance: { theme: "system", density: "comfortable" },
    privacy: { redactPathsInLogs: true, logLevel: "normal", logRetentionDays: 14 },
  };
}

export const appInfoFixture: AppInfo = {
  version: "0.1.0",
  dataDir: "/tmp/ff/data",
  logDir: "/tmp/ff/logs",
  databasePath: "/tmp/ff/data/folderflow.db",
  schemaVersion: 1,
};
