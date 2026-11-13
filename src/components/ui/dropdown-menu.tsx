import type { LucideIcon } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { cn } from "@/lib/cn";
import { Button, type ButtonProps } from "./button";

export interface MenuItem {
  label: string;
  icon?: LucideIcon;
  onSelect: () => void;
  separatorBefore?: boolean;
}

interface DropdownMenuProps {
  label: ReactNode;
  items: MenuItem[];
  align?: "start" | "end";
  variant?: ButtonProps["variant"];
}

/** Accessible menu button: arrow keys, Escape, click-outside. */
export function DropdownMenu({ label, items, align = "end", variant = "primary" }: DropdownMenuProps) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    if (!open) return;
    itemRefs.current[0]?.focus();
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const els = itemRefs.current.filter(Boolean) as HTMLButtonElement[];
    const i = els.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = els[(i + (e.key === "ArrowDown" ? 1 : -1) + els.length) % els.length];
      next?.focus();
    }
  };

  return (
    <div ref={root} className="relative inline-block">
      <Button
        ref={trigger}
        variant={variant}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((o) => !o)}
      >
        {label}
      </Button>
      {open && (
        <div
          id={menuId}
          role="menu"
          onKeyDown={onKeyDown}
          className={cn(
            "absolute top-full z-(--ff-z-dropdown) mt-1 min-w-48 rounded-lg border border-border bg-popover p-1 shadow-popover",
            align === "end" ? "right-0" : "left-0",
          )}
        >
          {items.map((item, i) => {
            const Icon = item.icon;
            return (
              <div key={item.label}>
                {item.separatorBefore && <div role="separator" className="my-1 border-t border-border" />}
                <button
                  ref={(el) => {
                    itemRefs.current[i] = el;
                  }}
                  type="button"
                  role="menuitem"
                  tabIndex={-1}
                  onClick={() => {
                    close();
                    item.onSelect();
                  }}
                  className="flex h-row w-full items-center gap-2 rounded-sm px-2 text-left text-sm text-fg hover:bg-hover focus-visible:bg-hover focus-visible:outline-none"
                >
                  {Icon && <Icon className="size-4 text-fg-muted" aria-hidden />}
                  {item.label}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
