# FolderFlow — Phase Plan Tracker

> **This file is the source of truth for progress.** Update it in the same change as the code.
> Requirements live in [PRD.md](PRD.md). Test IDs are defined in [TESTING_STRATEGY.md](TESTING_STRATEGY.md). Audits live in [audits/](audits/).

## Status legend

| Symbol | Meaning                                     |
| ------ | ------------------------------------------- |
| ⬜     | Not started                                 |
| 🟨     | In progress                                 |
| 🧪     | Tests written (red), implementation pending |
| ✅     | Done — tests green, gates passed            |
| 🔍     | In audit                                    |
| ⛔     | Blocked (reason in Notes)                   |

## Phase summary

| Phase | Name                 | Status | Tests                                              | Audit                           | Exit date  |
| ----- | -------------------- | ------ | -------------------------------------------------- | ------------------------------- | ---------- |
| 0     | Planning & docs      | ✅     | n/a                                                | [phase-0](audits/phase-0.md) ✅ | 2026-11-13 |
| 1     | Foundation           | ✅     | 62 / 62 (24 Rust · 38 TS)                          | [phase-1](audits/phase-1.md) ✅ | 2026-11-13 |
| 2     | Filesystem engine    | ✅     | 91 / 91 (77 Rust · 14 TS)                          | [phase-2](audits/phase-2.md) ✅ | 2026-11-13 |
| 3     | Rule engine          | ✅     | 67 / 40 (55 Rust · 12 TS)                          | [phase-3](audits/phase-3.md) ✅ | 2026-11-14 |
| 4     | Safe operations      | ✅     | 65 / 42 (60 Rust · 5 TS)                           | [phase-4](audits/phase-4.md) ✅ | 2026-11-14 |
| 5     | Preview & automation | ✅     | 43 / 30 (33 Rust · 10 TS)                          | [phase-5](audits/phase-5.md) ✅ | 2026-11-14 |
| 6     | Advanced features    | ✅     | 48 / 30 (36 Rust · 12 TS)                          | [phase-6](audits/phase-6.md) ✅ | 2026-11-14 |
| 7     | macOS integration    | ✅     | 18 / 18                                            | [phase-7](audits/phase-7.md) ✅ | 2026-11-14 |
| 8     | Quality assurance    | ✅     | 24 / 20 (12 Rust failure · 5 E2E/A11y · 7 Benches) | [phase-8](audits/phase-8.md) ✅ | 2026-11-14 |
| 9     | Production release   | 🟨     | 0 / 11 (3 release checks · 8 smoke)                | ⬜                              | —          |

Test counts are the planned minimum; update as tests are added.

---

## Per-phase workflow (mandatory for every phase)

Every phase runs this loop. A phase is **not** done until all 8 steps are checked.

```
1. PLAN     → Confirm tasks + FR IDs below; note any new ADRs in DECISIONS.md
2. RED      → Write failing tests for each task (test IDs listed per phase)
3. GREEN    → Implement the minimum to pass
4. REFACTOR → Clean up; keep tests green
5. GATES    → cargo fmt --check · cargo clippy -D warnings · cargo test
              npm run lint · typecheck · test · build · check:tokens
6. AUDIT    → Run the AUDIT_PROCESS.md checklist; record in audits/phase-N.md
              Every bug found → BUGS.md entry + regression test → fix → re-run gates
7. DOCS     → Update this tracker, ARCHITECTURE/DATABASE/SAFETY docs, CHANGELOG.md
8. VERIFY   → Re-run all previous phases' tests (no regressions); mark phase ✅
```

---

## Phase 0 — Planning & docs ✅

| ID     | Task                                                                              | Status |
| ------ | --------------------------------------------------------------------------------- | ------ |
| P0-T01 | PRD with stable FR/NFR IDs                                                        | ✅     |
| P0-T02 | Phase tracker (this file)                                                         | ✅     |
| P0-T03 | Architecture: Rust module boundaries, Tauri command interface, routes, components | ✅     |
| P0-T04 | Database schema + migration plan                                                  | ✅     |
| P0-T05 | Filesystem safety model                                                           | ✅     |
| P0-T06 | Testing strategy (TDD) with test catalog                                          | ✅     |
| P0-T07 | Audit process + templates                                                         | ✅     |
| P0-T08 | Design system + single theme file `src/styles/theme.css`                          | ✅     |
| P0-T09 | Decision log (ADRs), bug log, changelog                                           | ✅     |
| P0-T10 | `CLAUDE.md` working agreement                                                     | ✅     |

---

## Phase 1 — Foundation

**Goal:** A running Tauri 2 app with the UI shell, SQLite, logging, settings and the theme wired up. No file operations yet.
**FRs:** FR-180–185 (settings storage), NFR-011, NFR-012, NFR-031, NFR-040, NFR-041, NFR-050

| ID     | Task                                                                                                                                         | Tests first     | Status |
| ------ | -------------------------------------------------------------------------------------------------------------------------------------------- | --------------- | ------ |
| P1-T01 | Init Tauri 2 + React + TS (strict) + Vite; pnpm; repo layout per ARCHITECTURE.md                                                             | —               | ✅     |
| P1-T02 | Tailwind v4 + shadcn/ui wired **only** through `src/styles/theme.css`                                                                        | UI-THEME-01..03 | ✅     |
| P1-T03 | `scripts/check-tokens.ts`: fail on hex/rgb/hsl/named colors or arbitrary px values outside `theme.css`; verify dark/system-dark blocks match | UI-THEME-04..05 | ✅     |
| P1-T04 | Rust crate layout with `lib.rs` wiring only (modules are added when their phase starts — no empty stubs)                                     | —               | ✅     |
| P1-T05 | Error model: `AppError` (thiserror) → serializable `UiError { code, message, hint }`                                                         | R-ERR-01..03    | ✅     |
| P1-T06 | `tracing` + rolling file appender in `~/Library/Logs/com.folderflow.desktop`, retention setting, path redaction                              | R-LOG-01..02    | ✅     |
| P1-T07 | SQLite via SQLx: WAL, foreign keys, migration `0001_init.sql` (full schema from DATABASE.md)                                                 | R-DB-01..04     | ✅     |
| P1-T08 | Settings service (typed, defaults, validation) + `settings_get/settings_update` commands                                                     | R-SET-01..03    | ✅     |
| P1-T09 | Typed bindings Rust → TS (tauri-specta, see ADR-004)                                                                                         | R-CMD-01        | ✅     |
| P1-T10 | App shell: sidebar, header, router, Zustand stores, theme switch (light/dark/system), density                                                | UI-SHELL-01..03 | ✅     |
| P1-T11 | Tauri capabilities: deny-by-default; CSP set                                                                                                 | R-SEC-01        | ✅     |
| P1-T12 | Clippy `disallowed_methods` config banning `std::fs::rename`, `remove_file`, `remove_dir_all`, `std::process::Command` outside `safe_fs`     | R-SEC-02        | ✅     |
| P1-T13 | CI workflow (GitHub Actions, macos-15) running all gates                                                                                     | —               | ✅     |

**Exit criteria:** app launches; theme toggles; settings persist across restart; all gates green; token check passes; audit-1 recorded. ✅ Met — see [audits/phase-1.md](audits/phase-1.md).

**Phase 1 notes**

- Tauri bundle id is `com.folderflow.desktop` (BUG-001). Data: `~/Library/Application Support/com.folderflow.desktop/`, logs: `~/Library/Logs/com.folderflow.desktop/`.
- Settings UI ships **Appearance** and **About** only; other settings sections appear when the features they control exist (no inert toggles, NFR-044). All settings are already stored and validated in Rust.
- Sidebar shows unbuilt screens as disabled "Soon" items (see ARCHITECTURE §7).
- Deferred: NFR-032 (corrupted DB → backup & recreate) moved to **P4-T09** because it needs the safe file-operation layer.
- CI workflow is written but has not run yet (repo is not under git / on GitHub).

---

## Phase 2 — Filesystem engine

**Goal:** Folder selection, scanning, classification, watching with stability checks and reconciliation. Still read-only — no file mutations.
**FRs:** FR-002, FR-003, FR-010–025, FR-030–035

| ID     | Task                                                                                                                                      | Tests first                  | Status |
| ------ | ----------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------- | ------ |
| P2-T01 | Path authority: normalize, canonicalize, approved roots registry, reject traversal/symlinks                                               | R-PATH-01..08 (+proptest)    | ✅     |
| P2-T02 | Folder registry: add/edit/remove/enable monitored folders (native picker via `tauri-plugin-dialog`)                                       | R-FOLD-01..04                | ✅     |
| P2-T03 | TCC permission probing + denial detection + System Settings deep link + Info.plist purpose strings                                        | R-PERM-01..02, BUG-011       | ✅     |
| P2-T04 | Scanner: streaming `walkdir`-style, no symlink follow, loop guard, exclusions, ignore list, bounded concurrency, batched DB upserts       | R-SCAN-01..07                | ✅     |
| P2-T05 | Classifier: extension table + `infer` signatures + disagreement → Other; `.app` bundles atomic                                            | R-CLS-01..08                 | ✅     |
| P2-T06 | Custom categories CRUD + priority resolution (service + `categories_list`; editing UI/commands ship with the Categories screen in P6-T08) | R-CAT-01..03                 | ✅     |
| P2-T07 | Watcher service (`notify` FSEvents) → debounce/dedupe → bounded channel                                                                   | R-WATCH-01..05 (integration) | ✅     |
| P2-T08 | Stability checker (size+mtime N checks, configurable interval)                                                                            | R-STAB-01..04                | ✅     |
| P2-T09 | Reconciliation scans (interval + on wake + on restart); diff against `file_records`                                                       | R-RECON-01..03               | ✅     |
| P2-T10 | Watcher supervisor: restart with backoff, health states, volume unmount/remount                                                           | R-SUP-01..04                 | ✅     |
| P2-T11 | Folders screen: list, add, edit, pause/resume, health, errors, stats                                                                      | UI-FOLD-01..04               | ✅     |

**Exit criteria:** monitoring 3+ folders in a temp-dir harness detects create/modify/rename/delete; partial downloads are never emitted as stable; unmount simulation recovers; zero writes outside DB (verified by test). Audit-2 recorded. ✅ Met — see [audits/phase-2.md](audits/phase-2.md).

**Phase 2 notes**

- What "new file" means (matters for Phase 5): see SAFETY_MODEL.md § "New-file semantics". First scan of a folder and scope changes record a baseline and report nothing; incomplete scans never delete records.
- Monitored folders may not overlap (ADR-013).
- Global monitoring settings (ignore hidden/temp, stability, reconcile interval) are read when folder tasks start. When the Monitoring settings UI ships (Phase 5/6), saving must call `MonitorService::set_settings` and restart folders.
- Categories editing commands + UI → P6-T08; the classifier must be rebuilt (`MonitorService::set_classifier`) on every category change.
- Sleep/wake hook → P7-T03 calls `MonitorService::rescan_all` (already implemented).

---

## Phase 3 — Rule engine

**Goal:** Typed rule model, evaluator, templates, and the visual Rule Builder. Evaluation is pure (no IO beyond injected file facts).
**FRs:** FR-040–057, FR-070–076

| ID     | Task                                                                                                              | Tests first               | Status |
| ------ | ----------------------------------------------------------------------------------------------------------------- | ------------------------- | ------ |
| P3-T01 | Rule models (serde, versioned): `ConditionNode::{All, Any, Not, Leaf}`, `Action`, `ExecutionMode`                 | R-RULE-01..03             | ✅     |
| P3-T02 | Condition evaluators: text, size, date (tz-aware, injectable clock), location, advanced                           | R-COND-01..15 (+proptest) | ✅     |
| P3-T03 | Group logic AND/OR/NOT, nesting depth ≤ 5, short-circuit                                                          | R-GRP-01..05 (+proptest)  | ✅     |
| P3-T04 | Rule validation: regex compile + size limit, empty groups, unsafe actions, destination loops                      | R-VAL-01..06              | ✅     |
| P3-T05 | Template engine: variables, date source, sanitization, path validation, root containment                          | R-TPL-01..08 (+proptest)  | ✅     |
| P3-T06 | Evaluator: priority order, execution modes, apply-another-rule with cycle + depth limit, per-pass move limit      | R-EVAL-01..06             | ✅     |
| P3-T07 | Rules CRUD commands + persistence + stats columns                                                                 | R-RULE-04..06             | ✅     |
| P3-T08 | Starter/smart rule templates (PDF, screenshots, images, videos, archives, invoices, old downloads) — all disabled | R-TMPL-01..03             | ✅     |
| P3-T09 | Rules list screen (enable, reorder priority, duplicate, delete w/ confirm)                                        | UI-RULES-01..03           | ✅     |
| P3-T10 | Rule Builder UI: conditions tree editor, actions list, inline validation, live template preview                   | UI-RB-01..06              | ✅     |

**Exit criteria:** spec §6.2 example evaluates correctly; proptests pass 10k cases; builder creates/edits nested rules round-trip. Audit-3 recorded. ✅ Met — see [audits/phase-3.md](audits/phase-3.md).

**Phase 3 notes**

- `Clock` trait (`SystemClock` + `FixedClock` with DST transition support) lives in `src-tauri/src/utils/clock.rs` (landed early in Phase 3 for FR-045/FR-071; will also serve the Phase 5 scheduler).
- Promoted `chrono`, `glob`, and `regex` from `Cargo.lock` into `src-tauri/Cargo.toml` (0 new lockfile crates — ADR-015).
- `RuleService::test_draft_on_folder` (`rules_test_folder`) evaluates a draft rule against scanned files in SQLite with zero filesystem writes (FR-057, NFR-004).
- Approved destination roots selected via `rules_pick_destination_folder` are validated through `validate_new_root` and persisted in `destination_roots`.

---

## Phase 4 — Safe operations

**Goal:** Transactional move/copy/rename with write-ahead ledger, conflict handling, undo, and crash recovery. **Highest-risk phase — audit is mandatory with fault injection.**
**FRs:** FR-090–101, NFR-001–003

| ID     | Task                                                                                                                                                                  | Tests first                 | Status |
| ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- | ------ |
| P4-T01 | `safe_fs` module: the **only** mutating FS code. `rename_excl` (renamex_np RENAME_EXCL), `copy_excl` (create_new + streaming + fsync), `move_to_trash`                | R-SFS-01..08                | ✅     |
| P4-T02 | `Fs` trait with real + fault-injecting test impl (fail at step N, ENOSPC, EACCES, ENOENT, EXDEV)                                                                      | R-SFS-09..10                | ✅     |
| P4-T03 | Operation state machine with legal transitions only                                                                                                                   | R-OPS-01..05 (+proptest)    | ✅     |
| P4-T04 | Ledger: write intent (FULL sync) before any FS mutation; batch IDs                                                                                                    | R-LEDG-01..04               | ✅     |
| P4-T05 | Move pipeline: same-volume atomic; cross-volume staged copy → verify BLAKE3 → excl rename → remove source                                                             | R-MOVE-01..08               | ✅     |
| P4-T06 | Copy + rename pipelines                                                                                                                                               | R-COPY-01..03, R-REN-01..03 | ✅     |
| P4-T07 | Conflict resolver: auto-rename `(n)`, skip, ask (→ Needs review), keep both; race-safe                                                                                | R-CONF-01..05 (+proptest)   | ✅     |
| P4-T08 | Undo: last / selected / batch / restore structure; occupied-path prompt; copy-undo hash check                                                                         | R-UNDO-01..07               | ✅     |
| P4-T09 | Recovery on startup for every non-terminal state; orphan staging file cleanup (to Trash, never delete); corrupted DB → back up + recreate (NFR-032, deferred from P1) | R-RECOV-01..09              | ✅     |
| P4-T10 | Activity screen (table, filters, pagination, undo, reveal, retry, CSV export, clear)                                                                                  | UI-ACT-01..05               | ✅     |

**Exit criteria:** fault-injection suite proves: for every crash point, the file exists exactly at source or destination (or both, never neither), and no existing file is ever overwritten. Audit-4 recorded with the safety checklist fully signed off. ✅ Met — see [audits/phase-4.md](audits/phase-4.md).

**Phase 4 notes**

- `SafeFs` (`src-tauri/src/engine/safe_fs/`) is the sole module with `#![allow(clippy::disallowed_methods)]` for mutating filesystem operations. Production Trash uses `NSFileManager::trashItemAtURL_resultingItemURL_error`; tests use `SafeFs::with_trash_dir` so test runs never pollute the developer's `~/.Trash` (ADR-016).
- `LedgerToken` (minted only by `Ledger::begin_in_progress` with `PRAGMA synchronous = FULL`) and `VerifiedSourceToken` (minted only by `SafeFs::verify_copied`) enforce write-ahead logging and verified-copy source unlinking at the Rust type level.
- Startup crash recovery (`recover_startup`) runs automatically in `AppState::build` across all approved roots, and `database::open` backs up and recreates corrupted SQLite databases (`<name>.corrupt-<epoch>.bak`, `NFR-032`).

---

## Phase 5 — Preview & automation

**Goal:** Dry-run plans, conflict detection, and wiring rules to real-time, scheduled and manual execution.
**FRs:** FR-080–087, FR-130–134, FR-006–008

| ID     | Task                                                                                                      | Tests first                 | Status |
| ------ | --------------------------------------------------------------------------------------------------------- | --------------------------- | ------ |
| P5-T01 | Planner: produces immutable `ExecutionPlan` from a `ReadOnlyFs` (type-level: cannot mutate)               | R-PLAN-01..05               | ✅     |
| P5-T02 | Plan conflict detection: duplicate destinations, permissions, unavailable dest, rule conflicts            | R-PLAN-06..10               | ✅     |
| P5-T03 | Plan executor with per-item revalidation (identity, size, mtime, dest)                                    | R-EXEC-01..05               | ✅     |
| P5-T04 | Pipeline: watcher → stability → evaluate → execute, bounded with backpressure                             | R-PIPE-01..04 (integration) | ✅     |
| P5-T05 | Scheduler: intervals, daily/weekly, quiet hours, days of week, next-run calc, catch-up policy, no overlap | R-SCHED-01..08              | ✅     |
| P5-T06 | Preview screen (filters, search, deselect, execute w/ confirm, ⌘↩)                                        | UI-PREV-01..04              | ✅     |
| P5-T07 | Scheduler screen                                                                                          | UI-SCHED-01..02             | ✅     |
| P5-T08 | Onboarding flow (6 steps) using scanner + planner, zero writes                                            | UI-ONB-01..04, R-ONB-01     | ✅     |

**Exit criteria:** dry-run zero-write test passes over 10k-file fixture (filesystem snapshot before/after identical); stale-plan test passes; scheduler time math covered incl. DST. Audit-5 recorded. ✅ Met — see [audits/phase-5.md](audits/phase-5.md).

**Phase 5 notes**

- `AutomationPipeline` subscribes to `MonitorService` via `CompositeObserver` (`src-tauri/src/events.rs`), executes rules against stabilized files in bounded queues with overflow dirty-folder reconciliation, and honors both folder/rule schedule bindings and `Realtime` quiet hours (`BUG-023`).
- `SchedulerService` persists schedules in SQLite (`repo_schedules`), calculates timezone/DST-safe next-run timestamps (`FixedClock`/`SystemClock`), respects quiet hours and bounded catch-up policies (`Skip`/`RunOnce`), guards concurrent runs with per-scope `try_lock`, and scans via shared `collect_folder_candidates` (`FolderScanSpec`, `BUG-022`).
- `OnboardingService` persists progress in `settings` (`ONBOARDING_SETTINGS_KEY`), runs non-mutating scans using starter presets, and keeps starter-rule enablement unchecked until explicit confirmation (`R-ONB-01`, `UI-ONB-04`).

---

## Phase 6 — Advanced features

**FRs:** FR-110–124, FR-140–141, FR-170, FR-190–192, FR-200–202

| ID     | Task                                                                                                                | Tests first                                        | Status |
| ------ | ------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- | ------ |
| P6-T01 | Streaming BLAKE3 hasher + hash cache with invalidation                                                              | R-HASH-01..04                                      | ✅     |
| P6-T02 | Duplicate finder: size prefilter → partial hash → full hash                                                         | R-DUP-01..06, BUG-026, BUG-028..029                | ✅     |
| P6-T03 | Duplicate actions (keep, move to review, trash w/ confirm, ignore, export)                                          | R-DUP-07..10, BUG-030                              | ✅     |
| P6-T04 | Smart organizers as rule templates (screenshot patterns, invoice keywords, downloads, old files)                    | R-SMART-01..04                                     | ✅     |
| P6-T05 | Import/export (versioned JSON, validation, diff, imported rules disabled)                                           | R-IO-01..06, BUG-031                               | ✅     |
| P6-T06 | Notification service with batching + throttling + per-type prefs                                                    | R-NOTIF-01..04, BUG-032                            | ✅     |
| P6-T07 | Dashboard queries + screen (all real stats)                                                                         | R-STATS-01..03, UI-DASH-01..02                     | ✅     |
| P6-T08 | Duplicates screen + Categories screen (+ Settings Monitoring/Organization/Performance/Notifications/Privacy/Backup) | UI-DUP-01..03, UI-CAT-01..02, UI-SMART-01, BUG-033 | ✅     |

**Exit criteria:** hashing never exceeds fixed buffer (memory test on 2 GB sparse file); dashboard has zero hardcoded values. Audit-6 recorded in [audits/phase-6.md](audits/phase-6.md). ✅ Met — see [audits/phase-6.md](audits/phase-6.md).

---

## Phase 7 — macOS integration

**FRs:** FR-021–023, FR-132, FR-150–152, FR-210–215, FR-220

| ID     | Task                                                                                                                                                                                                          | Tests first                     | Status                                             |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- | -------------------------------------------------- |
| P7-T01 | Menu bar tray with live status + all items; app keeps running when window closes                                                                                                                              | R-TRAY-01..02, edge_07          | ✅                                                 |
| P7-T02 | Launch at login (`SMAppService`, macOS 13+)                                                                                                                                                                   | R-LOGIN-01, `settings.test.tsx` | ✅                                                 |
| P7-T03 | Sleep/wake + volume mount/unmount observers (`ff-macos-events` `NSWorkspace` bridge) → reconcile                                                                                                              | R-SYS-01..03, edge_07           | ✅                                                 |
| P7-T04 | Native notifications wired to notification service (`UNUserNotificationCenter`)                                                                                                                               | R-NOTIF-05                      | ✅                                                 |
| P7-T05 | Keychain storage for license key                                                                                                                                                                              | R-KEY-01..02                    | ✅                                                 |
| P7-T06 | License: offline Ed25519-signed key & `.fflicense` validation, provider-agnostic activation/deactivation (`Q1` online vendor deferred behind `UnconfiguredOnlineProvider` + tested `HttpJsonLicenseProvider`) | R-LIC-01..06, BUG-034           | ✅ (offline complete; `Q1` online vendor deferred) |
| P7-T07 | Keyboard shortcuts per FR-220 (no ⌘Z hijack inside inputs)                                                                                                                                                    | UI-KEYS-01..02                  | ✅                                                 |
| P7-T08 | License screen & Sidebar/Tray navigation                                                                                                                                                                      | UI-LIC-01 (`license.test.tsx`)  | ✅                                                 |

**Exit criteria:** close window → monitoring continues → tray pause/resume works; sleep/wake triggers reconciliation; app works fully offline with license server unreachable. Audit-7 recorded in [audits/phase-7.md](audits/phase-7.md). ✅ Met — see [audits/phase-7.md](audits/phase-7.md) (online provider `Q1` remains behind `UnconfiguredOnlineProvider` until chosen).

---

## Phase 8 — Quality assurance

| ID     | Task                                                                                                                      | Tests                                                                                                  | Status |
| ------ | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ------ |
| P8-T01 | Fill coverage gaps (Rust ≥ 85% lines on `engine/`, `rules/`, `safe_fs` ≥ 95%)                                             | `cargo llvm-cov` (`engine/` 92.90%, `rules/` 95.37%, `safe_fs/` 95.07%)                                | ✅     |
| P8-T02 | Failure suite from spec §24.3 complete                                                                                    | R-FAIL-01..12 (`failure_suite.rs`, 12/12)                                                              | ✅     |
| P8-T03 | Playwright E2E for key flows (onboarding, folder manage, rule create, preview+execute, undo/review/CSV, settings/license) | E2E-01..06 (`tests/e2e/app.spec.ts`)                                                                   | ✅     |
| P8-T04 | Criterion benchmarks 1k/10k/50k/100k; record in `docs/BENCHMARKS.md`                                                      | BENCH-01..07 (`folderflow_benches`, `bench_idle_monitor`)                                              | ✅     |
| P8-T05 | Security audit (capabilities, CSP, path authority, `cargo audit`, `cargo deny`)                                           | `capabilities/default.json`, `tauri.conf.json`, `cargo deny check`, `cargo audit`, `pnpm audit --prod` | ✅     |
| P8-T06 | Accessibility audit (keyboard, VoiceOver/ARIA labels, WCAG 2.1 AA contrast & axe-core)                                    | `UI-A11Y-01`, `categories.test.tsx`, `tests/e2e/app.spec.ts` (`@axe-core/playwright`)                  | ✅     |
| P8-T07 | Full regression re-audit of phases 1–8                                                                                    | `BUG-035`, `BUG-036`, `PROPTEST_CASES=10000`, 10× flake, [audits/phase-8.md](audits/phase-8.md)        | ✅     |

**Exit criteria:** coverage targets exceeded, 12/12 failure suite green, E2E & WCAG 2.1 AA 0 violations across all routes in Light & Dark modes, empirical `1k`–`100k` benchmarks recorded in `docs/BENCHMARKS.md`, security & supply-chain audits clean. Audit-8 recorded in [audits/phase-8.md](audits/phase-8.md). ✅ Met — see [audits/phase-8.md](audits/phase-8.md).

---

## Phase 9 — Production release

| ID  | Task | Tests first | Status |
| --- | ---- | ----------- | ------ |

## Final acceptance (spec §30)

| #   | Criterion                  | Verified by                                                                                                    | Status |
| --- | -------------------------- | -------------------------------------------------------------------------------------------------------------- | ------ |
| A01 | Multiple monitored folders | R-FOLD, E2E-01, E2E-02                                                                                         | ✅     |
| A02 | Background monitoring      | R-TRAY-01..02, `bench_idle_monitor`                                                                            | ✅     |
| A03 | Survives restart           | R-RECON-02, R-RECOV, R-FAIL-01..03                                                                             | ✅     |
| A04 | Correct classification     | R-CLS                                                                                                          | ✅     |
| A05 | Custom rules               | R-RULE, UI-RB, E2E-03                                                                                          | ✅     |
| A06 | Nested AND/OR              | R-GRP (+proptest 10k)                                                                                          | ✅     |
| A07 | Dynamic templates          | R-TPL (+proptest 10k)                                                                                          | ✅     |
| A08 | Preview accurate           | R-PLAN, R-EXEC, E2E-04                                                                                         | ✅     |
| A09 | Dry-run never modifies     | R-PLAN-01, R-PLAN-11 snapshot test, E2E-01                                                                     | ✅     |
| A10 | Safe move/copy/rename      | R-MOVE, R-COPY, R-REN                                                                                          | ✅     |
| A11 | No silent overwrites       | R-CONF, R-SFS, R-FAIL-06                                                                                       | ✅     |
| A12 | Undo works                 | R-UNDO, E2E-05                                                                                                 | ✅     |
| A13 | Interrupted ops recover    | R-RECOV, R-FAIL-01..03                                                                                         | ✅     |
| A14 | Hash-based duplicates      | R-DUP, R-HASH                                                                                                  | ✅     |
| A15 | Scheduled automation       | R-SCHED                                                                                                        | ✅     |
| A16 | Menu bar                   | R-TRAY                                                                                                         | ✅     |
| A17 | Fully offline              | R-SEC, R-LIC-01..06, E2E-06                                                                                    | ✅     |
| A18 | Migrations work            | R-DB                                                                                                           | ✅     |
| A19 | Failures logged + surfaced | R-ERR, R-LOG, R-FAIL-03..10                                                                                    | ✅     |
| A20 | All tests pass             | `pnpm gates && pnpm test:e2e`, `cargo test --workspace` (336 Rust + 109 Vitest + 5 Playwright, 10× flake pass) | ✅     |
| A21 | Benchmarks recorded        | [BENCHMARKS.md](BENCHMARKS.md) (`BENCH-01`..`07`)                                                              | ✅     |
| A22 | Signed + notarized         | P9 (Q3)                                                                                                        | ⬜     |
