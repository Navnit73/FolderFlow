/**
 * Shared mock of the generated bindings. Use in tests with:
 *   vi.mock("@/bindings", () => import("@/test/mockBindings"));
 * and call `resetBindingMocks()` in beforeEach.
 */
import { vi } from "vitest";
import type {
  AppNotificationEvent_Serialize,
  Category_Serialize,
  ConfigImportPreview,
  ConfigImportReport,
  DashboardStats_Serialize,
  DbMaintenanceReport,
  DuplicateGroupSummary_Serialize,
  DupProgressEvent,
  ExclusionEntry_Serialize,
  FolderActivityEvent,
  FolderRuntime_Serialize,
  LoginItemStatus,
  MonitoredFolder_Serialize,
  OnboardingState_Serialize,
  PlanItem,
  PlanSummary,
  RuleTemplate_Serialize,
  Rule_Serialize,
  Schedule_Serialize,
  SmartOrganizerCatalog,
  TrayNavigateEvent,
} from "@/bindings";
import { appInfoFixture, defaultLicenseStatus, defaultSettings } from "./fixtures";

type Listener<T> = (e: { payload: T }) => void;
const statusListeners = new Set<Listener<FolderRuntime_Serialize>>();
const activityListeners = new Set<Listener<FolderActivityEvent>>();
const dupProgressListeners = new Set<Listener<DupProgressEvent>>();
const notificationListeners = new Set<Listener<AppNotificationEvent_Serialize>>();
const trayNavigateListeners = new Set<Listener<TrayNavigateEvent>>();

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

export const commands = {
  appInfo: vi.fn(),
  settingsGet: vi.fn(),
  settingsUpdate: vi.fn(),
  loginItemStatus: vi.fn(),
  openLoginItemsSettings: vi.fn(),
  licenseStatus: vi.fn(),
  licenseActivate: vi.fn(),
  licenseActivateOffline: vi.fn(),
  licensePickAndActivateOfflineFile: vi.fn(),
  licenseDeactivate: vi.fn(),
  foldersList: vi.fn(),
  foldersAddWithPicker: vi.fn(),
  foldersAddKnown: vi.fn(),
  foldersUpdate: vi.fn(),
  foldersRemove: vi.fn(),
  foldersAction: vi.fn(),
  foldersPickSubfolder: vi.fn(),
  foldersSummary: vi.fn(),
  foldersErrors: vi.fn(),
  foldersAccess: vi.fn(),
  openPrivacySettings: vi.fn(),
  monitoringStatus: vi.fn(),
  monitoringSetPaused: vi.fn(),
  categoriesList: vi.fn(),
  categoriesCreate: vi.fn(),
  categoriesUpdate: vi.fn(),
  categoriesDelete: vi.fn(),
  categoriesSetEnabled: vi.fn(),
  categoriesReorder: vi.fn(),
  categoriesPickDestinationFolder: vi.fn(),
  rulesList: vi.fn(),
  rulesGet: vi.fn(),
  rulesCreate: vi.fn(),
  rulesCreateFromPreset: vi.fn(),
  rulesSmartCatalog: vi.fn(),
  rulesCreateSmartPreset: vi.fn(),
  rulesUpdate: vi.fn(),
  rulesDelete: vi.fn(),
  rulesReorder: vi.fn(),
  rulesSetEnabled: vi.fn(),
  rulesDuplicate: vi.fn(),
  rulesValidate: vi.fn(),
  rulesPresets: vi.fn(),
  templatePreview: vi.fn(),
  rulesTestFolder: vi.fn(),
  rulesPickDestinationFolder: vi.fn(),
  activityList: vi.fn(),
  activityUndo: vi.fn(),
  activityRetry: vi.fn(),
  activityExportCsv: vi.fn(),
  activityClear: vi.fn(),
  revealInFinder: vi.fn(),
  reviewList: vi.fn(),
  reviewResolve: vi.fn(),
  planCreate: vi.fn(),
  planItems: vi.fn(),
  planExecute: vi.fn(),
  planDiscard: vi.fn(),
  dupScan: vi.fn(),
  dupGroups: vi.fn(),
  dupGroupDetails: vi.fn(),
  dupAction: vi.fn(),
  dupExport: vi.fn(),
  dupRevealFile: vi.fn(),
  dupPickReviewFolder: vi.fn(),
  dashboardStats: vi.fn(),
  dbMaintenance: vi.fn(),
  exclusionsList: vi.fn(),
  exclusionsCreate: vi.fn(),
  exclusionsDelete: vi.fn(),
  configExport: vi.fn(),
  configPickImportFile: vi.fn(),
  configImportPreview: vi.fn(),
  configImportApply: vi.fn(),
  notificationsList: vi.fn(),
  notificationsMarkRead: vi.fn(),
  notificationsClear: vi.fn(),
  notificationsTest: vi.fn(),
  schedulesList: vi.fn(),
  schedulesUpsert: vi.fn(),
  schedulesDelete: vi.fn(),
  schedulesSetEnabled: vi.fn(),
  schedulesRunNow: vi.fn(),
  onboardingState: vi.fn(),
  onboardingSetStep: vi.fn(),
  onboardingScanPreview: vi.fn(),
  onboardingComplete: vi.fn(),
  onboardingReset: vi.fn(),
};

export const events = {
  appNotificationEvent: {
    listen: vi.fn(async (cb: Listener<AppNotificationEvent_Serialize>) => {
      notificationListeners.add(cb);
      return () => notificationListeners.delete(cb);
    }),
  },
  dupProgressEvent: {
    listen: vi.fn(async (cb: Listener<DupProgressEvent>) => {
      dupProgressListeners.add(cb);
      return () => dupProgressListeners.delete(cb);
    }),
  },
  folderStatusEvent: {
    listen: vi.fn(async (cb: Listener<FolderRuntime_Serialize>) => {
      statusListeners.add(cb);
      return () => statusListeners.delete(cb);
    }),
  },
  folderActivityEvent: {
    listen: vi.fn(async (cb: Listener<FolderActivityEvent>) => {
      activityListeners.add(cb);
      return () => activityListeners.delete(cb);
    }),
  },
  trayNavigateEvent: {
    listen: vi.fn(async (cb: Listener<TrayNavigateEvent>) => {
      trayNavigateListeners.add(cb);
      return () => trayNavigateListeners.delete(cb);
    }),
  },
};

export function emitDupProgress(payload: DupProgressEvent): void {
  dupProgressListeners.forEach((l) => l({ payload }));
}

export function emitAppNotification(payload: AppNotificationEvent_Serialize): void {
  notificationListeners.forEach((l) => l({ payload }));
}

export function emitFolderStatus(payload: FolderRuntime_Serialize): void {
  statusListeners.forEach((l) => l({ payload }));
}

export function emitFolderActivity(payload: FolderActivityEvent): void {
  activityListeners.forEach((l) => l({ payload }));
}

export function emitTrayNavigate(payload: TrayNavigateEvent): void {
  trayNavigateListeners.forEach((l) => l({ payload }));
}

export function folderFixture(over: Partial<MonitoredFolder_Serialize> = {}): MonitoredFolder_Serialize {
  return {
    id: "f1",
    name: "Downloads",
    path: "/Users/test/Downloads",
    enabled: true,
    recursive: false,
    settings: { includeExtensions: [], excludeExtensions: [], excludedSubfolders: [], stabilityIntervalSecs: null },
    health: "healthy",
    lastError: null,
    fileCount: 42,
    totalBytes: 1_500_000,
    lastScannedAt: Date.now() - 60_000,
    createdAt: Date.now() - 86_400_000,
    ...over,
  };
}

export function runtimeFixture(over: Partial<FolderRuntime_Serialize> = {}): FolderRuntime_Serialize {
  return { folderId: "f1", state: "watching", pendingEvents: 0, trackedFiles: 0, lastError: null, lastEventAt: null, ...over };
}

export function ruleFixture(over: Partial<Rule_Serialize> = {}): Rule_Serialize {
  return {
    id: "r1",
    name: "Organize Invoices",
    description: "Move PDF invoices into dated folders",
    enabled: true,
    priority: 10,
    sourceFolders: [],
    conditions: {
      type: "all",
      children: [
        {
          type: "leaf",
          condition: {
            kind: "text",
            field: "extension",
            op: "equals",
            value: "pdf",
            case_sensitive: false,
          },
        },
      ],
    },
    actions: [
      {
        kind: "move_into_dynamic_folder",
        root: null,
        subpath_template: "Documents/Invoices/{year}/{month_name}",
      },
    ],
    dateSource: "modified",
    executionMode: "first_match",
    origin: "user",
    lastRunAt: null,
    statMatched: 12,
    statSucceeded: 12,
    statFailed: 0,
    createdAt: Date.now() - 86_400_000,
    updatedAt: Date.now() - 3_600_000,
    ...over,
  };
}

export function presetFixture(over: Partial<RuleTemplate_Serialize> = {}): RuleTemplate_Serialize {
  const base = ruleFixture();
  return {
    id: "pdfs",
    name: "Organize PDFs",
    description: "Moves PDF documents into Documents/PDFs.",
    categoryHint: "documents",
    draft: {
      name: base.name,
      description: base.description,
      enabled: false,
      sourceFolders: [],
      conditions: base.conditions,
      actions: base.actions,
      dateSource: "modified",
      executionMode: "first_match",
    },
    ...over,
  };
}

export function planSummaryFixture(over: Partial<PlanSummary> = {}): PlanSummary {
  return {
    planId: "plan-1",
    schemaVersion: 1,
    createdAt: Date.now(),
    filesEvaluated: 3,
    filesMatched: 2,
    filesWithConflicts: 0,
    totalBytesAffected: 2048,
    crossVolumeBytes: 0,
    byCategory: [{ category: "documents", count: 2, bytes: 2048 }],
    byRule: [{ ruleId: "r1", ruleName: "Organize Invoices", count: 2 }],
    ...over,
  };
}

export function planItemFixture(over: Partial<PlanItem> = {}): PlanItem {
  return {
    id: "item-1",
    folderId: "f1",
    sourcePath: "/Users/test/Downloads/invoice.pdf",
    currentLocation: "/Users/test/Downloads",
    originalName: "invoice.pdf",
    proposedDestination: "/Users/test/Documents/Invoices/invoice.pdf",
    proposedName: "invoice.pdf",
    size: 1024,
    category: "documents",
    op: "move",
    ruleId: "r1",
    ruleName: "Organize Invoices",
    matchedConditions: ["Extension is pdf"],
    plannedOps: [{ kind: "move", destination_dir: "/Users/test/Documents/Invoices" }],
    conflicts: [],
    status: "ready",
    crossVolume: false,
    identity: { dev: 1, ino: 101, size: 1024, mtimeNs: 1_000_000 },
    ...over,
  };
}

export function scheduleFixture(over: Partial<Schedule_Serialize> = {}): Schedule_Serialize {
  return {
    id: "s1",
    ruleId: null,
    folderId: "f1",
    scheduleType: "daily",
    config: {
      intervalSecs: null,
      time: "18:00",
      weekdays: [],
      quietHours: null,
      catchUp: "run_once",
    },
    enabled: true,
    lastRunAt: null,
    nextRunAt: Date.now() + 3_600_000,
    ...over,
  };
}

export function onboardingStateFixture(over: Partial<OnboardingState_Serialize> = {}): OnboardingState_Serialize {
  return {
    currentStep: 6,
    completed: true,
    organizationPreference: "by_category",
    selectedPresetIds: ["pdfs", "screenshots", "invoices"],
    automationConfirmed: true,
    completedAt: Date.now() - 60_000,
    ...over,
  };
}

export function dashboardStatsFixture(over: Partial<DashboardStats_Serialize> = {}): DashboardStats_Serialize {
  return {
    filesOrganizedTotal: 0,
    filesOrganizedToday: 0,
    filesOrganizedThisWeek: 0,
    totalBytesOrganized: 0,
    activeFoldersCount: 0,
    totalFoldersCount: 0,
    activeRulesCount: 0,
    totalRulesCount: 0,
    pendingReviewCount: 0,
    failedOperationsCount: 0,
    totalTrackedFiles: 0,
    totalTrackedBytes: 0,
    duplicateGroupsCount: 0,
    duplicateWastedBytes: 0,
    categoryBreakdown: [],
    topRules: [],
    dailyActivity: [],
    recentOperations: [],
    ...over,
  };
}

export function categoryFixture(over: Partial<Category_Serialize> = {}): Category_Serialize {
  return {
    id: "documents",
    name: "Documents",
    description: "PDFs and text documents",
    icon: "file-text",
    colorToken: "cat-documents",
    extensions: ["pdf", "doc", "docx", "txt"],
    destinationPath: null,
    priority: 10,
    builtin: true,
    enabled: true,
    ...over,
  };
}

export function duplicateGroupFixture(over: Partial<DuplicateGroupSummary_Serialize> = {}): DuplicateGroupSummary_Serialize {
  return {
    id: "grp-1",
    contentHash: "a".repeat(64),
    fileSize: 4096,
    ignored: false,
    copyCount: 2,
    wastedBytes: 4096,
    allSameName: false,
    sameContentDifferentNames: true,
    createdAt: Date.now() - 60_000,
    members: [
      {
        fileRecordId: "fr-1",
        folderId: "f1",
        path: "/Users/test/Downloads/report.pdf",
        filename: "report.pdf",
        extension: "pdf",
        size: 4096,
        createdMs: 1_000_000,
        modifiedMs: 1_000_000,
        isKeeper: true,
        exists: true,
      },
      {
        fileRecordId: "fr-2",
        folderId: "f1",
        path: "/Users/test/Downloads/report-copy.pdf",
        filename: "report-copy.pdf",
        extension: "pdf",
        size: 4096,
        createdMs: 1_000_000,
        modifiedMs: 1_000_000,
        isKeeper: false,
        exists: true,
      },
    ],
    ...over,
  };
}

/** Restores default successful responses for every command. */
export function resetBindingMocks(): void {
  statusListeners.clear();
  activityListeners.clear();
  dupProgressListeners.clear();
  notificationListeners.clear();
  trayNavigateListeners.clear();
  Object.values(commands).forEach((m) => m.mockReset());
  commands.appInfo.mockResolvedValue(ok(appInfoFixture));
  commands.settingsGet.mockResolvedValue(ok(defaultSettings()));
  commands.settingsUpdate.mockImplementation(async (s: unknown) => ok(s));
  commands.loginItemStatus.mockResolvedValue(ok("not_registered" satisfies LoginItemStatus));
  commands.openLoginItemsSettings.mockResolvedValue(ok(true));
  commands.licenseStatus.mockResolvedValue(ok(defaultLicenseStatus()));
  commands.licenseActivate.mockImplementation(async (key: string) => {
    if (key.trim().startsWith("FF1.")) {
      return ok(
        defaultLicenseStatus({
          state: "active_offline",
          licensed: true,
          edition: "lifetime",
          source: "offline_key",
          licenseId: "FF-LIFE-2026-0001",
          maskedKey: "FF1.••••-demo",
          activatedAtMs: Date.now(),
        }),
      );
    }
    return {
      status: "error" as const,
      error: {
        code: "validation" as const,
        message: "Online activation provider is not configured in this build yet.",
        hint: "Paste an offline signed key (FF1…) or import a .fflicense file.",
        field: "licenseKey",
      },
    };
  });
  commands.licenseActivateOffline.mockResolvedValue(
    ok(
      defaultLicenseStatus({
        state: "active_offline",
        licensed: true,
        edition: "lifetime",
        source: "offline_key",
        licenseId: "FF-LIFE-2026-0001",
        maskedKey: "FF1.••••-demo",
        activatedAtMs: Date.now(),
      }),
    ),
  );
  commands.licensePickAndActivateOfflineFile.mockResolvedValue(ok(null));
  commands.licenseDeactivate.mockResolvedValue(ok(defaultLicenseStatus()));
  commands.foldersList.mockResolvedValue(ok([]));
  commands.monitoringStatus.mockResolvedValue(ok({ paused: false, folders: [] }));
  commands.monitoringSetPaused.mockImplementation(async (paused: boolean) => ok({ paused, folders: [] }));
  commands.foldersAction.mockResolvedValue(ok(null));
  commands.foldersRemove.mockResolvedValue(ok(null));
  commands.foldersSummary.mockResolvedValue(ok({ files: 0, bytes: 0, byCategory: [] }));
  commands.foldersErrors.mockResolvedValue(ok([]));
  commands.foldersAccess.mockResolvedValue(ok("granted"));
  commands.openPrivacySettings.mockResolvedValue(ok(true));
  commands.categoriesList.mockResolvedValue(ok([]));
  commands.categoriesCreate.mockImplementation(async (input: Record<string, unknown>) =>
    ok(categoryFixture({ ...(input as Partial<Category_Serialize>), id: "custom-cat", builtin: false })),
  );
  commands.categoriesUpdate.mockImplementation(async (id: string, input: Record<string, unknown>) =>
    ok(categoryFixture({ ...(input as Partial<Category_Serialize>), id, builtin: false })),
  );
  commands.categoriesDelete.mockResolvedValue(ok(null));
  commands.categoriesSetEnabled.mockImplementation(async (id: string, enabled: boolean) =>
    ok(categoryFixture({ id, enabled })),
  );
  commands.categoriesReorder.mockResolvedValue(ok([]));
  commands.categoriesPickDestinationFolder.mockResolvedValue(ok(null));
  commands.rulesList.mockResolvedValue(ok([]));
  commands.rulesPresets.mockResolvedValue(ok([presetFixture()]));
  commands.rulesSmartCatalog.mockResolvedValue(
    ok({
      allowedOldFileDays: [7, 15, 30, 60, 90, 180, 365],
      defaultScreenshotPatterns: ["Screenshot*", "Screen Shot*"],
      defaultInvoiceKeywords: ["invoice", "receipt", "statement"],
      downloadCategories: [
        { categoryId: "documents", categoryName: "Documents", defaultSubpath: "Documents" },
      ],
    } satisfies SmartOrganizerCatalog),
  );
  commands.rulesCreateSmartPreset.mockImplementation(async () => ok(ruleFixture({ id: "smart-1", enabled: false, origin: "preset" })));
  commands.rulesValidate.mockResolvedValue(ok([]));
  commands.rulesDelete.mockResolvedValue(ok(null));
  commands.rulesPickDestinationFolder.mockResolvedValue(ok(null));
  commands.dashboardStats.mockResolvedValue(ok(dashboardStatsFixture()));
  commands.dbMaintenance.mockResolvedValue(
    ok({
      freelistPagesBefore: 4,
      freelistPagesAfter: 0,
      sizeBytesBefore: 65536,
      sizeBytesAfter: 49152,
      completedAtMs: Date.now(),
    } satisfies DbMaintenanceReport),
  );
  commands.dupGroups.mockResolvedValue(
    ok({
      totals: { totalGroups: 0, activeGroups: 0, ignoredGroups: 0, totalDuplicateCopies: 0, totalWastedBytes: 0 },
      groups: [],
    }),
  );
  commands.dupScan.mockResolvedValue(
    ok({
      filesScanned: 0,
      uniqueSizeSkipped: 0,
      partialHashed: 0,
      fullHashed: 0,
      cacheHits: 0,
      groupsFound: 0,
      totalWastedBytes: 0,
      groups: {
        totals: { totalGroups: 0, activeGroups: 0, ignoredGroups: 0, totalDuplicateCopies: 0, totalWastedBytes: 0 },
        groups: [],
      },
    }),
  );
  commands.dupExport.mockResolvedValue(ok("group_id,content_hash\n"));
  commands.dupRevealFile.mockResolvedValue(ok(true));
  commands.dupPickReviewFolder.mockResolvedValue(ok(null));
  commands.exclusionsList.mockResolvedValue(ok([]));
  commands.exclusionsCreate.mockImplementation(async (input: Record<string, unknown>) =>
    ok({ id: "excl-1", folderId: null, createdAt: Date.now(), ...(input as Partial<ExclusionEntry_Serialize>) } as ExclusionEntry_Serialize),
  );
  commands.exclusionsDelete.mockResolvedValue(ok(true));
  commands.configExport.mockResolvedValue(ok('{"schemaVersion":1}'));
  commands.configPickImportFile.mockResolvedValue(ok(null));
  commands.configImportPreview.mockResolvedValue(
    ok({
      schemaVersion: 1,
      exportedAt: Date.now(),
      settingsChanges: [],
      foldersToAdd: [],
      foldersToUpdate: [],
      foldersUnchanged: [],
      categoriesToAdd: [],
      categoriesToUpdate: [],
      exclusionsToAdd: [],
      exclusionsUnchanged: [],
      rulesToAdd: [],
      rulesToUpdate: [],
      forcedDisabledRuleCount: 0,
      warnings: [],
    } satisfies ConfigImportPreview),
  );
  commands.configImportApply.mockResolvedValue(
    ok({
      mode: "merge",
      foldersImported: 0,
      categoriesImported: 0,
      exclusionsImported: 0,
      rulesImportedDisabled: 0,
      warnings: [],
    } satisfies ConfigImportReport),
  );
  commands.notificationsList.mockResolvedValue(ok([]));
  commands.notificationsMarkRead.mockResolvedValue(ok(0));
  commands.notificationsClear.mockResolvedValue(ok(0));
  commands.notificationsTest.mockResolvedValue(ok(null));
  commands.templatePreview.mockImplementation(async (input: { template: string; isFilename?: boolean }) =>
    ok({
      valid: true,
      rendered: input.isFilename ? "2026-04-15_invoice.pdf" : "Documents/Invoices/2026/April",
      error: null,
    }),
  );
  commands.rulesTestFolder.mockResolvedValue(
    ok({
      folderId: "f1",
      folderName: "Downloads",
      evaluatedFiles: 10,
      matchedFiles: 2,
      sampleMatches: [],
    }),
  );
  commands.activityList.mockResolvedValue(ok({ items: [], total: 0, limit: 50, offset: 0 }));
  commands.activityUndo.mockResolvedValue(ok({ undone: [], conflicts: [], failed: [], prunedDirs: 0 }));
  commands.activityExportCsv.mockResolvedValue(ok("id,status\n"));
  commands.activityClear.mockResolvedValue(ok(0));
  commands.revealInFinder.mockResolvedValue(ok(true));
  commands.reviewList.mockResolvedValue(ok([]));
  commands.planCreate.mockResolvedValue(ok(planSummaryFixture()));
  commands.planItems.mockResolvedValue(ok({ items: [], total: 0, limit: 50, offset: 0 }));
  commands.planExecute.mockResolvedValue(
    ok({
      planId: "plan-1",
      batchId: "batch-1",
      completed: [],
      queuedForReview: 0,
      skippedStale: [],
      failed: [],
    }),
  );
  commands.planDiscard.mockResolvedValue(ok(true));
  commands.schedulesList.mockResolvedValue(ok([]));
  commands.schedulesUpsert.mockImplementation(async (input: Record<string, unknown>) =>
    ok(
      scheduleFixture({
        ...(input as Partial<Schedule_Serialize>),
        id: (input.id as string | null | undefined) ?? "s1",
      }),
    ),
  );
  commands.schedulesDelete.mockResolvedValue(ok(null));
  commands.schedulesSetEnabled.mockImplementation(async (id: string, enabled: boolean) =>
    ok(scheduleFixture({ id, enabled })),
  );
  commands.schedulesRunNow.mockResolvedValue(
    ok({ scheduleId: "s1", status: "no_matching_files", report: null }),
  );
  commands.onboardingState.mockResolvedValue(ok(onboardingStateFixture()));
  commands.onboardingSetStep.mockImplementation(async (update: Record<string, unknown>) =>
    ok(onboardingStateFixture({ ...(update as Partial<OnboardingState_Serialize>), completed: false })),
  );
  commands.onboardingScanPreview.mockResolvedValue(
    ok({
      foldersScanned: 1,
      totalFiles: 3,
      totalFolders: 1,
      totalBytes: 2048,
      byCategory: [{ category: "documents", count: 2, bytes: 2048 }],
      planSummary: planSummaryFixture(),
      sampleItems: [planItemFixture()],
    }),
  );
  commands.onboardingComplete.mockImplementation(async (selectedPresetIds: string[], confirmEnableAutomation: boolean) =>
    ok({
      state: onboardingStateFixture({
        completed: true,
        selectedPresetIds,
        automationConfirmed: confirmEnableAutomation,
      }),
      rulesCreated: selectedPresetIds.length,
      automationEnabled: confirmEnableAutomation,
    }),
  );
  commands.onboardingReset.mockResolvedValue(
    ok(onboardingStateFixture({ currentStep: 1, completed: false, automationConfirmed: false, completedAt: null })),
  );
}
