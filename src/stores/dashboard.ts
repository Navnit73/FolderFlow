import { create } from "zustand";
import { dashboardService, type DashboardStats } from "@/services/dashboard";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

interface DashboardState {
  stats: DashboardStats | null;
  status: Status;
  error: UiError | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  reset: () => void;
}

const initial = {
  stats: null as DashboardStats | null,
  status: "idle" as Status,
  error: null as UiError | null,
};

export const useDashboard = create<DashboardState>((set) => ({
  ...initial,

  load: async () => {
    set({ status: "loading", error: null });
    try {
      const stats = await dashboardService.getStats();
      set({ stats, status: "ready" });
    } catch (e) {
      set({ status: "error", error: toUiError(e) });
    }
  },

  refresh: async () => {
    try {
      const stats = await dashboardService.getStats();
      set({ stats, status: "ready", error: null });
    } catch {
      // Preserve last known metrics on background refresh failure.
    }
  },

  reset: () => set(initial),
}));

