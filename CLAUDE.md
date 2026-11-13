# FolderFlow — Working Agreement

macOS file organizer. Tauri 2 + Rust core + React/TS UI. **Priority: user-data safety → performance → polish.**

## Docs are the source of truth
Read before working; update in the same change as code. Drift between docs and code is a bug.

| Doc | Holds |
|---|---|
| `docs/PRD.md` | Requirements (FR/NFR IDs — never renumber) |
| `docs/PLAN_TRACKER.md` | Phase/task status — **check here first for what to do next** |
| `docs/ARCHITECTURE.md` | Module boundaries, commands, events, routes |
| `docs/DATABASE.md` | Schema; must match `src-tauri/migrations/` |
| `docs/SAFETY_MODEL.md` | Filesystem invariants S1–S11 |
| `docs/TESTING_STRATEGY.md` | TDD rules + test catalog (test IDs) |
| `docs/AUDIT_PROCESS.md`, `docs/audits/` | End-of-phase audit + records |
| `docs/DESIGN_SYSTEM.md` | UI rules |
| `docs/DECISIONS.md`, `docs/BUGS.md`, `docs/CHANGELOG.md` | ADRs, bug log, changelog |

## Per-phase loop (mandatory)
PLAN → RED (write failing tests by test ID) → GREEN → REFACTOR → GATES → AUDIT (record in `docs/audits/phase-N.md`, bugs → `BUGS.md` + regression test) → DOCS → VERIFY (all earlier phases' tests). A phase is ✅ only after its audit is signed off.

## Hard rules
- All filesystem mutation lives in `src-tauri/src/engine/safe_fs/` only. Never use `std::fs::rename/copy/write/remove_*` or `File::create` elsewhere (clippy-banned). No-overwrite: `renamex_np(RENAME_EXCL)`, `create_new(true)`.
- Write ledger intent before any mutation. Dry-run code takes `&dyn ReadOnlyFs` only.
- No shell/`std::process::Command`. No network outside the license module.
- Frontend never sends raw source/destination paths for operations — only IDs.
- **UI styling comes only from `src/styles/theme.css`.** No hex/rgb/hsl, no Tailwind palette classes, no arbitrary `[...]` values in components. `pnpm check:tokens` must pass.
- Tests touch only temp dirs. Never real user folders.
- No warning suppression or skipped tests to make gates pass.

## Gates
```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test && cargo audit
pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm check:tokens
```
