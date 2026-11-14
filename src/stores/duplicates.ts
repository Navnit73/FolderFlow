import { create } from "zustand";
import { events, type DupProgressEvent } from "@/bindings";
import {
  duplicatesService,
  type DupAction,
  type DupActionReport,
  type DupExportFormat,
  type DuplicateGroupSummary,
  type DuplicateScanOptions,
  type DuplicateScanReport,
  type DuplicateSummaryTotals,
} from "@/services/duplicates";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

const EMPTY_TOTALS: DuplicateSummaryTotals = {
  totalGroups: 0,
  activeGroups: 0,
  ignoredGroups: 0,
  totalDuplicateCopies: 0,
  totalWastedBytes: 0,
};

interface DuplicatesState {
  groups: DuplicateGroupSummary[];
  totals: DuplicateSummaryTotals;
  selectedGroupId: string | null;
  includeIgnored: boolean;
  differentNamesOnly: boolean;
  search: string;
  scanning: boolean;
  scanProgress: DupProgressEvent | null;
  lastScanReport: DuplicateScanReport | null;
  status: Status;
  loadError: UiError | null;
  actionError: UiError | null;
  load: () => Promise<void>;
  scan: (opts?: DuplicateScanOptions) => Promise<DuplicateScanReport | null>;
  setIncludeIgnored: (v: boolean) => Promise<void>;
  setDifferentNamesOnly: (v: boolean) => void;
  setSearch: (q: string) => Promise<void>;
  selectGroup: (id: string | null) => void;
  applyAction: (action: DupAction) => Promise<DupActionReport | null>;
  exportReport: (format: DupExportFormat) => Promise<string | null>;
  revealFile: (fileRecordId: string) => Promise<boolean>;
  clearActionError: () => void;
  reset: () => void;
}

const initial = {
  groups: [] as DuplicateGroupSummary[],
  totals: EMPTY_TOTALS,
  selectedGroupId: null as string | null,
  includeIgnored: false,
  differentNamesOnly: false,
  search: "",
  scanning: false,
  scanProgress: null as DupProgressEvent | null,
  lastScanReport: null as DuplicateScanReport | null,
  status: "idle" as Status,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

export const useDuplicates = create<DuplicatesState>((set, get) => ({
  ...initial,

  load: async () => {
    const { includeIgnored, search, selectedGroupId } = get();
    set({ status: "loading", loadError: null });
    try {
      const resp = await duplicatesService.listGroups(includeIgnored, search.trim() || null);
      const nextSelected =
        selectedGroupId && resp.groups.some((g) => g.id === selectedGroupId)
          ? selectedGroupId
          : (resp.groups[0]?.id ?? null);
      set({
        groups: resp.groups,
        totals: resp.totals,
        selectedGroupId: nextSelected,
        status: "ready",
      });
    } catch (e) {
      set({ status: "error", loadError: toUiError(e) });
    }
  },

  scan: async (opts = {}) => {
    set({ scanning: true, scanProgress: null, actionError: null });
    const unlisten = await events.dupProgressEvent.listen((ev) => {
      set({ scanProgress: ev.payload });
    });
    try {
      const report = await duplicatesService.scan(opts);
      const { includeIgnored, search, selectedGroupId } = get();
      const resp =
        includeIgnored || search.trim()
          ? await duplicatesService.listGroups(includeIgnored, search.trim() || null)
          : report.groups;
      const nextSelected =
        selectedGroupId && resp.groups.some((g) => g.id === selectedGroupId)
          ? selectedGroupId
          : (resp.groups[0]?.id ?? null);
      set({
        groups: resp.groups,
        totals: resp.totals,
        selectedGroupId: nextSelected,
        lastScanReport: report,
        scanning: false,
        status: "ready",
      });
      return report;
    } catch (e) {
      set({ scanning: false, actionError: toUiError(e) });
      return null;
    } finally {
      unlisten();
    }
  },

  setIncludeIgnored: async (v) => {
    set({ includeIgnored: v });
    await get().load();
  },

  setDifferentNamesOnly: (v) => set({ differentNamesOnly: v }),

  setSearch: async (q) => {
    set({ search: q });
    await get().load();
  },

  selectGroup: (id) => set({ selectedGroupId: id }),

  applyAction: async (action) => {
    set({ actionError: null });
    try {
      const report = await duplicatesService.applyAction(action);
      await get().load();
      return report;
    } catch (e) {
      set({ actionError: toUiError(e) });
      return null;
    }
  },

  exportReport: async (format) => {
    set({ actionError: null });
    try {
      return await duplicatesService.exportReport(format, get().includeIgnored);
    } catch (e) {
      set({ actionError: toUiError(e) });
      return null;
    }
  },

  revealFile: async (fileRecordId) => {
    set({ actionError: null });
    try {
      return await duplicatesService.revealFile(fileRecordId);
    } catch (e) {
      set({ actionError: toUiError(e) });
      return false;
    }
  },

  clearActionError: () => set({ actionError: null }),

  reset: () => set(initial),
}));
