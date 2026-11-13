import type { Folder } from "@/services/folders";
import type { FolderRuntime } from "@/stores/folders";

export type Tone = "active" | "paused" | "error" | "idle";
export interface StatusView {
  label: string;
  tone: Tone;
  needsPermission: boolean;
}

/** One place that turns backend state into the label + tone the UI shows. */
export function folderStatus(folder: Folder, runtime: FolderRuntime | undefined): StatusView {
  if (!folder.enabled) return { label: "Disabled", tone: "idle", needsPermission: false };
  const state = runtime?.state;
  switch (state) {
    case "watching":
      return { label: "Watching", tone: "active", needsPermission: false };
    case "starting":
    case "scanning":
      return { label: "Scanning…", tone: "active", needsPermission: false };
    case "paused":
      return { label: "Paused", tone: "paused", needsPermission: false };
    case "unavailable":
      return { label: "Unavailable", tone: "error", needsPermission: false };
    case "permission_denied":
      return { label: "No access", tone: "error", needsPermission: true };
    case "error":
      return { label: "Error", tone: "error", needsPermission: false };
    case "stopped":
      return { label: "Stopped", tone: "idle", needsPermission: false };
    default:
      break;
  }
  switch (folder.health) {
    case "paused":
      return { label: "Paused", tone: "paused", needsPermission: false };
    case "unavailable":
      return { label: "Unavailable", tone: "error", needsPermission: false };
    case "permission_denied":
      return { label: "No access", tone: "error", needsPermission: true };
    case "error":
      return { label: "Error", tone: "error", needsPermission: false };
    default:
      return { label: "Starting…", tone: "idle", needsPermission: false };
  }
}

export function isPaused(folder: Folder, runtime: FolderRuntime | undefined): boolean {
  return runtime ? runtime.state === "paused" : folder.health === "paused";
}
