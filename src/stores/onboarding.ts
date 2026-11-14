import { create } from "zustand";
import { commands } from "@/bindings";
import {
  onboardingService,
  type FolderAccess,
  type KnownFolder,
  type MonitoredFolder_Serialize,
  type OnboardingCompleteReport,
  type OnboardingScanReport,
  type OnboardingState,
  type OnboardingStepUpdate,
  type OrganizationPreference,
  type RuleTemplate_Serialize,
} from "@/services/onboarding";
import { toUiError, unwrap } from "@/services/result";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

interface OnboardingStoreState {
  state: OnboardingState | null;
  folders: MonitoredFolder_Serialize[];
  accessByFolder: Record<string, FolderAccess>;
  presets: RuleTemplate_Serialize[];
  scanReport: OnboardingScanReport | null;
  scanning: boolean;
  completing: boolean;
  lastCompleteReport: OnboardingCompleteReport | null;
  status: Status;
  loadError: UiError | null;
  actionError: UiError | null;
  load: () => Promise<OnboardingState | null>;
  loadResources: () => Promise<void>;
  saveStep: (update: OnboardingStepUpdate) => Promise<OnboardingState | null>;
  addKnownFolder: (kind: KnownFolder, recursive: boolean) => Promise<void>;
  addPickerFolder: (recursive: boolean) => Promise<void>;
  recheckAccess: (folderId: string) => Promise<void>;
  openPrivacySettings: () => Promise<void>;
  setPreferenceLocal: (pref: OrganizationPreference) => void;
  togglePresetLocal: (presetId: string) => void;
  runScanPreview: () => Promise<OnboardingScanReport | null>;
  complete: (confirmEnableAutomation: boolean) => Promise<OnboardingCompleteReport | null>;
  restartOnboarding: () => Promise<OnboardingState | null>;
  clearActionError: () => void;
  resetStore: () => void;
}

const initial = {
  state: null as OnboardingState | null,
  folders: [] as MonitoredFolder_Serialize[],
  accessByFolder: {} as Record<string, FolderAccess>,
  presets: [] as RuleTemplate_Serialize[],
  scanReport: null as OnboardingScanReport | null,
  scanning: false,
  completing: false,
  lastCompleteReport: null as OnboardingCompleteReport | null,
  status: "idle" as Status,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

export const useOnboarding = create<OnboardingStoreState>((set, get) => ({
  ...initial,

  load: async () => {
    set({ status: "loading", loadError: null });
    try {
      const state = await onboardingService.getState();
      set({ state, status: "ready" });
      return state;
    } catch (e) {
      set({ status: "error", loadError: toUiError(e) });
      return null;
    }
  },

  loadResources: async () => {
    try {
      const [folders, presets] = await Promise.all([
        unwrap(commands.foldersList()),
        unwrap(commands.rulesPresets()),
      ]);
      const accessEntries = await Promise.all(
        folders.map(async (f) => {
          try {
            const access = await unwrap(commands.foldersAccess(f.id));
            return [f.id, access] as const;
          } catch {
            return [f.id, "granted" as FolderAccess] as const;
          }
        }),
      );
      set({
        folders,
        presets,
        accessByFolder: Object.fromEntries(accessEntries),
      });
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  saveStep: async (update) => {
    set({ actionError: null });
    try {
      const next = await onboardingService.setStep(update);
      set({ state: next });
      return next;
    } catch (e) {
      set({ actionError: toUiError(e) });
      return null;
    }
  },

  addKnownFolder: async (kind, recursive) => {
    set({ actionError: null });
    try {
      const folder = await unwrap(commands.foldersAddKnown(kind, recursive));
      const access = await unwrap(commands.foldersAccess(folder.id));
      set((s) => ({
        folders: s.folders.some((f) => f.id === folder.id)
          ? s.folders
          : [...s.folders, folder],
        accessByFolder: { ...s.accessByFolder, [folder.id]: access },
      }));
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  addPickerFolder: async (recursive) => {
    set({ actionError: null });
    try {
      const folder = await unwrap(commands.foldersAddWithPicker(recursive));
      if (!folder) return;
      const access = await unwrap(commands.foldersAccess(folder.id));
      set((s) => ({
        folders: s.folders.some((f) => f.id === folder.id)
          ? s.folders
          : [...s.folders, folder],
        accessByFolder: { ...s.accessByFolder, [folder.id]: access },
      }));
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  recheckAccess: async (folderId) => {
    try {
      const access = await unwrap(commands.foldersAccess(folderId));
      set((s) => ({
        accessByFolder: { ...s.accessByFolder, [folderId]: access },
      }));
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  openPrivacySettings: async () => {
    try {
      await unwrap(commands.openPrivacySettings());
    } catch (e) {
      set({ actionError: toUiError(e) });
    }
  },

  setPreferenceLocal: (organizationPreference) => {
    set((s) =>
      s.state ? { state: { ...s.state, organizationPreference } } : s,
    );
  },

  togglePresetLocal: (presetId) => {
    set((s) => {
      if (!s.state) return s;
      const exists = s.state.selectedPresetIds.includes(presetId);
      const selectedPresetIds = exists
        ? s.state.selectedPresetIds.filter((id) => id !== presetId)
        : [...s.state.selectedPresetIds, presetId];
      return { state: { ...s.state, selectedPresetIds } };
    });
  },

  runScanPreview: async () => {
    set({ scanning: true, actionError: null });
    try {
      const selected = get().state?.selectedPresetIds ?? null;
      const report = await onboardingService.scanPreview(selected);
      set({ scanReport: report, scanning: false });
      return report;
    } catch (e) {
      set({ scanning: false, actionError: toUiError(e) });
      return null;
    }
  },

  complete: async (confirmEnableAutomation) => {
    const selected = get().state?.selectedPresetIds ?? [];
    set({ completing: true, actionError: null });
    try {
      const report = await onboardingService.complete(
        selected,
        confirmEnableAutomation,
      );
      set({
        completing: false,
        state: report.state,
        lastCompleteReport: report,
      });
      return report;
    } catch (e) {
      set({ completing: false, actionError: toUiError(e) });
      return null;
    }
  },

  restartOnboarding: async () => {
    set({ actionError: null });
    try {
      const next = await onboardingService.reset();
      set({ state: next, scanReport: null, lastCompleteReport: null, status: "ready" });
      return next;
    } catch (e) {
      set({ actionError: toUiError(e) });
      return null;
    }
  },

  clearActionError: () => set({ actionError: null }),
  resetStore: () => set({ ...initial }),
}));

