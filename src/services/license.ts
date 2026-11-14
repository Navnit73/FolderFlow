import { commands, type LicenseStatus_Serialize, type LoginItemStatus } from "@/bindings";
import { unwrap } from "./result";

export type LicenseStatus = LicenseStatus_Serialize;
export type { LoginItemStatus };

export const licenseService = {
  status: (): Promise<LicenseStatus> => unwrap(commands.licenseStatus()),
  activate: (key: string): Promise<LicenseStatus> => unwrap(commands.licenseActivate(key)),
  activateOffline: (signedKeyOrJson: string): Promise<LicenseStatus> =>
    unwrap(commands.licenseActivateOffline(signedKeyOrJson)),
  pickAndActivateOfflineFile: (): Promise<LicenseStatus | null> =>
    unwrap(commands.licensePickAndActivateOfflineFile()),
  deactivate: (): Promise<LicenseStatus> => unwrap(commands.licenseDeactivate()),
};

export const loginItemService = {
  status: (): Promise<LoginItemStatus> => unwrap(commands.loginItemStatus()),
  openSettings: (): Promise<boolean> => unwrap(commands.openLoginItemsSettings()),
};

