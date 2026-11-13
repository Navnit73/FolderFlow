/**
 * check-tokens — enforces the single-theme-file rule (NFR-040, ADR-003).
 *
 * Fails if any file under src/ other than src/styles/theme.css contains a
 * hardcoded design value, and verifies the dark-theme blocks in theme.css
 * stay in sync.
 *
 * Usage: pnpm check:tokens
 * Escape hatch (needs a reason in review): append `tokens-ignore-line` in a comment.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export interface Violation {
  file: string;
  line: number;
  rule: string;
  text: string;
}

const THEME_FILE = "src/styles/theme.css";
const EXEMPT_FILES = new Set([THEME_FILE, "src/bindings.ts"]);

const PALETTE =
  "slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|black|white";
const COLOR_UTILS =
  "bg|text|border|border-[trblxyse]|ring|ring-offset|fill|stroke|from|via|to|outline|decoration|divide|placeholder|caret|accent|shadow";

const RULES: { rule: string; re: RegExp }[] = [
  { rule: "hex-color", re: /(?<![\w&/-])#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})\b/ },
  { rule: "color-function", re: /\b(?:rgba?|hsla?|hwb|oklch|oklab|lab|lch|color-mix)\(/ },
  { rule: "tailwind-palette-class", re: new RegExp(`(?<![\\w-])(?:${COLOR_UTILS})-(?:${PALETTE})(?:-\\d{2,3})?(?![\\w-])`) },
  // p-[13px], bg-[#eee], w-[calc(..)] — but allow var(--ff-*) and variants like data-[state=open]:
  { rule: "arbitrary-value", re: /(?<![\w-])[a-z][\w-]*-\[(?!var\(--ff-)[^\]\s]+\](?!:)/ },
  {
    rule: "inline-visual-style",
    re: /style=\{\{[^}]*\b(?:color|background|backgroundColor|fontSize|fontFamily|fontWeight|margin\w*|padding\w*|border\w*|boxShadow|lineHeight)\s*:/,
  },
];

/** Scans one source file's text. `file` is a repo-relative path. */
export function scanSource(text: string, file: string): Violation[] {
  if (file.endsWith(".css") && file !== THEME_FILE) {
    return [{ file, line: 1, rule: "extra-stylesheet", text: `Only ${THEME_FILE} may contain CSS.` }];
  }
  const out: Violation[] = [];
  text.split("\n").forEach((line, i) => {
    if (line.includes("tokens-ignore-line")) return;
    for (const { rule, re } of RULES) {
      if (re.test(line)) out.push({ file, line: i + 1, rule, text: line.trim() });
    }
  });
  return out;
}

type Decls = Map<string, string>;

/** Extracts `--name: value;` declarations from the first rule whose selector matches exactly. */
function declsForSelector(css: string, selector: string, within?: string): Decls {
  const scope = within ? (css.split(within)[1] ?? "") : css;
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = new RegExp(`(?:^|[\\s,}])${escaped}\\s*\\{([^}]*)\\}`, "m").exec(scope);
  const decls: Decls = new Map();
  if (!m?.[1]) return decls;
  for (const d of m[1].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    decls.set(d[1] as string, (d[2] as string).trim());
  }
  return decls;
}

export function darkBlocks(css: string): { explicit: Decls; system: Decls } {
  return {
    explicit: declsForSelector(css, ':root[data-theme="dark"]'),
    system: declsForSelector(css, ":root:not([data-theme])", "@media (prefers-color-scheme: dark)"),
  };
}

/** UI-THEME-05: the explicit dark block and the system-dark block must be identical. */
export function checkThemeParity(css: string): string[] {
  const { explicit, system } = darkBlocks(css);
  const problems: string[] = [];
  if (explicit.size === 0) problems.push('Missing :root[data-theme="dark"] block.');
  if (system.size === 0) problems.push("Missing system-dark :root:not([data-theme]) block.");
  for (const [k, v] of explicit) {
    if (!system.has(k)) problems.push(`System-dark block is missing ${k}.`);
    else if (system.get(k) !== v) problems.push(`${k} differs: explicit "${v}" vs system "${system.get(k)}".`);
  }
  for (const k of system.keys()) if (!explicit.has(k)) problems.push(`Explicit dark block is missing ${k}.`);
  return problems;
}

function walk(dir: string, files: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, files);
    else if (/\.(tsx?|css)$/.test(name) && !/\.test\.tsx?$/.test(name)) files.push(p);
  }
  return files;
}

export function run(root: string): { violations: Violation[]; parity: string[] } {
  const violations: Violation[] = [];
  for (const abs of walk(join(root, "src"))) {
    const file = relative(root, abs).split(sep).join("/");
    if (EXEMPT_FILES.has(file)) continue;
    violations.push(...scanSource(readFileSync(abs, "utf8"), file));
  }
  const parity = checkThemeParity(readFileSync(join(root, THEME_FILE), "utf8"));
  return { violations, parity };
}

const invokedDirectly = process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href;
if (invokedDirectly) {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const { violations, parity } = run(root);
  for (const v of violations) console.error(`${v.file}:${v.line}  [${v.rule}]  ${v.text}`);
  for (const p of parity) console.error(`${THEME_FILE}  [theme-parity]  ${p}`);
  const total = violations.length + parity.length;
  if (total > 0) {
    console.error(`\n✗ check:tokens found ${total} problem(s). All design values belong in ${THEME_FILE}.`);
    process.exit(1);
  }
  console.log(`✓ check:tokens — all UI styling comes from ${THEME_FILE}`);
}
