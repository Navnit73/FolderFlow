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

function parseVars(block: string): Map<string, string> {
  const vars = new Map<string, string>();
  for (const m of block.matchAll(/(--ff-[a-z0-9-]+):\s*([^;]+);/gi)) {
    const k = m[1];
    const v = m[2];
    if (k !== undefined && v !== undefined) {
      vars.set(k, v.trim());
    }
  }
  return vars;
}

function parseColor(
  raw: string,
  lookup: (name: string) => string | undefined,
  depth = 0,
): { r: number; g: number; b: number; a: number } {
  if (depth > 8) throw new Error(`cyclic var: ${raw}`);
  const varMatch = /^var\(\s*(--ff-[a-z0-9-]+)\s*\)$/i.exec(raw.trim());
  const varName = varMatch?.[1];
  if (varName) {
    const resolved = lookup(varName);
    if (!resolved) throw new Error(`missing token ${varName}`);
    return parseColor(resolved, lookup, depth + 1);
  }
  const hexMatch = /^#([0-9a-f]{6})$/i.exec(raw.trim());
  const hexVal = hexMatch?.[1];
  if (hexVal) {
    const n = Number.parseInt(hexVal, 16);
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 1 };
  }
  const rgbMatch = /^rgb\(\s*(\d+)\s+(\d+)\s+(\d+)(?:\s*\/\s*([\d.]+))?\s*\)$/i.exec(raw.trim());
  if (rgbMatch) {
    return {
      r: Number(rgbMatch[1]),
      g: Number(rgbMatch[2]),
      b: Number(rgbMatch[3]),
      a: rgbMatch[4] === undefined ? 1 : Number(rgbMatch[4]),
    };
  }
  throw new Error(`unsupported CSS color expression: ${raw}`);
}

function compositeOver(
  fg: { r: number; g: number; b: number; a: number },
  bg: { r: number; g: number; b: number; a: number },
): { r: number; g: number; b: number; a: number } {
  return {
    r: fg.r * fg.a + bg.r * (1 - fg.a),
    g: fg.g * fg.a + bg.g * (1 - fg.a),
    b: fg.b * fg.a + bg.b * (1 - fg.a),
    a: 1,
  };
}

function relativeLuminance(rgb: { r: number; g: number; b: number }): number {
  const [r, g, b] = [rgb.r, rgb.g, rgb.b].map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrastRatio(
  a: { r: number; g: number; b: number },
  b: { r: number; g: number; b: number },
): number {
  const l1 = relativeLuminance(a);
  const l2 = relativeLuminance(b);
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
}

describe("UI-A11Y-01 WCAG 2.1 AA token contrast (NFR-043 / BUG-035)", () => {
  it("passes >= 4.5:1 across all text, subtle badge, button, and category tokens in Light & Dark modes", () => {
    const primitivesBlock = themeCss.match(/1\.\s*PRIMITIVES[\s\S]*?:root\s*\{([\s\S]*?)\n\}/i)?.[1] ?? "";
    const lightBlock = themeCss.match(/:root,\s*:root\[data-theme="light"\]\s*\{([\s\S]*?)\n\}/i)?.[1] ?? "";
    const darkBlock = themeCss.match(/:root\[data-theme="dark"\]\s*\{([\s\S]*?)\n\}/i)?.[1] ?? "";

    const primitives = parseVars(primitivesBlock);
    const light = new Map([...primitives, ...parseVars(lightBlock)]);
    const dark = new Map([...primitives, ...parseVars(darkBlock)]);

    const categoryKeys = [
      "--ff-cat-documents",
      "--ff-cat-images",
      "--ff-cat-videos",
      "--ff-cat-audio",
      "--ff-cat-archives",
      "--ff-cat-spreadsheets",
      "--ff-cat-presentations",
      "--ff-cat-installers",
      "--ff-cat-design",
      "--ff-cat-development",
      "--ff-cat-fonts",
      "--ff-cat-ebooks",
      "--ff-cat-other",
    ];

    for (const [modeName, tokens] of [
      ["light", light],
      ["dark", dark],
    ] as const) {
      const getToken = (key: string): string => {
        const value = tokens.get(key);
        if (!value) throw new Error(`Missing token ${key} in ${modeName} theme`);
        return value;
      };
      const lookup = (k: string) => tokens.get(k);
      const baseBg = parseColor(getToken("--ff-bg"), lookup);
      const selectedBg = compositeOver(parseColor(getToken("--ff-bg-selected"), lookup), baseBg);
      const surfaces = [
        ...["--ff-bg", "--ff-bg-sidebar", "--ff-bg-subtle", "--ff-bg-muted"].map((s) =>
          parseColor(getToken(s), lookup),
        ),
        selectedBg,
      ];

      for (const fgTok of ["--ff-fg", "--ff-fg-muted", "--ff-fg-subtle"]) {
        const fg = parseColor(getToken(fgTok), lookup);
        for (const surf of surfaces) {
          expect(contrastRatio(fg, surf), `${modeName} ${fgTok}`).toBeGreaterThanOrEqual(4.5);
        }
      }

      // Solid button fills with `--ff-fg-on-accent`
      const onAccent = parseColor(getToken("--ff-fg-on-accent"), lookup);
      for (const solidTok of [
        "--ff-accent",
        "--ff-accent-hover",
        "--ff-danger-solid",
        "--ff-danger-solid-hover",
      ]) {
        const bg = parseColor(getToken(solidTok), lookup);
        expect(contrastRatio(onAccent, bg), `${modeName} on-accent on ${solidTok}`).toBeGreaterThanOrEqual(4.5);
      }

      // Status text over subtle badge surfaces (composited over both bg and muted)
      for (const [fgTok, subtleTok] of [
        ["--ff-accent-fg", "--ff-accent-subtle"],
        ["--ff-success", "--ff-success-subtle"],
        ["--ff-warning", "--ff-warning-subtle"],
        ["--ff-danger", "--ff-danger-subtle"],
        ["--ff-info", "--ff-info-subtle"],
      ] as const) {
        const fg = parseColor(getToken(fgTok), lookup);
        const subtle = parseColor(getToken(subtleTok), lookup);
        for (const surf of surfaces) {
          const effBg = compositeOver(subtle, surf);
          expect(contrastRatio(fg, effBg), `${modeName} ${fgTok} on ${subtleTok}`).toBeGreaterThanOrEqual(4.5);
        }
      }

      // Category badge colors on all neutral surfaces
      for (const catTok of categoryKeys) {
        const fg = parseColor(getToken(catTok), lookup);
        for (const surf of surfaces) {
          expect(contrastRatio(fg, surf), `${modeName} ${catTok}`).toBeGreaterThanOrEqual(4.5);
        }
      }
    }
  });
});
