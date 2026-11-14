/**
 * Shared mock of the generated bindings. Use in tests with:
 *   vi.mock("@/bindings", () => import("@/test/mockBindings"));
 * and call `resetBindingMocks()` in beforeEach.
 */
import { vi } from "vitest";
import type {
  FolderActivityEvent,
  FolderRuntime_Serialize,
  MonitoredFolder_Serialize,
  RuleTemplate_Serialize,
  Rule_Serialize,
} from "@/bindings";
import { appInfoFixture, defaultSettings } from "./fixtures";

type Listener<T> = (e: { payload: T }) => void;
const statusListeners = new Set<Listener<FolderRuntime_Serialize>>();
const activityListeners = new Set<Listener<FolderActivityEvent>>();

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

export const commands = {
  appInfo: vi.fn(),
  settingsGet: vi.fn(),
  settingsUpdate: vi.fn(),
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
  rulesList: vi.fn(),
  rulesGet: vi.fn(),
  rulesCreate: vi.fn(),
  rulesCreateFromPreset: vi.fn(),
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
};

export const events = {
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
};

export function emitFolderStatus(payload: FolderRuntime_Serialize): void {
  statusListeners.forEach((l) => l({ payload }));
}

export function emitFolderActivity(payload: FolderActivityEvent): void {
  activityListeners.forEach((l) => l({ payload }));
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

/** Restores default successful responses for every command. */
export function resetBindingMocks(): void {
  statusListeners.clear();
  activityListeners.clear();
  Object.values(commands).forEach((m) => m.mockReset());
  commands.appInfo.mockResolvedValue(ok(appInfoFixture));
  commands.settingsGet.mockResolvedValue(ok(defaultSettings()));
  commands.settingsUpdate.mockImplementation(async (s: unknown) => ok(s));
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
  commands.rulesList.mockResolvedValue(ok([]));
  commands.rulesPresets.mockResolvedValue(ok([presetFixture()]));
  commands.rulesValidate.mockResolvedValue(ok([]));
  commands.rulesDelete.mockResolvedValue(ok(null));
  commands.rulesPickDestinationFolder.mockResolvedValue(ok(null));
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
}
