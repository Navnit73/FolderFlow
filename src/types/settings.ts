import type {
  AppInfo,
  Density,
  Settings_Serialize,
  ThemeMode,
  UiError_Serialize,
} from "@/bindings";

/** Canonical frontend shapes (the fully-populated "serialize" side of the Rust types). */
export type Settings = Settings_Serialize;
export type UiError = UiError_Serialize;
export type { AppInfo, Density, ThemeMode };
