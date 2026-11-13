import { NavLink } from "react-router";
import { NAV_ITEMS } from "@/app/navigation";
import { cn } from "@/lib/cn";

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
    </aside>
  );
}
