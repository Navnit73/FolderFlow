import { create } from "zustand";
import { settingsService } from "@/services/settings";
import { toUiError } from "@/services/result";
import { applyAppearance } from "@/theme/appearance";
import type { Settings, UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

interface SettingsState {
  settings: Settings | null;
  status: Status;
  saving: boolean;
  /** Error from the initial load. */
  loadError: UiError | null;
  /** Error from the most recent save. */
  saveError: UiError | null;
  load: () => Promise<void>;
  /** Applies a change optimistically, persists it, and reverts on failure. */
  update: (change: (current: Settings) => Settings) => Promise<void>;
}

/** Monotonic id of the most recent save; older responses must not overwrite newer state. */
let saveSeq = 0;

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  status: "idle",
  saving: false,
  loadError: null,
  saveError: null,

  load: async () => {
    set({ status: "loading", loadError: null });
    try {
      const settings = await settingsService.get();
      applyAppearance(settings.appearance);
      set({ settings, status: "ready" });
    } catch (e) {
      set({ status: "error", loadError: toUiError(e) });
    }
  },

  update: async (change) => {
    const previous = get().settings;
    if (!previous) return;
    const seq = ++saveSeq;
    const next = change(previous);
    applyAppearance(next.appearance);
    set({ settings: next, saving: true, saveError: null });
    try {
      const saved = await settingsService.update(next);
      if (seq === saveSeq) {
        applyAppearance(saved.appearance);
        set({ settings: saved, saving: false });
      }
    } catch (e) {
      const saveError = toUiError(e);
      // Saves may complete out of order, so never roll back to a local guess:
      // re-read what Rust actually persisted (BUG-004).
      let truth: Settings = previous;
      try {
        truth = await settingsService.get();
      } catch {
        // Keep `previous` if the backend is unreachable; saveError tells the user.
      }
      applyAppearance(truth.appearance);
      set({ settings: truth, saveError, saving: seq !== saveSeq && get().saving });
    }
  },
}));
