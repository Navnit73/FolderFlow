import { Link, NavLink } from "react-router";
import { NAV_ITEMS } from "@/app/navigation";
import { cn } from "@/lib/cn";
import { folderStatus } from "@/lib/folderStatus";
import { useFolders } from "@/stores/folders";
import { useLicense } from "@/stores/license";
import { StatusDot } from "./StatusDot";

/** Overall monitoring & license indicators for the sidebar footer (`DESIGN_SYSTEM.md §5`). */
function MonitoringIndicator() {
  const { folders, runtime, paused, status } = useFolders();
  const { license, status: licStatus } = useLicense();

  if (status !== "ready" && licStatus !== "ready") return null;

  let view: { tone: "active" | "paused" | "error" | "idle"; label: string } | null = null;
  if (status === "ready") {
    const active = folders.filter((f) => f.enabled);
    if (active.length === 0) view = { tone: "idle", label: "No folders monitored" };
    else if (paused) view = { tone: "paused", label: "Monitoring paused" };
    else if (active.some((f) => folderStatus(f, runtime[f.id]).tone === "error")) {
      view = { tone: "error", label: "Needs attention" };
    } else {
      view = { tone: "active", label: "Monitoring" };
    }
  }

  return (
    <div className="space-y-1.5 border-t border-border px-4 py-3 text-fg-muted">
      {view && <StatusDot tone={view.tone} label={view.label} className="text-xs" />}
      {licStatus === "ready" && license && (
        <Link
          to="/license"
          className={cn(
            "block text-xs hover:underline",
            license.licensed ? "font-medium text-success" : "text-fg-subtle hover:text-fg",
          )}
        >
          {license.licensed ? "Licensed ✓" : "Unlicensed"}
        </Link>
      )}
    </div>
  );
}

export function Sidebar() {
  return (
    <aside className="flex w-sidebar shrink-0 flex-col border-r border-border bg-sidebar">
      <div data-tauri-drag-region className="h-titlebar shrink-0" />
      <nav aria-label="Main" className="flex flex-1 flex-col gap-0.5 px-2 pt-2">
        {NAV_ITEMS.map(({ id, label, path, icon: Icon, available }) =>
          available ? (
            <NavLink
              key={id}
              to={path}
              className={({ isActive }) =>
                cn(
                  "flex h-row items-center gap-2 rounded-md px-2 text-sm transition-colors duration-(--ff-duration-base)",
                  isActive ? "bg-selected font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
                )
              }
            >
              <Icon className="size-4" aria-hidden />
              {label}
            </NavLink>
          ) : (
            <span
              key={id}
              aria-disabled="true"
              title="Not available yet"
              className="flex h-row cursor-default items-center gap-2 rounded-md px-2 text-sm text-fg-disabled"
            >
              <Icon className="size-4" aria-hidden />
              {label}
              <span className="ml-auto text-2xs uppercase tracking-wide">Soon</span>
            </span>
          ),
        )}
      </nav>
      <MonitoringIndicator />
    </aside>
  );
}
