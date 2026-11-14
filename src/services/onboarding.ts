import { commands } from "@/bindings";
import type {
  FolderAccess,
  KnownFolder,
  MonitoredFolder_Serialize,
  OnboardingCompleteReport_Serialize,
  OnboardingScanReport,
  OnboardingState_Serialize,
  OnboardingStepUpdate_Deserialize,
  OrganizationPreference,
  RuleTemplate_Serialize,
} from "@/bindings";
import { unwrap } from "./result";

export type OnboardingState = OnboardingState_Serialize;
export type OnboardingStepUpdate = OnboardingStepUpdate_Deserialize;
export type OnboardingCompleteReport = OnboardingCompleteReport_Serialize;
export type {
  FolderAccess,
  KnownFolder,
  MonitoredFolder_Serialize,
  OnboardingScanReport,
  OrganizationPreference,
  RuleTemplate_Serialize,
};

export const onboardingService = {
  getState: (): Promise<OnboardingState> => unwrap(commands.onboardingState()),

  setStep: (update: OnboardingStepUpdate): Promise<OnboardingState> =>
    unwrap(
      commands.onboardingSetStep({
        currentStep: update.currentStep,
        organizationPreference: update.organizationPreference ?? null,
        selectedPresetIds: update.selectedPresetIds ?? null,
      }),
    ),

  scanPreview: (selectedPresetIds: string[] | null = null): Promise<OnboardingScanReport> =>
    unwrap(commands.onboardingScanPreview(selectedPresetIds)),

  complete: (
    selectedPresetIds: string[],
    confirmEnableAutomation: boolean,
  ): Promise<OnboardingCompleteReport> =>
    unwrap(commands.onboardingComplete(selectedPresetIds, confirmEnableAutomation)),

  reset: (): Promise<OnboardingState> => unwrap(commands.onboardingReset()),
};

