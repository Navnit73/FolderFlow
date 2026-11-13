import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

/** Teach tailwind-merge our token-based spacing names (see src/styles/theme.css §5). */
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      spacing: ["row", "control", "control-sm", "sidebar", "header", "page", "titlebar"],
    },
  },
});

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
