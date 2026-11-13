import type { ReactNode } from "react";

interface PageHeaderProps {
  title: string;
  description?: string;
  actions?: ReactNode;
}

/** Top bar of every page. Doubles as the window drag region (transparent titlebar). */
export function PageHeader({ title, description, actions }: PageHeaderProps) {
  return (
    <header data-tauri-drag-region className="flex h-header shrink-0 items-center gap-4 border-b border-border px-page">
      <div data-tauri-drag-region className="min-w-0 flex-1">
        <h1 className="truncate font-display text-lg font-semibold tracking-tight text-fg">{title}</h1>
        {description && <p className="truncate text-xs text-fg-muted">{description}</p>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </header>
  );
}
