import { create } from "zustand";
import { events, type FolderRuntime_Serialize } from "@/bindings";
import { foldersService, type Category, type Folder, type FolderAction, type FolderPatch, type KnownFolder } from "@/services/folders";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";
import { useSettings } from "./settings";

export type FolderRuntime = FolderRuntime_Serialize;
type Status = "idle" | "loading" | "ready" | "error";
export type SaveResult = { ok: true; folder: Folder } | { ok: false; error: UiError };

interface FoldersState {
  folders: Folder[];
  runtime: Record<string, FolderRuntime>;
  categories: Category[];
  paused: boolean;
  status: Status;
  loadError: UiError | null;
  /** Error from the last user action (add, remove, pause…). */
  actionError: UiError | null;
  load: () => Promise<void>;
  /** Re-fetches the list without showing a loading state. */
  refresh: () => Promise<void>;
  addKnown: (kind: KnownFolder) => Promise<void>;
  addWithPicker: () => Promise<void>;
  update: (id: string, patch: FolderPatch) => Promise<SaveResult>;
  remove: (id: string) => Promise<boolean>;
  action: (id: string, action: FolderAction) => Promise<void>;
  setPaused: (paused: boolean) => Promise<void>;
  applyStatus: (rt: FolderRuntime) => void;
  clearActionError: () => void;
  /** Subscribes to backend events; returns an unsubscribe function. */
  subscribe: () => Promise<() => void>;
  reset: () => void;
}

const initial = {
  folders: [] as Folder[],
  runtime: {} as Record<string, FolderRuntime>,
  categories: [] as Category[],
  paused: false,
  status: "idle" as Status,
  loadError: null,
  actionError: null,
};

const recursiveDefault = () => useSettings.getState().settings?.monitoring.recursiveByDefault ?? false;

export const useFolders = create<FoldersState>((set, get) => {
  const upsert = (f: Folder) =>
    set((s) => ({
      folders: s.folders.some((x) => x.id === f.id) ? s.folders.map((x) => (x.id === f.id ? f : x)) : [...s.folders, f],
    }));

  const run = async (fn: () => Promise<void>) => {
    set({ actionError: null });
    try {
      await fn();
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  };

  let refreshTimer: ReturnType<typeof setTimeout> | undefined;

  return {
    ...initial,

    load: async () => {
      set({ status: "loading", loadError: null });
      try {
        const [folders, status, categories] = await Promise.all([
          foldersService.list(),
          foldersService.status(),
          foldersService.categories(),
        ]);
        set({
          folders,
          categories,
          paused: status.paused,
          runtime: Object.fromEntries(status.folders.map((r) => [r.folderId, r])),
          status: "ready",
        });
      } catch (e) {
        set({ status: "error", loadError: toUiError(e) });
      }
    },

    refresh: async () => {
      try {
        set({ folders: await foldersService.list() });
      } catch {
        // Background refresh: keep showing the last good data.
      }
    },

    addKnown: (kind) => run(async () => upsert(await foldersService.addKnown(kind, recursiveDefault()))),

    addWithPicker: () =>
      run(async () => {
        const f = await foldersService.addWithPicker(recursiveDefault());
        if (f) upsert(f);
      }),

    update: async (id, patch) => {
      try {
        const folder = await foldersService.update(id, patch);
        upsert(folder);
        return { ok: true, folder };
      } catch (e) {
        return { ok: false, error: toUiError(e) };
      }
    },

    remove: async (id) => {
      let removed = false;
      await run(async () => {
        await foldersService.remove(id);
        removed = true;
        set((s) => ({
          folders: s.folders.filter((f) => f.id !== id),
          runtime: Object.fromEntries(Object.entries(s.runtime).filter(([k]) => k !== id)),
        }));
      });
      return removed;
    },

    action: (id, action) => run(async () => void (await foldersService.action(id, action))),

    setPaused: (paused) =>
      run(async () => {
        const status = await foldersService.setPaused(paused);
        set({ paused: status.paused });
      }),

    applyStatus: (rt) => set((s) => ({ runtime: { ...s.runtime, [rt.folderId]: rt } })),

    clearActionError: () => set({ actionError: null }),

    subscribe: async () => {
      const offStatus = await events.folderStatusEvent.listen((e) => {
        get().applyStatus(e.payload as FolderRuntime);
        if (e.payload.state === "watching") void get().refresh();
      });
      const offActivity = await events.folderActivityEvent.listen(() => {
        clearTimeout(refreshTimer);
        refreshTimer = setTimeout(() => void get().refresh(), 750);
      });
      return () => {
        clearTimeout(refreshTimer);
        offStatus();
        offActivity();
      };
    },

    reset: () => set({ ...initial }),
  };
});
