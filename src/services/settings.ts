import { commands } from "@/bindings";
import type { AppInfo, Settings } from "@/types/settings";
import { unwrap } from "./result";

export const settingsService = {
  get: (): Promise<Settings> => unwrap(commands.settingsGet()),
  update: (settings: Settings): Promise<Settings> => unwrap(commands.settingsUpdate(settings)),
};

export const appService = {
  info: (): Promise<AppInfo> => unwrap(commands.appInfo()),
};
