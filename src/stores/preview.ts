import { create } from "zustand";
import {
  previewService,
  type ConflictPolicy,
  type PlanExecutionReport,
  type PlanItem,
  type PlanItemFilter,
  type PlanScope,
  type PlanSummary,
} from "@/services/preview";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

interface PreviewState {
  summary: PlanSummary | null;
  items: PlanItem[];
  total: number;
  limit: number;
  offset: number;
  scope: PlanScope;
  filter: PlanItemFilter;
  conflictPolicy: ConflictPolicy;
  selectedIds: string[];
  executing: boolean;
  lastReport: PlanExecutionReport | null;
  status: Status;
  loadError: UiError | null;
  actionError: UiError | null;
  generate: (scopePatch?: Partial<PlanScope>) => Promise<void>;
  refreshItems: (offset?: number) => Promise<void>;
  setFilter: (patch: Partial<PlanItemFilter>) => Promise<void>;
  resetFilter: () => Promise<void>;
  setPage: (offset: number) => Promise<void>;
  setConflictPolicy: (policy: ConflictPolicy) => void;
  toggleItem: (itemId: string) => void;
  selectAllSelectable: () => void;
  deselectAll: () => void;
  executeSelected: () => Promise<PlanExecutionReport | null>;
  discard: () => Promise<void>;
  clearActionError: () => void;
  reset: () => void;
}

const DEFAULT_LIMIT = 100;

const initial = {
  summary: null as PlanSummary | null,
  items: [] as PlanItem[],
  total: 0,
  limit: DEFAULT_LIMIT,
  offset: 0,
  scope: { folderId: null, ruleId: null, filePaths: null } as PlanScope,
  filter: { search: null, category: null, ruleId: null, onlyConflicts: false } as PlanItemFilter,
  conflictPolicy: "auto_rename" as ConflictPolicy,
  selectedIds: [] as string[],
  executing: false,
  lastReport: null as PlanExecutionReport | null,
  status: "idle" as Status,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

export const usePreview = create<PreviewState>((set, get) => ({
  ...initial,

  generate: async (scopePatch) => {
    const nextScope: PlanScope = { ...get().scope, ...scopePatch };
    set({
      scope: nextScope,
      status: "loading",
      loadError: null,
      actionError: null,
      lastReport: null,
      offset: 0,
    });
    try {
      const summary = await previewService.create(nextScope);
      const page = await previewService.items(
        summary.planId,
        get().filter,
        get().limit,
        0,
      );
      const selectedIds = page.items
        .filter((item) => item.status !== "blocked")
        .map((item) => item.id);
      set({
        summary,
        items: page.items,
        total: page.total,
        limit: page.limit,
        offset: page.offset,
        selectedIds,
        status: "ready",
      });
    } catch (e) {
      set({ status: "error", loadError: toUiError(e) });
    }
  },

  refreshItems: async (offset) => {
    const { summary, filter, limit } = get();
    if (!summary) return;
    const nextOffset = offset ?? get().offset;
    try {
      const page = await previewService.items(summary.planId, filter, limit, nextOffset);
      set({
        items: page.items,
        total: page.total,
        limit: page.limit,
        offset: page.offset,
      });
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  setFilter: async (patch) => {
    const nextFilter: PlanItemFilter = { ...get().filter, ...patch };
    set({ filter: nextFilter, offset: 0, actionError: null });
    await get().refreshItems(0);
  },

  resetFilter: async () => {
    set({
      filter: { search: null, category: null, ruleId: null, onlyConflicts: false },
      offset: 0,
      actionError: null,
    });
    await get().refreshItems(0);
  },

  setPage: async (offset) => {
    await get().refreshItems(Math.max(0, offset));
  },

  setConflictPolicy: (conflictPolicy) => set({ conflictPolicy }),

  toggleItem: (itemId) => {
    const item = get().items.find((i) => i.id === itemId);
    if (item?.status === "blocked") return;
    const current = new Set(get().selectedIds);
    if (current.has(itemId)) current.delete(itemId);
    else current.add(itemId);
    set({ selectedIds: Array.from(current) });
  },

  selectAllSelectable: () => {
    const selectable = get()
      .items.filter((i) => i.status !== "blocked")
      .map((i) => i.id);
    set({ selectedIds: selectable });
  },

  deselectAll: () => set({ selectedIds: [] }),

  executeSelected: async () => {
    const { summary, selectedIds, conflictPolicy, executing } = get();
    if (!summary || selectedIds.length === 0 || executing) return null;
    set({ executing: true, actionError: null });
    try {
      const report = await previewService.execute(
        summary.planId,
        selectedIds,
        conflictPolicy,
      );
      const ranSet = new Set(selectedIds);
      const staleIds = new Set(report.skippedStale.map((s) => s.itemId));
      const isFailed = (item: PlanItem) =>
        report.failed.some((msg) => msg.startsWith(`${item.sourcePath}: `));
      const prevItems = get().items;
      const nextItems = prevItems.filter(
        (item) => !ranSet.has(item.id) || staleIds.has(item.id) || isFailed(item),
      );
      const prunedCount = prevItems.length - nextItems.length;
      set({
        executing: false,
        lastReport: report,
        items: nextItems,
        total: Math.max(0, get().total - prunedCount),
        selectedIds: selectedIds.filter((id) => !ranSet.has(id)),
      });
      return report;
    } catch (e) {
      set({ executing: false, actionError: toUiError(e) });
      return null;
    }
  },

  discard: async () => {
    const { summary } = get();
    if (!summary) return;
    try {
      await previewService.discard(summary.planId);
      set({
        summary: null,
        items: [],
        total: 0,
        selectedIds: [],
        lastReport: null,
      });
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  clearActionError: () => set({ actionError: null }),

  reset: () => set({ ...initial }),
}));

