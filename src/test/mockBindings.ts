/**
 * Shared mock of the generated bindings. Use in tests with:
 *   vi.mock("@/bindings", () => import("@/test/mockBindings"));
 * and call `resetBindingMocks()` in beforeEach.
 */
import { vi } from "vitest";
import type { FolderActivityEvent, FolderRuntime_Serialize, MonitoredFolder_Serialize } from "@/bindings";
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
}
