import type { Page } from "@playwright/test";
import type {
  ActivityPage_Serialize,
  AppInfo,
  Category_Serialize,
  DashboardStats_Serialize,
  DuplicateGroupsResponse_Serialize,
  FolderAccess,
  FolderSummary,
  LicenseStatus_Serialize,
  MonitoredFolder_Serialize,
  MonitoringStatus_Serialize,
  OnboardingCompleteReport_Serialize,
  OnboardingScanReport,
  OnboardingState_Serialize,
  OperationView_Serialize,
  PlanExecutionReport_Serialize,
  PlanItem,
  PlanItemsPage,
  PlanSummary,
  ReviewItemView_Serialize,
  RuleDraft_Deserialize,
  RuleTemplate_Serialize,
  RuleTestReport,
  Rule_Serialize,
  Schedule_Serialize,
  Settings_Serialize,
  SmartOrganizerCatalog,
  UndoReport_Serialize,
  ValidationIssue,
} from "@/bindings";

export interface TauriMockOptions {
  onboardingCompleted: boolean;
}

interface MockState {
  onboarding: OnboardingState_Serialize;
  settings: Settings_Serialize;
  license: LicenseStatus_Serialize;
  folders: MonitoredFolder_Serialize[];
  runtimePaused: boolean;
  categories: Category_Serialize[];
  rules: Rule_Serialize[];
  presets: RuleTemplate_Serialize[];
  operations: OperationView_Serialize[];
  reviews: ReviewItemView_Serialize[];
  schedules: Schedule_Serialize[];
  lastPlanItems: PlanItem[];
  planCounter: number;
}

const NOW_MS = 1_735_689_600_000;

function makeInitialState(opts: TauriMockOptions): MockState {
  const defaultFolders: MonitoredFolder_Serialize[] = opts.onboardingCompleted
    ? [
        {
          id: "f1",
          name: "Downloads",
          path: "/Users/demo/Downloads",
          enabled: true,
          recursive: false,
          settings: {
            includeExtensions: [],
            excludeExtensions: [],
            excludedSubfolders: [],
            stabilityIntervalSecs: null,
          },
          health: "healthy",
          lastError: null,
          fileCount: 4,
          totalBytes: 2_450_000,
          lastScannedAt: NOW_MS - 30_000,
          createdAt: NOW_MS - 86_400_000,
        },
        {
          id: "f2",
          name: "Desktop",
          path: "/Users/demo/Desktop",
          enabled: true,
          recursive: true,
          settings: {
            includeExtensions: [],
            excludeExtensions: ["tmp"],
            excludedSubfolders: [],
            stabilityIntervalSecs: 5,
          },
          health: "healthy",
          lastError: null,
          fileCount: 2,
          totalBytes: 850_000,
          lastScannedAt: NOW_MS - 60_000,
          createdAt: NOW_MS - 86_400_000,
        },
      ]
    : [];

  const presets: RuleTemplate_Serialize[] = [
    {
      id: "preset-screenshots",
      name: "Sort Screenshots by Date",
      description: "Moves Screenshot files into Pictures/Screenshots/{year}/{month}.",
      categoryHint: "images",
      draft: {
        name: "Sort Screenshots by Date",
        description: "Organize screenshots by year and month",
        enabled: false,
        sourceFolders: [],
        conditions: {
          type: "all",
          children: [
            {
              type: "leaf",
              condition: {
                kind: "text",
                field: "name",
                op: "starts_with",
                value: "Screenshot",
                case_sensitive: false,
              },
            },
          ],
        },
        actions: [
          {
            kind: "move_into_dynamic_folder",
            root: null,
            subpath_template: "Screenshots/{year}/{month}",
          },
        ],
        dateSource: "modified",
        executionMode: "first_match",
      },
    },
    {
      id: "preset-invoices",
      name: "Archive PDF Invoices",
      description: "Groups PDFs with invoice keywords into Documents/Finance/{year}.",
      categoryHint: "documents",
      draft: {
        name: "Archive PDF Invoices",
        description: "Collect invoice PDFs",
        enabled: false,
        sourceFolders: [],
        conditions: {
          type: "all",
          children: [
            {
              type: "leaf",
              condition: { kind: "extension_group", extensions: ["pdf"] },
            },
            {
              type: "leaf",
              condition: {
                kind: "text",
                field: "name",
                op: "contains",
                value: "invoice",
                case_sensitive: false,
              },
            },
          ],
        },
        actions: [
          {
            kind: "move_into_dynamic_folder",
            root: null,
            subpath_template: "Finance/{year}",
          },
        ],
        dateSource: "modified",
        executionMode: "first_match",
      },
    },
  ];

  const invoicePreset = presets[1];
  if (!invoicePreset) {
    throw new Error("Expected invoice preset fixture");
  }
  const rules: Rule_Serialize[] = opts.onboardingCompleted
    ? [
        {
          id: "r1",
          name: "Sort Invoices",
          description: "Route invoice PDFs to Finance",
          enabled: true,
          priority: 1,
          sourceFolders: ["f1"],
          conditions: invoicePreset.draft.conditions,
          actions: invoicePreset.draft.actions,
          dateSource: "modified",
          executionMode: "first_match",
          origin: "user",
          lastRunAt: NOW_MS - 300_000,
          statMatched: 3,
          statSucceeded: 3,
          statFailed: 0,
          createdAt: NOW_MS - 86_000_000,
          updatedAt: NOW_MS - 300_000,
        },
      ]
    : [];

  const categories: Category_Serialize[] = [
    {
      id: "documents",
      name: "Documents",
      description: "PDFs, text, and office documents",
      icon: "file-text",
      colorToken: "cat-documents",
      extensions: ["pdf", "doc", "docx", "txt"],
      destinationPath: "/Users/demo/Documents",
      priority: 100,
      builtin: true,
      enabled: true,
    },
    {
      id: "images",
      name: "Images",
      description: "Screenshots and photos",
      icon: "image",
      colorToken: "cat-images",
      extensions: ["png", "jpg", "jpeg", "webp"],
      destinationPath: "/Users/demo/Pictures",
      priority: 100,
      builtin: true,
      enabled: true,
    },
    {
      id: "archives",
      name: "Archives",
      description: "Compressed packages",
      icon: "archive",
      colorToken: "cat-archives",
      extensions: ["zip", "tar", "gz"],
      destinationPath: null,
      priority: 100,
      builtin: true,
      enabled: true,
    },
  ];

  const initialPlanItems: PlanItem[] = [
    {
      id: "plan-item-1",
      folderId: "f1",
      sourcePath: "/Users/demo/Downloads/Invoice_Jan.pdf",
      currentLocation: "/Users/demo/Downloads",
      originalName: "Invoice_Jan.pdf",
      proposedDestination: "/Users/demo/Documents/Finance/2025/Invoice_Jan.pdf",
      proposedName: "Invoice_Jan.pdf",
      size: 245_760,
      category: "documents",
      op: "move",
      ruleId: "r1",
      ruleName: "Sort Invoices",
      matchedConditions: ["extension == pdf", "name contains invoice"],
      plannedOps: [{ kind: "move", destination_dir: "/Users/demo/Documents/Finance/2025" }],
      conflicts: [],
      status: "ready",
      crossVolume: false,
      identity: { dev: 1, ino: 1001, size: 245_760, mtimeNs: NOW_MS * 1_000_000 },
    },
    {
      id: "plan-item-2",
      folderId: "f2",
      sourcePath: "/Users/demo/Desktop/Screenshot 2025-01-01.png",
      currentLocation: "/Users/demo/Desktop",
      originalName: "Screenshot 2025-01-01.png",
      proposedDestination: "/Users/demo/Pictures/Screenshots/2025/01/Screenshot 2025-01-01.png",
      proposedName: "Screenshot 2025-01-01.png",
      size: 512_000,
      category: "images",
      op: "move",
      ruleId: "r1",
      ruleName: "Sort Invoices",
      matchedConditions: ["name starts_with Screenshot"],
      plannedOps: [{ kind: "move", destination_dir: "/Users/demo/Pictures/Screenshots/2025/01" }],
      conflicts: [],
      status: "ready",
      crossVolume: false,
      identity: { dev: 1, ino: 1002, size: 512_000, mtimeNs: NOW_MS * 1_000_000 },
    },
  ];

  const operations: OperationView_Serialize[] = opts.onboardingCompleted
    ? [
        {
          id: "op-1",
          batchId: "batch-initial",
          operationType: "move",
          sourcePath: "/Users/demo/Downloads/Quarterly_Report.pdf",
          destinationPath: "/Users/demo/Documents/Quarterly_Report.pdf",
          originalName: "Quarterly_Report.pdf",
          newName: "Quarterly_Report.pdf",
          size: 409_600,
          contentHash: "blake3-initial",
          crossVolume: false,
          ruleId: "r1",
          ruleName: "Sort Invoices",
          status: "COMPLETED",
          reversible: true,
          reversedBy: null,
          errorCode: null,
          errorMessage: null,
          startedAt: NOW_MS - 120_000,
          completedAt: NOW_MS - 119_500,
        },
      ]
    : [];

  const reviews: ReviewItemView_Serialize[] = opts.onboardingCompleted
    ? [
        {
          id: "rev-1",
          path: "/Users/demo/Downloads/Statement.pdf",
          reason: "Destination file already exists (conflict policy: ask)",
          ruleId: "r1",
          proposed: {
            operationType: "move",
            sourcePath: "/Users/demo/Downloads/Statement.pdf",
            destinationPath: "/Users/demo/Documents/Statement.pdf",
            originalName: "Statement.pdf",
            newName: "Statement.pdf",
          },
          createdAt: NOW_MS - 90_000,
          resolvedAt: null,
        },
      ]
    : [];

  const schedules: Schedule_Serialize[] = opts.onboardingCompleted
    ? [
        {
          id: "sched-1",
          ruleId: "r1",
          folderId: "f1",
          scheduleType: "interval",
          config: {
            intervalSecs: 900,
            time: null,
            weekdays: [],
            quietHours: null,
            catchUp: "run_once",
          },
          enabled: true,
          lastRunAt: NOW_MS - 900_000,
          nextRunAt: NOW_MS + 900_000,
        },
      ]
    : [];

  return {
    onboarding: {
      currentStep: 1,
      completed: opts.onboardingCompleted,
      organizationPreference: "by_category",
      selectedPresetIds: ["preset-invoices"],
      automationConfirmed: opts.onboardingCompleted,
      completedAt: opts.onboardingCompleted ? NOW_MS - 86_000_000 : null,
    },
    settings: {
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
        defaultDestination: "/Users/demo/Documents",
        conflictPolicy: "auto_rename",
        namingTemplate: "{original_name}",
        executionMode: "first_match",
        autoCreateFolders: true,
      },
      performance: {
        cpuPreference: "balanced",
        backgroundProcessing: true,
        hashOnScan: false,
        maxScanConcurrency: 4,
      },
      appearance: {
        theme: "light",
        density: "comfortable",
      },
      privacy: {
        redactPathsInLogs: true,
        logLevel: "normal",
        logRetentionDays: 14,
      },
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
    },
    license: {
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
    },
    folders: defaultFolders,
    runtimePaused: false,
    categories,
    rules,
    presets,
    operations,
    reviews,
    schedules,
    lastPlanItems: initialPlanItems,
    planCounter: 1,
  };
}

/**
 * Installs a stateful `window.__TAURI_INTERNALS__` + `window.__TAURI_EVENT_PLUGIN_INTERNALS__`
 * mock before app scripts boot so every Tauri IPC command and event subscription runs cleanly
 * against an in-browser state machine under Vite preview.
 */
export async function installTauriMock(page: Page, options: TauriMockOptions): Promise<void> {
  const seed = makeInitialState(options);
  await page.addInitScript((initialState: MockState) => {
    const state: MockState = structuredClone(initialState);
    let callbackSeq = 1;
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners = new Map<string, Set<number>>();
    let nowSeq = 1_735_690_000_000;
    const tick = () => {
      nowSeq += 1000;
      return nowSeq;
    };

    const emitLocal = (event: string, payload: unknown) => {
      const ids = listeners.get(event);
      if (!ids) return;
      for (const id of ids) {
        const cb = callbacks.get(id);
        if (cb) {
          cb({ event, id, payload });
        }
      }
    };

    const buildStats = (): DashboardStats_Serialize => ({
      filesOrganizedTotal: state.operations.filter((o) => o.status === "COMPLETED").length,
      filesOrganizedToday: state.operations.filter((o) => o.status === "COMPLETED").length,
      filesOrganizedThisWeek: state.operations.filter((o) => o.status === "COMPLETED").length,
      totalBytesOrganized: state.operations
        .filter((o) => o.status === "COMPLETED")
        .reduce((sum, o) => sum + (o.size ?? 0), 0),
      activeFoldersCount: state.folders.filter((f) => f.enabled).length,
      totalFoldersCount: state.folders.length,
      activeRulesCount: state.rules.filter((r) => r.enabled).length,
      totalRulesCount: state.rules.length,
      pendingReviewCount: state.reviews.length,
      failedOperationsCount: state.operations.filter((o) => o.status === "FAILED").length,
      totalTrackedFiles: state.folders.reduce((s, f) => s + f.fileCount, 0),
      totalTrackedBytes: state.folders.reduce((s, f) => s + (f.totalBytes ?? 0), 0),
      duplicateGroupsCount: 1,
      duplicateWastedBytes: 512_000,
      categoryBreakdown: [
        {
          categoryId: "documents",
          categoryName: "Documents",
          colorToken: "cat-documents",
          fileCount: 3,
          totalBytes: 1_500_000,
        },
        {
          categoryId: "images",
          categoryName: "Images",
          colorToken: "cat-images",
          fileCount: 3,
          totalBytes: 1_800_000,
        },
      ],
      topRules: state.rules.map((r) => ({
        ruleId: r.id,
        ruleName: r.name,
        enabled: r.enabled,
        matchedOps: r.statMatched,
        bytesOrganized: 409_600,
        failedOps: r.statFailed,
        lastRunAtMs: r.lastRunAt,
      })),
      dailyActivity: [
        { date: "2025-01-01", count: state.operations.length },
      ],
      recentOperations: state.operations.slice(0, 5),
    });

    const validateRuleDraft = (draft: RuleDraft_Deserialize): ValidationIssue[] => {
      const issues: ValidationIssue[] = [];
      if (!draft.name.trim()) {
        issues.push({
          field: "name",
          code: "required",
          message: "Rule name cannot be empty.",
          severity: "error",
        });
      }
      if (draft.actions.length === 0) {
        issues.push({
          field: "actions",
          code: "required",
          message: "Add at least one action.",
          severity: "error",
        });
      }
      if (!draft.enabled) {
        issues.push({
          field: "enabled",
          code: "disabled_rule",
          message: "Rule is currently disabled and will only run in preview.",
          severity: "warning",
        });
      }
      return issues;
    };

    const dupGroupsData = (): DuplicateGroupsResponse_Serialize => ({
      totals: {
        totalGroups: 1,
        activeGroups: 1,
        ignoredGroups: 0,
        totalDuplicateCopies: 1,
        totalWastedBytes: 512_000,
      },
      groups: [
        {
          id: "dup-g1",
          contentHash: "blake3-sample-hash-001",
          fileSize: 512_000,
          ignored: false,
          copyCount: 2,
          wastedBytes: 512_000,
          allSameName: false,
          sameContentDifferentNames: true,
          createdAt: tick(),
          members: [
            {
              fileRecordId: "fr-1",
              folderId: "f1",
              path: "/Users/demo/Downloads/CopyA.pdf",
              filename: "CopyA.pdf",
              extension: "pdf",
              size: 512_000,
              createdMs: tick(),
              modifiedMs: tick(),
              isKeeper: true,
              exists: true,
            },
            {
              fileRecordId: "fr-2",
              folderId: "f1",
              path: "/Users/demo/Downloads/CopyB.pdf",
              filename: "CopyB.pdf",
              extension: "pdf",
              size: 512_000,
              createdMs: tick(),
              modifiedMs: tick(),
              isKeeper: false,
              exists: true,
            },
          ],
        },
      ],
    });

    const appInfo: AppInfo = {
      version: "0.1.0",
      dataDir: "/Users/demo/Library/Application Support/com.folderflow.desktop",
      logDir: "/Users/demo/Library/Logs/com.folderflow.desktop",
      databasePath: "/Users/demo/Library/Application Support/com.folderflow.desktop/folderflow.db",
      schemaVersion: 1,
    };

    const monitoringStatus = (): MonitoringStatus_Serialize => ({
      paused: state.runtimePaused,
      folders: state.folders.map((f) => ({
        folderId: f.id,
        state: !f.enabled || state.runtimePaused ? "paused" : "watching",
        pendingEvents: 0,
        trackedFiles: f.fileCount,
        lastError: f.lastError,
        lastEventAt: f.lastScannedAt,
      })),
    });

    const handleCommand = async (cmd: string, args: Record<string, unknown> = {}): Promise<unknown> => {
      switch (cmd) {
        case "plugin:event|listen": {
          const eventName = String(args.event ?? "");
          const handlerId = Number(args.handler ?? 0);
          let set = listeners.get(eventName);
          if (!set) {
            set = new Set();
            listeners.set(eventName, set);
          }
          set.add(handlerId);
          return handlerId;
        }
        case "plugin:event|unlisten": {
          const eventName = String(args.event ?? "");
          const eventId = Number(args.eventId ?? 0);
          listeners.get(eventName)?.delete(eventId);
          return null;
        }
        case "plugin:event|emit": {
          emitLocal(String(args.event ?? ""), args.payload);
          return null;
        }
        case "app_info":
          return appInfo;
        case "settings_get":
          return state.settings;
        case "settings_update": {
          const next = args.settings as Settings_Serialize;
          state.settings = structuredClone(next);
          return state.settings;
        }
        case "login_item_status":
          return "not_registered";
        case "open_login_items_settings":
        case "open_privacy_settings":
          return true;
        case "license_status":
          return state.license;
        case "license_activate":
        case "license_activate_offline": {
          const rawKey = String(args.key ?? args.signedKeyOrJson ?? "").trim();
          if (!rawKey || rawKey.startsWith("INVALID")) {
            throw {
              code: "validation",
              message: "Invalid offline license signature.",
              hint: "Check your FF1 key string and retry.",
              field: "key",
            };
          }
          state.license = {
            state: "active_offline",
            licensed: true,
            edition: "lifetime",
            source: "offline_key",
            licenseId: "FF-E2E-LIFETIME-001",
            maskedKey: `${rawKey.slice(0, 6)}••••${rawKey.slice(-4)}`,
            customerHint: "demo@folderflow.app",
            providerName: null,
            onlineProviderConfigured: false,
            deviceLimit: 2,
            activatedAtMs: tick(),
            expiresAtMs: null,
            lastError: null,
          };
          return state.license;
        }
        case "license_deactivate": {
          state.license = {
            ...state.license,
            state: "unlicensed",
            licensed: false,
            edition: null,
            source: null,
            licenseId: null,
            maskedKey: null,
            customerHint: null,
            activatedAtMs: null,
          };
          return state.license;
        }
        case "onboarding_state":
          return state.onboarding;
        case "onboarding_set_step": {
          const upd = args.update as {
            currentStep: number;
            organizationPreference?: OnboardingState_Serialize["organizationPreference"] | null;
            selectedPresetIds?: string[] | null;
          };
          state.onboarding = {
            ...state.onboarding,
            currentStep: upd.currentStep,
            organizationPreference:
              upd.organizationPreference ?? state.onboarding.organizationPreference,
            selectedPresetIds: upd.selectedPresetIds ?? state.onboarding.selectedPresetIds,
          };
          return state.onboarding;
        }
        case "onboarding_scan_preview": {
          const report: OnboardingScanReport = {
            foldersScanned: Math.max(1, state.folders.length),
            totalFiles: 12,
            totalFolders: 2,
            totalBytes: 3_300_000,
            byCategory: [
              { category: "documents", count: 7, bytes: 1_900_000 },
              { category: "images", count: 5, bytes: 1_400_000 },
            ],
            planSummary: {
              planId: `onb-plan-${state.planCounter++}`,
              schemaVersion: 1,
              createdAt: tick(),
              filesEvaluated: 12,
              filesMatched: state.lastPlanItems.length,
              filesWithConflicts: 0,
              totalBytesAffected: 757_760,
              crossVolumeBytes: 0,
              byCategory: [
                { category: "documents", count: 1, bytes: 245_760 },
                { category: "images", count: 1, bytes: 512_000 },
              ],
              byRule: [{ ruleId: "r1", ruleName: "Sort Invoices", count: 2 }],
            },
            sampleItems: state.lastPlanItems,
          };
          return report;
        }
        case "onboarding_complete": {
          const presetIds = (args.selectedPresetIds as string[] | undefined) ?? [];
          const enable = Boolean(args.confirmEnableAutomation);
          for (const pid of presetIds) {
            const tpl = state.presets.find((p) => p.id === pid);
            if (tpl && !state.rules.some((r) => r.name === tpl.name)) {
              state.rules.push({
                id: `r-onb-${state.rules.length + 1}`,
                name: tpl.name,
                description: tpl.description,
                enabled: enable,
                priority: state.rules.length + 1,
                sourceFolders: tpl.draft.sourceFolders ?? [],
                conditions: tpl.draft.conditions,
                actions: tpl.draft.actions,
                dateSource: tpl.draft.dateSource ?? "modified",
                executionMode: tpl.draft.executionMode ?? "first_match",
                origin: "preset",
                lastRunAt: null,
                statMatched: 0,
                statSucceeded: 0,
                statFailed: 0,
                createdAt: tick(),
                updatedAt: tick(),
              });
            }
          }
          state.onboarding = {
            ...state.onboarding,
            completed: true,
            automationConfirmed: enable,
            selectedPresetIds: presetIds,
            completedAt: tick(),
          };
          const out: OnboardingCompleteReport_Serialize = {
            state: state.onboarding,
            rulesCreated: presetIds.length,
            automationEnabled: enable,
          };
          return out;
        }
        case "onboarding_reset":
          state.onboarding = { ...state.onboarding, completed: false, currentStep: 1 };
          return state.onboarding;
        case "folders_list":
          return state.folders;
        case "folders_add_known": {
          const kind = String(args.kind ?? "downloads");
          const cap = kind.charAt(0).toUpperCase() + kind.slice(1);
          const id = `f-${state.folders.length + 1}`;
          const created: MonitoredFolder_Serialize = {
            id,
            name: cap,
            path: `/Users/demo/${cap}`,
            enabled: true,
            recursive: Boolean(args.recursive),
            settings: {
              includeExtensions: [],
              excludeExtensions: [],
              excludedSubfolders: [],
              stabilityIntervalSecs: null,
            },
            health: "healthy",
            lastError: null,
            fileCount: 6,
            totalBytes: 1_200_000,
            lastScannedAt: tick(),
            createdAt: tick(),
          };
          state.folders.push(created);
          return created;
        }
        case "folders_update": {
          const id = String(args.id ?? "");
          const patch = args.patch as {
            name?: string | null;
            enabled?: boolean | null;
            recursive?: boolean | null;
            settings?: MonitoredFolder_Serialize["settings"] | null;
          };
          state.folders = state.folders.map((f) =>
            f.id === id
              ? {
                  ...f,
                  name: patch.name ?? f.name,
                  enabled: patch.enabled ?? f.enabled,
                  recursive: patch.recursive ?? f.recursive,
                  settings: patch.settings ?? f.settings,
                }
              : f,
          );
          return state.folders.find((f) => f.id === id);
        }
        case "folders_action": {
          const id = String(args.id ?? "");
          const act = String(args.action ?? "");
          state.folders = state.folders.map((f) => {
            if (f.id !== id) return f;
            if (act === "pause") return { ...f, health: "paused" };
            if (act === "resume" || act === "rescan") return { ...f, health: "healthy", lastScannedAt: tick() };
            return f;
          });
          emitLocal("folder-status-event", {
            folderId: id,
            state: act === "pause" ? "paused" : "watching",
            pendingEvents: 0,
            trackedFiles: 4,
            lastError: null,
            lastEventAt: tick(),
          });
          return null;
        }
        case "folders_remove":
          state.folders = state.folders.filter((f) => f.id !== String(args.id ?? ""));
          return null;
        case "folders_summary": {
          const sum: FolderSummary = {
            files: 4,
            bytes: 2_450_000,
            byCategory: [
              { category: "documents", files: 2, bytes: 1_200_000 },
              { category: "images", files: 2, bytes: 1_250_000 },
            ],
          };
          return sum;
        }
        case "folders_errors":
          return [];
        case "folders_access":
          return "granted" satisfies FolderAccess;
        case "monitoring_status":
          return monitoringStatus();
        case "monitoring_set_paused":
          state.runtimePaused = Boolean(args.paused);
          return monitoringStatus();
        case "categories_list":
          return state.categories;
        case "categories_set_enabled": {
          const id = String(args.id ?? "");
          const enabled = Boolean(args.enabled);
          state.categories = state.categories.map((c) => (c.id === id ? { ...c, enabled } : c));
          return state.categories.find((c) => c.id === id);
        }
        case "rules_list":
          return state.rules;
        case "rules_get": {
          const id = String(args.id ?? "");
          const found = state.rules.find((r) => r.id === id);
          if (!found) {
            throw { code: "not_found", message: `Rule ${id} not found`, hint: null, field: null };
          }
          return found;
        }
        case "rules_create": {
          const draft = args.draft as RuleDraft_Deserialize;
          const issues = validateRuleDraft(draft);
          const firstError = issues.find((i) => i.severity === "error");
          if (firstError) {
            throw {
              code: "validation",
              message: firstError.message,
              hint: null,
              field: firstError.field,
            };
          }
          const created: Rule_Serialize = {
            id: `r-${state.rules.length + 1}`,
            name: draft.name.trim(),
            description: draft.description ?? null,
            enabled: draft.enabled,
            priority: state.rules.length + 1,
            sourceFolders: draft.sourceFolders ?? [],
            conditions: draft.conditions as Rule_Serialize["conditions"],
            actions: draft.actions as Rule_Serialize["actions"],
            dateSource: draft.dateSource ?? "modified",
            executionMode: draft.executionMode ?? "first_match",
            origin: "user",
            lastRunAt: null,
            statMatched: 0,
            statSucceeded: 0,
            statFailed: 0,
            createdAt: tick(),
            updatedAt: tick(),
          };
          state.rules.push(created);
          return created;
        }
        case "rules_update": {
          const id = String(args.id ?? "");
          const draft = args.draft as RuleDraft_Deserialize;
          state.rules = state.rules.map((r) =>
            r.id === id
              ? {
                  ...r,
                  name: draft.name.trim(),
                  description: draft.description ?? null,
                  enabled: draft.enabled,
                  sourceFolders: draft.sourceFolders ?? [],
                  conditions: draft.conditions as Rule_Serialize["conditions"],
                  actions: draft.actions as Rule_Serialize["actions"],
                  dateSource: draft.dateSource ?? r.dateSource,
                  executionMode: draft.executionMode ?? r.executionMode,
                  updatedAt: tick(),
                }
              : r,
          );
          return state.rules.find((r) => r.id === id);
        }
        case "rules_delete":
          state.rules = state.rules.filter((r) => r.id !== String(args.id ?? ""));
          return null;
        case "rules_set_enabled": {
          const id = String(args.id ?? "");
          const enabled = Boolean(args.enabled);
          state.rules = state.rules.map((r) => (r.id === id ? { ...r, enabled } : r));
          return state.rules.find((r) => r.id === id);
        }
        case "rules_validate":
          return validateRuleDraft(args.draft as RuleDraft_Deserialize);
        case "rules_presets":
          return state.presets;
        case "rules_smart_catalog": {
          const cat: SmartOrganizerCatalog = {
            defaultScreenshotPatterns: ["Screenshot", "Screen Shot", "CleanShot"],
            defaultInvoiceKeywords: ["Invoice", "Receipt", "Bill", "Payment", "Tax", "GST"],
            allowedOldFileDays: [7, 15, 30, 60, 90, 180, 365],
            downloadCategories: [
              { categoryId: "documents", categoryName: "Documents", defaultSubpath: "Documents" },
            ],
          };
          return cat;
        }
        case "rules_test_folder": {
          const report: RuleTestReport = {
            folderId: String(args.folderId ?? "f1"),
            folderName: "Downloads",
            evaluatedFiles: 4,
            matchedFiles: 1,
            sampleMatches: [
              {
                path: "/Users/demo/Downloads/Invoice_Jan.pdf",
                originalName: "Invoice_Jan.pdf",
                finalFilename: "Invoice_Jan.pdf",
                finalDirectory: "/Users/demo/Documents/Invoices/2025/January",
                actions: [
                  {
                    ruleId: "preview",
                    ruleName: "Preview",
                    op: { kind: "move", destination_dir: "/Users/demo/Documents/Invoices/2025/January" },
                  },
                ],
              },
            ],
          };
          return report;
        }
        case "template_preview":
          return {
            rendered: "Documents/Invoices/2025/January/Invoice_Jan.pdf",
            sanitizedVariables: [],
            valid: true,
          };
        case "plan_create": {
          const summary: PlanSummary = {
            planId: `plan-${state.planCounter++}`,
            schemaVersion: 1,
            createdAt: tick(),
            filesEvaluated: 6,
            filesMatched: state.lastPlanItems.length,
            filesWithConflicts: 0,
            totalBytesAffected: state.lastPlanItems.reduce((s, i) => s + (i.size ?? 0), 0),
            crossVolumeBytes: 0,
            byCategory: [
              { category: "documents", count: 1, bytes: 245_760 },
              { category: "images", count: 1, bytes: 512_000 },
            ],
            byRule: [{ ruleId: "r1", ruleName: "Sort Invoices", count: state.lastPlanItems.length }],
          };
          return summary;
        }
        case "plan_items": {
          const page: PlanItemsPage = {
            items: state.lastPlanItems,
            total: state.lastPlanItems.length,
            limit: Number(args.limit ?? 100),
            offset: Number(args.offset ?? 0),
          };
          return page;
        }
        case "plan_execute": {
          const selected = (args.selectedItemIds as string[] | null) ?? state.lastPlanItems.map((i) => i.id);
          const executedItems = state.lastPlanItems.filter((i) => selected.includes(i.id));
          const completedOps: OperationView_Serialize[] = executedItems.map((it, idx) => ({
            id: `op-exec-${state.operations.length + idx + 1}`,
            batchId: "batch-e2e",
            operationType: "move",
            sourcePath: it.sourcePath,
            destinationPath: it.proposedDestination,
            originalName: it.originalName,
            newName: it.proposedName,
            size: it.size,
            contentHash: "blake3-exec",
            crossVolume: it.crossVolume,
            ruleId: it.ruleId,
            ruleName: it.ruleName,
            status: "COMPLETED",
            reversible: true,
            reversedBy: null,
            errorCode: null,
            errorMessage: null,
            startedAt: tick(),
            completedAt: tick(),
          }));
          state.operations = [...completedOps, ...state.operations];
          state.lastPlanItems = state.lastPlanItems.filter((i) => !selected.includes(i.id));
          const report: PlanExecutionReport_Serialize = {
            planId: String(args.planId ?? "plan-1"),
            batchId: "batch-e2e",
            completed: completedOps,
            queuedForReview: 0,
            skippedStale: [],
            failed: [],
          };
          return report;
        }
        case "plan_discard":
          state.lastPlanItems = [];
          return true;
        case "activity_list": {
          const page: ActivityPage_Serialize = {
            items: state.operations,
            total: state.operations.length,
            limit: 25,
            offset: 0,
          };
          return page;
        }
        case "activity_undo": {
          const target = args.target as { kind: string; op_ids?: string[] };
          const candidate =
            target.kind === "last"
              ? state.operations.find((o) => o.status === "COMPLETED" && o.reversible && !o.reversedBy)
              : state.operations.find((o) => target.op_ids?.includes(o.id));
          if (!candidate) {
            return { undone: [], conflicts: [], failed: [], prunedDirs: 0 } satisfies UndoReport_Serialize;
          }
          const undoOp: OperationView_Serialize = {
            id: `op-undo-${state.operations.length + 1}`,
            batchId: candidate.batchId,
            operationType: "undo_move",
            sourcePath: candidate.destinationPath,
            destinationPath: candidate.sourcePath,
            originalName: candidate.newName,
            newName: candidate.originalName,
            size: candidate.size,
            contentHash: candidate.contentHash,
            crossVolume: candidate.crossVolume,
            ruleId: candidate.ruleId,
            ruleName: candidate.ruleName,
            status: "COMPLETED",
            reversible: false,
            reversedBy: null,
            errorCode: null,
            errorMessage: null,
            startedAt: tick(),
            completedAt: tick(),
          };
          state.operations = [
            undoOp,
            ...state.operations.map((o) =>
              o.id === candidate.id ? { ...o, status: "ROLLED_BACK" as const, reversedBy: undoOp.id } : o,
            ),
          ];
          return {
            undone: [undoOp],
            conflicts: [],
            failed: [],
            prunedDirs: 0,
          } satisfies UndoReport_Serialize;
        }
        case "activity_export_csv":
          return "id,operation_type,source_path,destination_path,status\nop-1,move,/Users/demo/Downloads/Quarterly_Report.pdf,/Users/demo/Documents/Quarterly_Report.pdf,COMPLETED\n";
        case "activity_clear": {
          const count = state.operations.length;
          state.operations = [];
          return count;
        }
        case "review_list":
          return state.reviews;
        case "review_resolve": {
          const id = String(args.id ?? "");
          state.reviews = state.reviews.filter((r) => r.id !== id);
          return { reviewId: id, decision: args.decision, operation: null };
        }
        case "dashboard_stats":
          return buildStats();
        case "dup_groups":
          return dupGroupsData();
        case "schedules_list":
          return state.schedules;
        case "exclusions_list":
          return [];
        case "notifications_list":
          return [];
        case "db_maintenance":
          return {
            freelistPagesBefore: 2,
            freelistPagesAfter: 0,
            sizeBytesBefore: 65_536,
            sizeBytesAfter: 49_152,
            completedAtMs: tick(),
          };
        default:
          return null;
      }
    };

    const g = window as unknown as {
      __TAURI_INTERNALS__?: Record<string, unknown>;
      __TAURI_EVENT_PLUGIN_INTERNALS__?: Record<string, unknown>;
    };

    g.__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args?: Record<string, unknown>) => handleCommand(cmd, args),
      transformCallback: (cb: (payload: unknown) => void, once = false) => {
        const id = callbackSeq++;
        callbacks.set(id, (payload: unknown) => {
          if (once) callbacks.delete(id);
          cb(payload);
        });
        return id;
      },
      unregisterCallback: (id: number) => {
        callbacks.delete(id);
      },
      runCallback: (id: number, payload: unknown) => {
        callbacks.get(id)?.(payload);
      },
      callbacks: Object.fromEntries(callbacks),
      convertFileSrc: (filePath: string) => `asset://localhost/${encodeURIComponent(filePath)}`,
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { windowLabel: "main", label: "main" },
      },
    };

    g.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (event: string, eventId: number) => {
        listeners.get(event)?.delete(eventId);
      },
    };
  }, seed);
}
