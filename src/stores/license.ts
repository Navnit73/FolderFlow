import { create } from "zustand";
import { licenseService, type LicenseStatus } from "@/services/license";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";

type StoreStatus = "idle" | "loading" | "ready" | "error";

interface LicenseState {
  license: LicenseStatus | null;
  status: StoreStatus;
  busy: boolean;
  loadError: UiError | null;
  actionError: UiError | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  activate: (key: string) => Promise<LicenseStatus | null>;
  activateOffline: (signedKeyOrJson: string) => Promise<LicenseStatus | null>;
  pickAndActivateOfflineFile: () => Promise<LicenseStatus | null>;
  deactivate: () => Promise<LicenseStatus | null>;
  clearActionError: () => void;
  reset: () => void;
}

const initial = {
  license: null as LicenseStatus | null,
  status: "idle" as StoreStatus,
  busy: false,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

export const useLicense = create<LicenseState>((set, get) => {
  const runMutation = async (fn: () => Promise<LicenseStatus | null>): Promise<LicenseStatus | null> => {
    set({ busy: true, actionError: null });
    try {
      const next = await fn();
      if (next !== null) {
        set({ license: next, status: "ready", busy: false });
      } else {
        set({ busy: false });
      }
      return next;
    } catch (e) {
      // Preserve existing valid/stored license state if an activation/deactivation call rejects (UI-LIC-01).
      set({ busy: false, actionError: toUiError(e) });
      return null;
    }
  };

  return {
    ...initial,

    load: async () => {
      set({ status: "loading", loadError: null });
      try {
        const license = await licenseService.status();
        set({ license, status: "ready" });
      } catch (e) {
        set({ status: "error", loadError: toUiError(e) });
      }
    },

    refresh: async () => {
      try {
        const license = await licenseService.status();
        set({ license, status: "ready" });
      } catch {
        // Keep last known license state on transient refresh failure.
      }
    },

    activate: (key) => runMutation(() => licenseService.activate(key)),

    activateOffline: (signedKeyOrJson) => runMutation(() => licenseService.activateOffline(signedKeyOrJson)),

    pickAndActivateOfflineFile: () => runMutation(() => licenseService.pickAndActivateOfflineFile()),

    deactivate: () => runMutation(() => licenseService.deactivate()),

    clearActionError: () => set({ actionError: null }),

    reset: () => {
      const prevBusy = get().busy;
      if (prevBusy) {
        set({ ...initial, busy: false });
      } else {
        set({ ...initial });
      }
    },
  };
});

