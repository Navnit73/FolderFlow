# Phase 5 Audit — Preview & automation

**Date:** 2026-11-14
**Scope:** Full Phases 1–5 verification and Phase 5 end-to-end audit covering dry-run planner (`engine/planner.rs`), real-time automation pipeline (`engine/pipeline.rs`), cron/interval/DST/quiet-hours scheduler (`engine/scheduler.rs`, `database/repo_schedules.rs`), onboarding wizard (`onboarding/mod.rs`), wiring in `app_state.rs`/`events.rs`/`commands/*`, and frontend Preview, Scheduler, Onboarding, and Settings → General UI (`P5-T01`..`P5-T08`).

---

## 1. Gate results (final run after all fixes)

| Gate | Command | Result |
|---|---|---|
| Lint | `pnpm lint` (`eslint . --max-warnings 0`) | ✅ 0 errors, 0 warnings |
| Typecheck | `pnpm typecheck` (`tsc --noEmit -p tsconfig.json`) | ✅ 0 errors |
| Frontend tests | `pnpm test` (`vitest run`) | ✅ 13 test files, 79 tests passed |
| Build | `pnpm build` (`vite build`) | ✅ Clean production bundle |
| Token enforcement | `pnpm check:tokens` | ✅ Passed (all UI styling from `src/styles/theme.css`) |
| Rust format | `cargo fmt --check` | ✅ Diff clean |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings (`unwrap_used`, `disallowed_methods`, `too_many_arguments`, `ptr_arg` clean) |
| Rust tests (+proptest 10k) | `PROPTEST_CASES=10000 cargo test` | ✅ 249 passed; 0 failed (0 ignored) |
| Bindings staleness | `r_cmd_01_bindings_are_up_to_date` | ✅ `src/bindings.ts` matches current commands |
| Flake check (10×) | `for i in $(seq 1 10); do cargo test --quiet; done` | ✅ 10/10 runs passed (249 passed each run, ~4.85s/run) |
| Security audit | `cargo audit` (`src-tauri/Cargo.lock`, 515 crates) | ⚠️ 1 upstream vulnerability: `RUSTSEC-2026-0459` in `libsqlite3-sys v0.37.0` (pinned by `sqlx-sqlite v0.9.0`, no newer `sqlx` release on crates.io yet); 3 upstream warnings (`paste`, `proc-macro-error`, `glib`) |

---

## 2. Requirement coverage

| Req / Task | Description | Verified by |
|---|---|---|
| `P5-T01` (`R-PLAN-01..05`) | Read-only dry-run planner (`&dyn ReadOnlyFs`), scope filters, summary totals, 10k-file zero-write guarantee | `r_plan_01`..`r_plan_05`, `r_plan_11_dry_run_over_10k_files_performs_zero_writes` |
| `P5-T02` (`R-PLAN-06..10`) | Disk collision, intra-plan duplicate targets, `PathAuthority` & ancestor `PermissionDenied`, unavailable volume / space, rule conflicts | `r_plan_06`..`r_plan_10`, `bug_024_plan_detects_ancestor_permission_denied`, `bug_024_plan_ignores_disabled_folders_and_prevents_double_execute` |
| `P5-T03` (`R-EXEC-01..05`) | Per-item execution selection and pre-execution stale revalidation (inode/dev, size/mtime, missing source, late destination collision) | `r_exec_01`..`r_exec_05`, `bug_024_plan_ignores_disabled_folders_and_prevents_double_execute` |
| `P5-T04` (`R-PIPE-01..04`) | Bounded watcher → stability → pipeline execution, paused gating, 50k-event overflow backpressure + dirty reconcile, `file_records` sync | `r_pipe_01`..`r_pipe_04`, `bug_023_realtime_pipeline_scopes_folder_schedules_and_enforces_quiet_hours` |
| `P5-T05` (`R-SCHED-01..08`) | Schedule SQLite CRUD, interval/daily/weekly/on-launch/manual scheduling, DST spring-forward/fall-back, quiet hours, catch-up policies, per-scope mutex | `r_sched_01`..`r_sched_08`, `bug_022_scheduled_run_honors_folder_exclusions_and_monitoring_settings` |
| `P5-T06` (`UI-PREV-01..04`) | Preview screen: plan generation, breakdown, filters/search, deselect, conflict badges & disabled blocked items, ⌘↩ confirm + anti-double-execute | `UI-PREV-01`..`UI-PREV-04` (`src/pages/preview/preview.test.tsx`) |
| `P5-T07` (`UI-SCHED-01..02`) | Scheduler screen: list, enable switch, Run now, Delete confirm, create & edit interval/weekly schedules w/ quiet hours & catch-up | `UI-SCHED-01`..`UI-SCHED-02` (`src/pages/scheduler/scheduler.test.tsx`) |
| `P5-T08` (`UI-ONB-01..04`, `R-ONB-01`) | 6-step onboarding flow, zero filesystem writes, TCC permission guidance, relaunch resume, Settings → General re-run & confirmation gate | `r_onb_01_onboarding_flow_performs_zero_filesystem_writes`, `UI-ONB-01`..`UI-ONB-04` (`src/pages/onboarding/onboarding.test.tsx`) |

---

## 3. Checklist results

### A. Data safety (`SAFETY_MODEL.md` S1–S11)
- [x] `build_execution_plan` receives only `&dyn ReadOnlyFs` (`PlanContext`); dry-run over 10,000 files (`r_plan_11_dry_run_over_10k_files_performs_zero_writes`) and full 6-step onboarding scan (`r_onb_01_onboarding_flow_performs_zero_filesystem_writes`) leave disk snapshots byte-for-byte identical.
- [x] Stale plan revalidation checks `dev`/`ino`, size/mtime, source presence, and destination availability right before calling `Organizer` (`r_exec_02`..`r_exec_05`).
- [x] `BUG-024`: unscoped previews ignore disabled folders, nested non-existent targets check deepest reachable ancestor permissions, and executed items are pruned from `PreviewStore` and `usePreview` selection so double-execution (e.g., duplicate copies) cannot occur.
- [x] Onboarding automation switch defaults unchecked and requires explicit user confirmation before enabling starter rules (`UI-ONB-01 & UI-ONB-04`).

### B. Correctness
- [x] Schedule timing math (`compute_next_run_at`, `evaluate_due_schedule`, `is_in_quiet_hours`) handles DST spring-forward skipped hours, fall-back ambiguous hours, overnight quiet windows, weekday filters, and bounded catch-up (`Skip` vs `RunOnce`) (`r_sched_02`..`r_sched_06`).
- [x] `BUG-022`: all candidate scans (`SchedulerService`, `plan_create`, `AutomationPipeline` reconcile, `OnboardingService`) now go through `FolderScanSpec::from_folder` using persisted `MonitoringSettings` (`ignore_hidden_files`, `ignore_temporary_files`) and per-folder extension/subfolder filters.
- [x] `BUG-023`: folder-bound rule schedules `(folder_id = f1, rule_id = r1)` no longer suppress `r1` in unrelated folder `f2`, and `Realtime` schedules respect configured `quiet_hours`.

### C. Concurrency & resources
- [x] `AutomationPipeline` uses a bounded `mpsc` queue (`DEFAULT_PIPELINE_QUEUE_CAPACITY = 4096`) with `try_send` drop-to-dirty reconciliation verified under a 50,000-event flood (`r_pipe_03`).
- [x] `SchedulerService` serializes overlapping runs on the same `(folder_id, rule_id)` scope via `try_lock` (`r_sched_08_per_scope_lock_prevents_overlapping_runs`).
- [x] `BUG-025`: synchronous directory scans in `plan_create`, `scheduler`, `pipeline` reconcile, and `onboarding::preview` are offloaded via `tokio::task::spawn_blocking`.

### D. Security
- [x] All destinations remain confined by `PathAuthority::authorize`; frontend holds zero raw path-manipulation authority (`AppError`/`UiError` redaction unchanged).
- [x] `cargo audit` flagged **1 upstream vulnerability**: `RUSTSEC-2026-0459` (CVE-2026-119375, severity 6.8 medium) on transitive `libsqlite3-sys v0.37.0`, pinned directly by `sqlx-sqlite v0.9.0`. No newer `sqlx` / `sqlx-sqlite` release (`> 0.9.0`) is available on crates.io (`cargo search sqlx` shows `0.9.0`). Documented under Section 6 for dependency monitoring rather than suppressed.

### E. UI / theme
- [x] `PreviewPage`, `SchedulerPage`, `OnboardingPage`, and `SettingsPage` → `GeneralSection` pass `pnpm check:tokens` with zero hardcoded colors/spacing outside `src/styles/theme.css`.
- [x] Navigation items in `src/app/navigation.ts` enable implemented screens (`/folders`, `/preview`, `/rules`, `/activity`, `/scheduler`, `/settings/:section`, `/onboarding/:step`) cleanly while leaving Phase 6/7 screens flagged "Soon".

### F. Docs
- [x] `docs/BUGS.md`, `docs/PLAN_TRACKER.md`, `docs/ARCHITECTURE.md`, `docs/CHANGELOG.md`, `README.md`, and `docs/audits/phase-5.md` synchronized with verified code, routes, commands, test counts (`249` Rust · `79` TS), and audit results.

---

## 4. Bugs found and fixed during audit

| ID | Severity | Area | Summary | Regression test |
|---|---|---|---|---|
| `BUG-022` | S1 | `scheduler.rs`, `commands/preview.rs`, `pipeline.rs`, `onboarding/mod.rs` | `SchedulerService::scan_candidates_for_folder` dropped `folder.settings` extension/subfolder filters and all four scan sites hardcoded `ignore_hidden/temporary = true`. Unified via `FolderScanSpec::from_folder` + persisted `MonitoringSettings`. | `bug_022_scheduled_run_honors_folder_exclusions_and_monitoring_settings` |
| `BUG-023` | S1 | `engine/pipeline.rs` | `process_file` filtered rule schedules without `folder_id` (leaking `(f1, r1)` suppression onto `f2`) and ignored `quiet_hours` on active `Realtime` schedules. | `bug_023_realtime_pipeline_scopes_folder_schedules_and_enforces_quiet_hours` |
| `BUG-024` | S1 | `commands/preview.rs`, `engine/planner.rs`, `src/stores/preview.ts` | Unscoped preview included disabled folders; planner permission check skipped missing subdirs under locked ancestors; executed plan items remained selectable/present enabling accidental double-execution. | `bug_024_plan_detects_ancestor_permission_denied`, `bug_024_plan_ignores_disabled_folders_and_prevents_double_execute`, `UI-PREV-04` |
| `BUG-025` | S2 | `commands/settings.rs`, `commands/preview.rs`, `onboarding/mod.rs`, `engine/{scheduler,pipeline}.rs` | `settings_update` did not push `MonitoringSettings` changes to running `MonitorService`; sync directory scans ran directly on Tokio async command threads instead of `spawn_blocking`. | Build/clippy/test clean + async offload verified across `cargo test` |

Additionally, pre-gate cleanup resolved `clippy::too_many_arguments` on `build_execution_plan` (via `PlanContext`), `clippy::ptr_arg` in `scheduler.rs`, test `.unwrap_used` in `scheduler.rs`, strict TS indexed access in `scheduler.test.tsx`, and formatting diffs via `cargo fmt`.

---

## 5. Process record

1. Ran full build/lint/test suite across Phases 1–5 (`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `pnpm gates`) and audited all Phase 5 Rust and TypeScript code paths.
2. Logged `BUG-022`..`BUG-025` in `docs/BUGS.md` with severity before fixes.
3. Wrote failing RED regression tests for `BUG-022`, `BUG-023`, `BUG-024` (Rust + Vitest), and edit assertions for `UI-SCHED-02`; confirmed all 4 targeted tests failed for the expected bug causes before editing production code.
4. Extracted shared `PlanContext` and `collect_folder_candidates` helpers, implemented minimal fixes across backend and frontend, and ran `cargo test` + `vitest` to confirm all RED tests turned GREEN.
5. Verified full gates, `PROPTEST_CASES=10000`, `10×` consecutive flake run, and `cargo audit` (logging upstream `RUSTSEC-2026-0459`), then updated documentation.

---

## 6. Deferred / owner actions

1. **Upstream `RUSTSEC-2026-0459` (`libsqlite3-sys v0.37.0`)**: monitor `sqlx` / `sqlx-sqlite` releases (`> 0.9.0`) on crates.io (or evaluate unbundled SQLite linking in `P8-T05`) to clear the `cargo audit` advisory once an upstream `sqlx` bump ships.
2. **macOS desktop UI interactive check**: live FSEvents/launch-window confirmation and tray/sleep-wake integration (`Phase 7`) will be exercised end-to-end on desktop builds as Phase 6–8 progress.

---

## 7. Sign-off

- **Phases 1–5 automated suites:** **328 / 328 tests passing (`249` Rust · `79` TypeScript)** across **10/10** consecutive Rust runs and **10,000** proptest cases per property.
- **Safety invariant check:** Zero unapproved writes in planner/onboarding (`R-PLAN-11`, `R-ONB-01`), stale plans revalidated pre-execution (`R-EXEC-02`..`05`), double-execution prevented (`BUG-024`), quiet hours & scope isolation enforced (`BUG-022`, `BUG-023`).
- **Status:** **Phase 5 complete.**

