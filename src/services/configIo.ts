import {
  commands,
  type ConfigImportMode,
  type ConfigImportPreview,
  type ConfigImportReport,
  type ExclusionEntry_Serialize,
  type ExclusionInput_Deserialize,
  type ExclusionKind,
  type NotificationRecord_Serialize,
} from "@/bindings";
import { unwrap } from "./result";

export type ExclusionEntry = ExclusionEntry_Serialize;
export type ExclusionInput = ExclusionInput_Deserialize;
export type NotificationRecord = NotificationRecord_Serialize;
export type { ConfigImportMode, ConfigImportPreview, ConfigImportReport, ExclusionKind };

export const configIoService = {
  exportConfig: (): Promise<string> => unwrap(commands.configExport()),
  pickImportFile: (): Promise<string | null> => unwrap(commands.configPickImportFile()),
  previewImport: (rawJson: string, mode: ConfigImportMode = "merge"): Promise<ConfigImportPreview> =>
    unwrap(commands.configImportPreview(rawJson, mode)),
  applyImport: (rawJson: string, mode: ConfigImportMode): Promise<ConfigImportReport> =>
    unwrap(commands.configImportApply(rawJson, mode)),
};

export const exclusionsService = {
  list: (folderId: string | null = null): Promise<ExclusionEntry[]> => unwrap(commands.exclusionsList(folderId)),
  create: (input: ExclusionInput): Promise<ExclusionEntry> => unwrap(commands.exclusionsCreate(input)),
  remove: (id: string): Promise<boolean> => unwrap(commands.exclusionsDelete(id)),
};

export const notificationsService = {
  list: (): Promise<NotificationRecord[]> => unwrap(commands.notificationsList()),
  markRead: (id: string | null = null): Promise<number> => unwrap(commands.notificationsMarkRead(id)),
  clear: (): Promise<number> => unwrap(commands.notificationsClear()),
  sendTest: (): Promise<NotificationRecord | null> => unwrap(commands.notificationsTest()),
};

