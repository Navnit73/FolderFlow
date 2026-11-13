import type { Density, ThemeMode } from "@/types/settings";

export interface Appearance {
  theme: ThemeMode;
  density: Density;
}

/**
 * Applies appearance to <html>. The only code that writes data-theme / data-density.
 * "system" removes data-theme so theme.css follows prefers-color-scheme.
 */
export function applyAppearance({ theme, density }: Appearance, root: HTMLElement = document.documentElement): void {
  if (theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);
  root.setAttribute("data-density", density);
}
