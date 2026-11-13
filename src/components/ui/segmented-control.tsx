import type { LucideIcon } from "lucide-react";
import { useId, type KeyboardEvent } from "react";
import { cn } from "@/lib/cn";

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
  icon?: LucideIcon;
}

interface SegmentedControlProps<T extends string> {
  label: string;
  value: T;
  options: SegmentOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
}

/** Accessible radio group styled as a macOS segmented control (arrow keys move selection). */
export function SegmentedControl<T extends string>({ label, value, options, onChange, disabled }: SegmentedControlProps<T>) {
  const name = useId();
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const i = options.findIndex((o) => o.value === value);
    const delta = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!delta || i < 0) return;
    e.preventDefault();
    const next = options[(i + delta + options.length) % options.length];
    if (next) onChange(next.value);
  };

  return (
    <div
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled}
      onKeyDown={onKeyDown}
      className="inline-flex rounded-md border border-border bg-muted p-0.5"
    >
      {options.map((o) => {
        const selected = o.value === value;
        const Icon = o.icon;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            name={name}
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            disabled={disabled}
            onClick={() => onChange(o.value)}
            className={cn(
              "inline-flex h-control-sm items-center gap-1.5 rounded-sm px-3 text-sm transition-colors duration-(--ff-duration-base)",
              selected ? "bg-surface text-fg shadow-popover" : "text-fg-muted hover:text-fg",
            )}
          >
            {Icon && <Icon className="size-3.5" aria-hidden />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
