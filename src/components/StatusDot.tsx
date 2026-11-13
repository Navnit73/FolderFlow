import { cn } from "@/lib/cn";
import type { Tone } from "@/lib/folderStatus";

const DOT: Record<Tone, string> = {
  active: "bg-status-active",
  paused: "bg-status-paused",
  error: "bg-status-error",
  idle: "bg-status-idle",
};

/** Colored dot + text label (status is never conveyed by color alone). */
export function StatusDot({ tone, label, className }: { tone: Tone; label: string; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-sm", className)}>
      <span aria-hidden className={cn("size-2 shrink-0 rounded-full", DOT[tone])} />
      <span>{label}</span>
    </span>
  );
}
