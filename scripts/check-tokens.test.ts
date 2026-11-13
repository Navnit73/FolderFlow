// @vitest-environment node
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { checkThemeParity, darkBlocks, run, scanSource } from "./check-tokens.ts";

const root = process.cwd();
const themeCss = readFileSync(resolve(root, "src/styles/theme.css"), "utf8");

describe("UI-THEME-04 check-tokens flags hardcoded design values", () => {
  const bad: [string, string][] = [
    ["hex-color", `const c = "#fff";`],
    ["hex-color", `<div className="x" data-c="#1d1d1f" />`],
    ["color-function", `const c = "rgb(0 0 0)";`],
    ["color-function", `background: "hsl(0 0% 50%)"`],
    ["tailwind-palette-class", `<p className="text-red-500">`],
    ["tailwind-palette-class", `<p className="bg-white">`],
    ["arbitrary-value", `<div className="p-[13px]">`],
    ["arbitrary-value", `<div className="bg-[#eee]">`],
    ["inline-visual-style", `<div style={{ color: "red" }} />`],
  ];
  it.each(bad)("flags %s in: %s", (rule, line) => {
    expect(scanSource(line, "src/x.tsx").map((v) => v.rule)).toContain(rule);
  });

  const good = [
    `<div className="bg-surface text-fg-muted border-border rounded-md h-control p-page">`,
    `<div className="bg-accent text-fg-on-accent hover:bg-accent-hover text-cat-images">`,
    `<div className="data-[state=open]:bg-hover aria-[disabled=true]:opacity-50">`,
    `<div className="w-[var(--ff-sidebar-width)]">`,
    `<a href="#/settings/appearance">`,
    `<div style={{ width: \`\${pct}%\` }} />`,
  ];
  it.each(good)("allows token usage: %s", (line) => {
    expect(scanSource(line, "src/x.tsx")).toEqual([]);
  });

  it("rejects any stylesheet other than theme.css", () => {
    expect(scanSource("", "src/components/extra.css")[0]?.rule).toBe("extra-stylesheet");
  });

  it("honours tokens-ignore-line", () => {
    expect(scanSource(`const c = "#fff"; // tokens-ignore-line: reason`, "src/x.ts")).toEqual([]);
  });
});

describe("UI-THEME-05 dark theme blocks stay in sync", () => {
  it("theme.css explicit-dark and system-dark blocks are identical", () => {
    expect(checkThemeParity(themeCss)).toEqual([]);
    expect(darkBlocks(themeCss).explicit.size).toBeGreaterThan(30);
  });

  it("detects drift between the two blocks", () => {
    const drifted = themeCss.replace(
      /(@media \(prefers-color-scheme: dark\)[\s\S]*?--ff-bg:)\s*[^;]+;/,
      "$1 var(--ff-gray-0);",
    );
    expect(checkThemeParity(drifted).join("\n")).toContain("--ff-bg differs");
  });
});

describe("repository is clean", () => {
  it("has no token violations in src/", () => {
    const { violations, parity } = run(root);
    expect(violations).toEqual([]);
    expect(parity).toEqual([]);
  });
});
