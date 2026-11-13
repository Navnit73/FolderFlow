import { commands } from "@/bindings";
import type {
  Category_Serialize,
  FolderAccess,
  FolderAction,
  FolderPatch_Deserialize,
  FolderSummary,
  KnownFolder,
  MonitoredFolder_Serialize,
  MonitoringStatus_Serialize,
  WatcherErrorView,
} from "@/bindings";
import { unwrap } from "./result";

export type Folder = MonitoredFolder_Serialize;
export type FolderPatch = FolderPatch_Deserialize;
export type MonitoringStatus = MonitoringStatus_Serialize;
export type Category = Category_Serialize;
export type { FolderAccess, FolderAction, FolderSummary, KnownFolder, WatcherErrorView };

export const foldersService = {
  list: (): Promise<Folder[]> => unwrap(commands.foldersList()),
  addKnown: (kind: KnownFolder, recursive: boolean): Promise<Folder> => unwrap(commands.foldersAddKnown(kind, recursive)),
  addWithPicker: (recursive: boolean): Promise<Folder | null> => unwrap(commands.foldersAddWithPicker(recursive)),
  update: (id: string, patch: FolderPatch): Promise<Folder> => unwrap(commands.foldersUpdate(id, patch)),
  remove: (id: string): Promise<null> => unwrap(commands.foldersRemove(id)),
  action: (id: string, action: FolderAction): Promise<null> => unwrap(commands.foldersAction(id, action)),
  pickSubfolder: (id: string): Promise<string | null> => unwrap(commands.foldersPickSubfolder(id)),
  summary: (id: string): Promise<FolderSummary> => unwrap(commands.foldersSummary(id)),
  errors: (id: string): Promise<WatcherErrorView[]> => unwrap(commands.foldersErrors(id)),
  openPrivacySettings: (): Promise<boolean> => unwrap(commands.openPrivacySettings()),
  status: (): Promise<MonitoringStatus> => unwrap(commands.monitoringStatus()),
  setPaused: (paused: boolean): Promise<MonitoringStatus> => unwrap(commands.monitoringSetPaused(paused)),
  categories: (): Promise<Category[]> => unwrap(commands.categoriesList()),
};
