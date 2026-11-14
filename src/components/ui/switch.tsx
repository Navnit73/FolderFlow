import { cn } from "@/lib/cn";

interface SwitchProps {
  id: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  ariaLabel?: string;
}

/** macOS-style toggle. Label it with `<label htmlFor={id}>` or `ariaLabel`. */
export function Switch({ id, checked, onChange, disabled, ariaLabel }: SwitchProps) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border border-border transition-colors duration-(--ff-duration-base) disabled:opacity-50",
        checked ? "bg-accent" : "bg-muted",
      )}
    >
      <span
        aria-hidden
        className={cn(
          "inline-block size-4 rounded-full bg-surface shadow-popover transition-transform duration-(--ff-duration-base)",
          checked ? "translate-x-4" : "translate-x-0.5",
        )}
      />
    </button>
  );
}
