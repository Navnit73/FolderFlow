import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { applyAppearance } from "./appearance";

const css = readFileSync(resolve(process.cwd(), "src/styles/theme.css"), "utf8");

function block(selector: string): Map<string, string> {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const body = new RegExp(`${esc}\\s*\\{([^}]*)\\}`).exec(css)?.[1] ?? "";
  return new Map([...body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1] as string, (m[2] as string).trim()]));
}

describe("UI-THEME-01 dark theme swaps token values", () => {
  it("sets data-theme=dark on <html>", () => {
    applyAppearance({ theme: "dark", density: "comfortable" });
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("dark block overrides every light semantic color token with a different value", () => {
    const light = block(':root[data-theme="light"]');
    const dark = block(':root[data-theme="dark"]');
    expect(light.size).toBeGreaterThan(30);
    for (const [token, value] of light) {
      if (token === "--ff-fg-on-accent" || token === "--ff-accent") continue; // intentionally shared
      expect(dark.has(token), `dark theme missing ${token}`).toBe(true);
      expect(dark.get(token), `${token} identical in light and dark`).not.toBe(value);
    }
  });
});

describe("UI-THEME-02 system mode follows the OS", () => {
  it("removes data-theme so the prefers-color-scheme block applies", () => {
    applyAppearance({ theme: "dark", density: "comfortable" });
    applyAppearance({ theme: "system", density: "comfortable" });
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(css).toMatch(/@media \(prefers-color-scheme: dark\)\s*\{\s*:root:not\(\[data-theme\]\)/);
  });
});

describe("UI-THEME-03 density changes sizing tokens", () => {
  it("sets data-density and compact tokens are smaller", () => {
    applyAppearance({ theme: "light", density: "compact" });
    expect(document.documentElement).toHaveAttribute("data-density", "compact");
    const px = (v: string | undefined) => Number.parseInt(v ?? "", 10);
    const comfy = block(':root[data-density="comfortable"]');
    const compact = block(':root[data-density="compact"]');
    for (const t of ["--ff-row-height", "--ff-control-height", "--ff-page-padding", "--ff-space-unit"]) {
      expect(px(compact.get(t)), t).toBeLessThan(px(comfy.get(t)));
    }
  });
});
