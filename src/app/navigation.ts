import {
  Activity,
  CalendarClock,
  Copy,
  Eye,
  FolderOpen,
  LayoutDashboard,
  ListChecks,
  Settings,
  Tags,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  id: string;
  label: string;
  path: string;
  icon: LucideIcon;
  /** False until the screen is fully implemented (no placeholder screens, NFR-044). */
  available: boolean;
}

/** Sidebar order per docs/DESIGN_SYSTEM.md §5. Flip `available` when a screen ships. */
export const NAV_ITEMS: NavItem[] = [
  { id: "overview", label: "Overview", path: "/overview", icon: LayoutDashboard, available: false },
  { id: "folders", label: "Folders", path: "/folders", icon: FolderOpen, available: true },
  { id: "rules", label: "Rules", path: "/rules", icon: ListChecks, available: true },
  { id: "preview", label: "Preview", path: "/preview", icon: Eye, available: true },
  { id: "activity", label: "Activity", path: "/activity", icon: Activity, available: true },
  { id: "duplicates", label: "Duplicates", path: "/duplicates", icon: Copy, available: false },
  { id: "scheduler", label: "Scheduler", path: "/scheduler", icon: CalendarClock, available: true },
  { id: "categories", label: "Categories", path: "/categories", icon: Tags, available: false },
  { id: "settings", label: "Settings", path: "/settings", icon: Settings, available: true },
];

export function defaultPath(): string {
  return NAV_ITEMS.find((i) => i.available)?.path ?? "/settings";
}
