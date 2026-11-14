import { create } from "zustand";
import { toUiError } from "@/services/result";
import {
  scheduleService,
  type Schedule,
  type ScheduleInput,
  type ScheduleRunOutcome,
} from "@/services/schedules";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

interface SchedulesState {
  schedules: Schedule[];
  lastOutcome: ScheduleRunOutcome | null;
  runningId: string | null;
  status: Status;
  loadError: UiError | null;
  actionError: UiError | null;
  load: () => Promise<void>;
  upsert: (input: ScheduleInput) => Promise<Schedule | null>;
  setEnabled: (id: string, enabled: boolean) => Promise<void>;
  remove: (id: string) => Promise<void>;
  runNow: (id: string) => Promise<ScheduleRunOutcome | null>;
  clearActionError: () => void;
  clearLastOutcome: () => void;
  reset: () => void;
}

const initial = {
  schedules: [] as Schedule[],
  lastOutcome: null as ScheduleRunOutcome | null,
  runningId: null as string | null,
  status: "idle" as Status,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

export const useSchedules = create<SchedulesState>((set, get) => ({
  ...initial,

  load: async () => {
    set({ status: "loading", loadError: null });
    try {
      const schedules = await scheduleService.list();
      set({ schedules, status: "ready" });
    } catch (e) {
      set({ status: "error", loadError: toUiError(e) });
    }
  },

  upsert: async (input) => {
    set({ actionError: null });
    try {
      const saved = await scheduleService.upsert(input);
      const list = get().schedules;
      const exists = list.some((s) => s.id === saved.id);
      set({
        schedules: exists
          ? list.map((s) => (s.id === saved.id ? saved : s))
          : [...list, saved],
      });
      return saved;
    } catch (e) {
      set({ actionError: toUiError(e) });
      return null;
    }
  },

  setEnabled: async (id, enabled) => {
    set({ actionError: null });
    try {
      const updated = await scheduleService.setEnabled(id, enabled);
      set((state) => ({
        schedules: state.schedules.map((s) => (s.id === id ? updated : s)),
      }));
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  remove: async (id) => {
    set({ actionError: null });
    try {
      await scheduleService.remove(id);
      set((state) => ({
        schedules: state.schedules.filter((s) => s.id !== id),
      }));
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  runNow: async (id) => {
    set({ runningId: id, actionError: null });
    try {
      const outcome = await scheduleService.runNow(id);
      const refreshed = await scheduleService.list();
      set({ schedules: refreshed, runningId: null, lastOutcome: outcome });
      return outcome;
    } catch (e) {
      set({ runningId: null, actionError: toUiError(e) });
      return null;
    }
  },

  clearActionError: () => set({ actionError: null }),
  clearLastOutcome: () => set({ lastOutcome: null }),
  reset: () => set({ ...initial }),
}));

