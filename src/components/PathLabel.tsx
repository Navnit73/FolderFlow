import { cn } from "@/lib/cn";

/** Middle-truncates long paths so both the root and the leaf stay visible. */
export function truncateMiddle(text: string, max: number): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.ceil(keep * 0.4);
  return `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`;
}

export function PathLabel({ path, max = 64, className }: { path: string; max?: number; className?: string }) {
  return (
    <span title={path} className={cn("ff-selectable block truncate text-xs text-fg-muted", className)}>
      {truncateMiddle(path, max)}
    </span>
  );
}
