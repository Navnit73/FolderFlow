import type { LicenseStatus_Serialize } from "@/bindings";
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
    notifications: {
      enabled: true,
      batchWindowSecs: 10,
      cooldownSecs: 30,
      largeBatchThreshold: 25,
      perType: {
        successSummary: true,
        largeBatchDone: true,
        failure: true,
        permissionIssue: true,
        destinationUnavailable: true,
        duplicatesFound: true,
        monitoringStopped: true,
      },
    },
  };
}

export const appInfoFixture: AppInfo = {
  version: "1.0.0",
  dataDir: "/tmp/ff/data",
  logDir: "/tmp/ff/logs",
  databasePath: "/tmp/ff/data/folderflow.db",
  schemaVersion: 1,
};

export function defaultLicenseStatus(over: Partial<LicenseStatus_Serialize> = {}): LicenseStatus_Serialize {
  return {
    state: "unlicensed",
    licensed: false,
    edition: null,
    source: null,
    licenseId: null,
    maskedKey: null,
    customerHint: null,
    providerName: null,
    onlineProviderConfigured: false,
    deviceLimit: null,
    activatedAtMs: null,
    expiresAtMs: null,
    lastError: null,
    ...over,
  };
}
