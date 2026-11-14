import { create } from "zustand";
import {
  activityService,
  type ActivityFilter,
  type OperationView,
  type PageRequest,
  type RevealTarget,
  type ReviewDecision,
  type ReviewItemView,
  type UndoConflict,
  type UndoReport,
  type UndoTarget,
} from "@/services/activity";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

interface ActivityState {
  items: OperationView[];
  total: number;
  page: PageRequest;
  filter: ActivityFilter;
  reviews: ReviewItemView[];
  undoConflicts: UndoConflict[];
  lastUndoSummary: string | null;
  status: Status;
  loadError: UiError | null;
  actionError: UiError | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  setFilter: (patch: Partial<ActivityFilter>) => Promise<void>;
  resetFilter: () => Promise<void>;
  setPage: (offset: number) => Promise<void>;
  undo: (target: UndoTarget) => Promise<UndoReport | null>;
  retry: (opId: string) => Promise<OperationView | null>;
  exportCsv: () => Promise<string | null>;
  clearHistory: (beforeMs?: number | null) => Promise<number | null>;
  revealInFinder: (opId: string, which: RevealTarget) => Promise<boolean>;
  resolveReview: (id: string, decision: ReviewDecision) => Promise<boolean>;
  dismissUndoConflicts: () => void;
  clearActionError: () => void;
  reset: () => void;
}

const DEFAULT_PAGE: PageRequest = { limit: 50, offset: 0 };

const initial = {
  items: [] as OperationView[],
  total: 0,
  page: { ...DEFAULT_PAGE },
  filter: {} as ActivityFilter,
  reviews: [] as ReviewItemView[],
  undoConflicts: [] as UndoConflict[],
  lastUndoSummary: null as string | null,
  status: "idle" as Status,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

export const useActivity = create<ActivityState>((set, get) => {
  const fetchCurrent = async () => {
    const { filter, page } = get();
    const [res, reviews] = await Promise.all([
      activityService.list(filter, page),
      activityService.reviewList(),
    ]);
    set({
      items: res.items,
      total: res.total,
      page: { limit: res.limit, offset: res.offset },
      reviews,
    });
  };

  return {
    ...initial,

    load: async () => {
      set({ status: "loading", loadError: null });
      try {
        await fetchCurrent();
        set({ status: "ready" });
      } catch (e) {
        set({ status: "error", loadError: toUiError(e) });
      }
    },

    refresh: async () => {
      try {
        await fetchCurrent();
      } catch {
        // Keep last known good state on background refresh failure.
      }
    },

    setFilter: async (patch) => {
      const nextFilter: ActivityFilter = { ...get().filter, ...patch };
      set({ filter: nextFilter, page: { ...get().page, offset: 0 }, actionError: null });
      try {
        await fetchCurrent();
      } catch (e) {
        set({ actionError: toUiError(e) });
      }
    },

    resetFilter: async () => {
      set({ filter: {}, page: { ...DEFAULT_PAGE }, actionError: null });
      try {
        await fetchCurrent();
      } catch (e) {
        set({ actionError: toUiError(e) });
      }
    },

    setPage: async (offset) => {
      set({ page: { ...get().page, offset: Math.max(0, offset) }, actionError: null });
      try {
        await fetchCurrent();
      } catch (e) {
        set({ actionError: toUiError(e) });
      }
    },

    undo: async (target) => {
      set({ actionError: null, lastUndoSummary: null });
      try {
        const rep = await activityService.undo(target);
        const summaryParts: string[] = [];
        if (rep.undone.length > 0) {
          summaryParts.push(`Undid ${rep.undone.length} operation${rep.undone.length === 1 ? "" : "s"}.`);
        }
        if (rep.prunedDirs > 0) {
          summaryParts.push(`Removed ${rep.prunedDirs} empty folder${rep.prunedDirs === 1 ? "" : "s"}.`);
        }
        if (rep.failed.length > 0) {
          summaryParts.push(rep.failed.join(" "));
        }
        set({
          undoConflicts: rep.conflicts,
          lastUndoSummary: summaryParts.length > 0 ? summaryParts.join(" ") : null,
        });
        await fetchCurrent();
        return rep;
      } catch (e) {
        set({ actionError: toUiError(e) });
        void get().refresh();
        return null;
      }
    },

    retry: async (opId) => {
      set({ actionError: null });
      try {
        const updated = await activityService.retry(opId);
        await fetchCurrent();
        return updated;
      } catch (e) {
        set({ actionError: toUiError(e) });
        void get().refresh();
        return null;
      }
    },

    exportCsv: async () => {
      set({ actionError: null });
      try {
        return await activityService.exportCsv(get().filter);
      } catch (e) {
        set({ actionError: toUiError(e) });
        return null;
      }
    },

    clearHistory: async (beforeMs = null) => {
      set({ actionError: null });
      try {
        const removed = await activityService.clear(beforeMs);
        await fetchCurrent();
        return removed;
      } catch (e) {
        set({ actionError: toUiError(e) });
        void get().refresh();
        return null;
      }
    },

    revealInFinder: async (opId, which) => {
      set({ actionError: null });
      try {
        return await activityService.revealInFinder(opId, which);
      } catch (e) {
        set({ actionError: toUiError(e) });
        return false;
      }
    },

    resolveReview: async (id, decision) => {
      set({ actionError: null });
      try {
        await activityService.reviewResolve(id, decision);
        await fetchCurrent();
        return true;
      } catch (e) {
        set({ actionError: toUiError(e) });
        void get().refresh();
        return false;
      }
    },

    dismissUndoConflicts: () => set({ undoConflicts: [] }),
    clearActionError: () => set({ actionError: null }),
    reset: () => set({ ...initial }),
  };
});

