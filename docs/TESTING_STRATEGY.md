# FolderFlow — Testing Strategy (TDD)

> Every task in [PLAN_TRACKER.md](PLAN_TRACKER.md) lists the test IDs it must satisfy. Tests are written **first** (red), then code (green).

## 1. TDD loop per task

1. Read the task's FR IDs in [PRD.md](PRD.md).
2. Write tests named with their test ID (e.g. `fn r_path_03_rejects_parent_traversal()`), run them, confirm they **fail for the right reason**.
3. Implement the minimum to pass.
4. Refactor; keep green.
5. Mark the task 🧪 → ✅ in the tracker only after gates pass.

A test that passes before the implementation exists is a bad test — fix it.

## 2. Test ID scheme

`<LAYER>-<AREA>-<NN>` — `R-` Rust unit/integration, `UI-` frontend component, `E2E-` Playwright, `BENCH-` Criterion.
Test function names embed the ID in lowercase snake case so `cargo test r_move_` / `vitest -t UI-RB` selects a group.

## 3. Layers and tools

| Layer | Tool | Location | Runs in CI |
|---|---|---|---|
| Rust unit | `cargo test` | `#[cfg(test)] mod tests` beside code | ✅ |
| Rust property | `proptest` | same; `PROPTEST_CASES=10000` in CI | ✅ |
| Rust integration | `cargo test` + `tempfile` | `src-tauri/tests/` | ✅ (macOS runner) |
| Fault injection | `FaultyFs` impl of `Fs` trait | `src-tauri/tests/faults/` | ✅ |
| DB | SQLx in-memory + temp-file DB | `database/` tests | ✅ |
| Frontend unit | Vitest + React Testing Library | `src/**/*.test.tsx` | ✅ |
| Theme | Vitest + `scripts/check-tokens.mjs` | | ✅ |
| E2E | Playwright (+ tauri-driver where supported; otherwise web build with mocked bindings for UI-only flows) | `tests/e2e/` | ✅ (Phase 8) |
| Benchmarks | Criterion | `src-tauri/benches/` | manual / nightly |
| Coverage | `cargo llvm-cov`, Vitest c8 | | report |

## 4. Rules for tests

- **Never touch real user directories.** All FS tests use `tempfile::TempDir`. A guard `assert_test_path(p)` panics if a path is not under the temp root.
- **Inject time.** `Clock` trait; tests use `FixedClock`. No `sleep`-based timing in unit tests — use `tokio::time::pause()`.
- **Inject FS.** Read paths via `ReadOnlyFs`; mutations via `Fs`. `FaultyFs` can fail at the Nth call with a chosen errno.
- **Watcher tests are eventual.** Assert with a bounded `wait_until(timeout)` helper and also assert reconciliation catches what events missed.
- **Every bug = regression test** referencing `BUG-NNN` in the test name/comment.
- **No `#[ignore]`** without a linked BUGS.md entry and an owner.
- **Dry-run proof:** snapshot (path, size, mtime, inode) of the whole fixture tree before/after; must be identical.

## 5. Test catalog by phase

### Phase 1 — Foundation (19)
| ID | Asserts |
|---|---|
| UI-THEME-01 | `data-theme="dark"` swaps token values on `:root` |
| UI-THEME-02 | `system` follows `prefers-color-scheme` |
| UI-THEME-03 | `data-density="compact"` changes spacing/row tokens |
| UI-THEME-04 | `check-tokens` fails on a fixture containing `#fff`, `rgb(`, `bg-[#…]`, `text-red-500` |
| UI-THEME-05 | `[data-theme="dark"]` and system-dark blocks in `theme.css` define identical values |
| R-ERR-01..03 | AppError → UiError mapping; no internal paths/stack traces leak; codes stable |
| R-LOG-01..02 | log files created + rotated; path redaction when enabled |
| R-DB-01..04 | migrations apply on empty DB; FK enforced; WAL on; re-open idempotent |
| R-SET-01..03 | defaults; invalid values rejected; persisted across reopen |
| R-CMD-01 | generated `bindings.ts` is up to date (diff check) |
| UI-SHELL-01..03 | sidebar nav routes; header renders title/actions; theme switch persists |
| R-SEC-01 | capability file grants no fs/shell/http permissions to webview |
| R-SEC-02 | clippy `disallowed_methods` config present (triggering verified manually with a probe in the Phase 1 audit) |

Added during Phase 1: R-DB-05 (CHECK constraints), R-SET-04 (corrupt section → defaults), R-SET-05 (unknown/missing fields tolerated), R-SEC-01b (strict CSP), R-LOG-02b (home → `~`), BUG-004 store race, BUG-007 log filter, App bootstrap load/error, About retry, `cn` merge rules, check-tokens allow-list cases.

### Phase 2 — Filesystem engine (91 implemented)
| Area | Tests |
|---|---|
| Path authority | R-PATH-01..08, R-PATH-08b, R-PATH proptest (512 cases: no generated path escapes the root) |
| Classifier / categories | R-CLS-01..08, R-CAT-01, R-CAT-02, R-CAT-02b, R-CAT-03, R-CAT-03b |
| Read-only FS view | fs_view: kinds/hidden/identity, bounded header read refuses symlinks and FIFOs |
| Ignore rules | always-ignored names & partial downloads, hidden/temp by setting, cache/excluded dirs pruned, include/exclude extensions |
| Scanner | R-SCAN-01..09 (counts & categories, non-recursive, exclusions, ignore list, symlink loops, packages, 500-item batches, unreadable subfolder, missing root) |
| Folder registry / permissions | R-FOLD-01..05, R-PERM-01..02 |
| Reconcile | R-RECON-01..04 |
| Debounce / stability (pure) | debounce dedupe + overflow, R-STAB-01..04 + capacity |
| Live monitor (real FSEvents, temp dirs) | R-WATCH-01..06, R-STAB-05 (growing download → once, after rename), R-SUP-01 (start failures + backoff), R-SUP-02/03 (deleted → unavailable → recreated), R-SUP-04 (moved away/back + permission revoked/restored), pause/resume, global pause, overflow → reconcile loses nothing, rescan, 3 folders at once, **zero filesystem writes snapshot** |
| Regression | BUG-008, BUG-009, BUG-010, BUG-011, BUG-012, BUG-013 |
| UI | UI-FOLD-01..04 (+ error/retry, picker cancel, field errors, System Settings link), format helpers, migrated shell/App tests |

**Watcher tests are timing-sensitive by nature.** They use bounded waits (15 s), fast timing, and real FSEvents. Every phase audit runs the full Rust suite ≥ 10 times; any failure is investigated as a bug (BUG-013 was found this way), never retried away.

### Phase 3 — Rule engine (40)
R-RULE-01..06 · R-COND-01..15 (every operator family, tz-aware dates via FixedClock, DST boundaries) · R-GRP-01..05 (proptest: `Not(Not(x)) == x`, De Morgan equivalence, empty All = true / empty Any = false, depth limit) · R-VAL-01..06 · R-TPL-01..08 (proptest: output never contains `..`, `/` from variables, NUL, `:`; length limits; inside root) · R-EVAL-01..06 (priority, each execution mode, apply-rule cycle detected, per-pass move limit) · R-TMPL-01..03 (presets valid + disabled) · UI-RULES-01..03 · UI-RB-01..06 (add/remove/nest/reorder conditions, validation errors shown, template preview calls Rust)

### Phase 4 — Safe operations (42)
R-SFS-01..10 (rename_excl refuses existing dest; copy_excl refuses existing; fsync called; trash works; EXDEV detected; symlink dest parent refused; fault injection each step) ·
R-OPS-01..05 (only legal transitions; proptest random transition sequences never reach illegal state) ·
R-LEDG-01..04 (intent committed before mutation — FaultyFs asserts ledger row exists on first mutating call) ·
R-MOVE-01..08 · R-COPY-01..03 · R-REN-01..03 · R-CONF-01..05 (proptest: N concurrent conflicting moves → N distinct files, zero overwrites) ·
R-UNDO-01..07 · R-RECOV-01..08 (one test per recovery-table row) · UI-ACT-01..05

### Phase 5 — Preview & automation (30)
R-PLAN-01..10 (**01 = zero-write snapshot test**) · R-EXEC-01..05 (stale identity/size/mtime/dest each skip) · R-PIPE-01..04 (backpressure: flood 50k events, memory bounded, nothing lost after reconcile) · R-SCHED-01..08 (next-run math, DST, quiet hours, weekdays, catch-up skip/once, no overlap) · UI-PREV-01..04 · UI-SCHED-01..02 · UI-ONB-01..04 · R-ONB-01 (onboarding zero-write)

### Phase 6 — Advanced (30)
R-HASH-01..04 (known vectors; streaming buffer bound; cache invalidated on mtime/size change) · R-DUP-01..10 · R-SMART-01..04 · R-IO-01..06 (round trip; unknown schema_version rejected; malicious paths rejected; imported rules disabled) · R-NOTIF-01..04 (throttle; batching) · R-STATS-01..03 · UI-DASH-01..02 · UI-DUP-01..02 · UI-CAT-01

### Phase 7 — macOS (18)
R-TRAY-01..02 · R-LOGIN-01 · R-SYS-01..03 · R-NOTIF-05 · R-KEY-01..02 · R-LIC-01..06 (valid/invalid/tampered signature, offline works with network off, expiry n/a for lifetime, deactivate) · UI-KEYS-01..02 · UI-LIC-01

### Phase 8 — QA (20+)
R-FAIL-01..12 (spec §24.3: interrupt move, interrupt copy, DB interruption, dest unavailable, source vanishes, dest appears mid-exec, ENOSPC, EACCES, corrupted config, watcher failure, duplicate events, rapid changes) · E2E-01..06 · BENCH-01..07 (scan, event latency, rule throughput, query, hash MB/s, RSS, CPU)

### Phase 9 — Release (8)
Smoke tests on clean VMs: install, first launch, onboarding, organize, undo, restart persistence, offline, uninstall leaves user files intact.

## 6. CI gates (all must pass)

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo audit
pnpm lint && pnpm typecheck && pnpm test && pnpm build
pnpm check:tokens
```
No warning suppression to make a gate pass. Failing tests are fixed, not skipped.
