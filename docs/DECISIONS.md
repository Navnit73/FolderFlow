# Architecture Decision Records

Append-only. Status: Proposed → Accepted → Superseded (by ADR-x).

| ADR | Title | Status | Date |
|---|---|---|---|
| 001 | Tauri 2 + Rust core + React UI | Accepted | 2026-11-13 |
| 002 | Direct distribution (Developer ID), not Mac App Store | Accepted | 2026-11-13 |
| 003 | Single theme file controls all UI | Accepted | 2026-11-13 |
| 004 | Generate TS bindings from Rust (tauri-specta) | Proposed — confirm in P1-T09 | 2026-11-13 |
| 005 | No-overwrite primitives via `renamex_np(RENAME_EXCL)` | Accepted | 2026-11-13 |
| 006 | SQLx with runtime-checked queries + migrations | Proposed — confirm in P1-T07 | 2026-11-13 |
| 007 | pnpm as package manager | Accepted | 2026-11-13 |

---

### ADR-001 — Tauri 2 + Rust core + React UI
Spec-mandated. Small binary, Rust owns filesystem logic, web UI for fast iteration.

### ADR-002 — Direct distribution, not Mac App Store
**Context:** MAS requires App Sandbox; monitoring arbitrary user folders and external drives under the sandbox needs security-scoped bookmarks for every folder and complicates background operation and login items.
**Decision:** Ship Developer-ID-signed, notarized DMG with hardened runtime, non-sandboxed. Rely on TCC prompts for Desktop/Documents/Downloads and the user-selected folder picker. Still store folder paths only after explicit user selection (least privilege by convention).
**Consequence:** MAS later would require a bookmark layer; `PathAuthority` is the single place that would change.

### ADR-003 — Single theme file
**Decision:** `src/styles/theme.css` holds every design value (colors light/dark, typography, spacing, radii, density, motion, z-index, category/status colors) as CSS custom properties, and maps them into Tailwind v4 via `@theme inline`. Components use only token-backed utilities (`bg-surface`, `text-fg-muted`, `rounded-md`). `scripts/check-tokens.mjs` fails CI on hex/rgb/hsl literals, Tailwind palette classes (`text-red-500`) and arbitrary values (`p-[13px]`) outside that file.
**Consequence:** Rebranding or re-theming = editing one file.

### ADR-004 — TS bindings from Rust
**Decision (proposed):** tauri-specta to generate `src/bindings.ts` for commands, events and types so the TS layer cannot drift from Rust. If its Tauri 2 support is not stable at P1-T09, fall back to `ts-rs` for types + a hand-written thin invoke wrapper checked by R-CMD-01.

### ADR-005 — No-overwrite primitives
`std::fs::rename` maps to `rename(2)`, which replaces an existing destination. Use `renamex_np(from, to, RENAME_EXCL)` (macOS 10.12+) via `libc`; file creation via `OpenOptions::create_new(true)`. Ban the std variants via clippy outside `safe_fs`.

### ADR-006 — SQLx runtime queries
Compile-time `query!` macros need a live DB or committed `.sqlx` offline data; acceptable but adds CI friction. Proposal: `sqlx::migrate!` + `query_as::<_, T>()` with `FromRow`, covered by repository tests. Revisit if query bugs appear.

### ADR-007 — pnpm
Faster, strict dependency resolution. Available on the dev machine.
