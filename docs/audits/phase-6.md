# Phase 6 Audit — Advanced features

**Date:** 2026-11-14
**Scope:** Full Phases 1–6 verification and Phase 6 end-to-end audit covering streaming BLAKE3 hashing & invalidation cache (`duplicates/hasher.rs`, `duplicates/cache.rs`), 3-stage duplicate finder & undoable/safe actions (`duplicates/finder.rs`, `duplicates/actions.rs`), Smart Organizer templates (`rules/smart.rs`), versioned config export/import (`config_io/{export,import,schema}.rs`), batched/throttled notification service (`notifications/{mod,service}.rs`), real SQLite dashboard metrics & DB maintenance (`database/repo_stats.rs`, `commands/dashboard.rs`), custom categories CRUD (`categories/mod.rs`, `commands/categories.rs`), global exclusions (`database/repo_exclusions.rs`, `commands/exclusions.rs`), and frontend Overview, Duplicates, Categories, Rules Smart Organizer menu, and Settings (Monitoring, Organization, Performance, Notifications, Privacy, Backup) screens (`P6-T01`..`P6-T08`).

---

## 1. Gate results (final run after all fixes)

| Gate | Command | Result |
|---|---|---|
| Lint | `pnpm lint` (`eslint . --max-warnings 0`) | ✅ 0 errors, 0 warnings |
| Typecheck | `pnpm typecheck` (`tsc --noEmit -p tsconfig.json`) | ✅ 0 errors |
| Frontend tests | `pnpm test` (`vitest run`) | ✅ 17 test files, 98 tests passed |
| Build | `pnpm build` (`vite build`) | ✅ Clean production bundle |
| Token enforcement | `pnpm check:tokens` | ✅ Passed (all UI styling from `src/styles/theme.css`) |
| Rust format | `cargo fmt --check` | ✅ Diff clean |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings (`unwrap_used`, `disallowed_methods`, `cloned_ref_to_slice_refs`, `ptr_arg` clean) |
| Rust tests (+proptest 10k) | `PROPTEST_CASES=10000 cargo test` | ✅ 299 passed; 0 failed (0 ignored) |
| Bindings staleness | `r_cmd_01_bindings_are_up_to_date` | ✅ `src/bindings.ts` matches current commands |
| Flake check (10×) | `for i in $(seq 1 10); do cargo test --quiet; done` | ✅ 10/10 runs passed (299 passed each run, 5.84s–12.75s/run) |
| Security audit (Rust & npm) | `cargo audit` (515 crates) + `pnpm audit --prod` | ✅ 0 vulnerabilities found (`cargo audit`: 3 upstream allowed warnings `paste`, `proc-macro-error`, `glib`; `pnpm audit`: no known vulnerabilities) |

---

## 2. Requirement coverage

| Req / Task | Description | Verified by |
|---|---|---|
| `P6-T01` (`R-HASH-01..04`) | Streaming 64 KiB BLAKE3 hasher, 3-window partial hash, 2 GB sparse file bounded-buffer test, `(dev, ino, size, modified_ns)` cache invalidation | `r_hash_01`..`r_hash_04` |
| `P6-T02` (`R-DUP-01..06`, `BUG-026..029`) | Size prefilter, partial hash elimination, full hash grouping, cross-folder same-content detection, copy count / wasted bytes / ignore groups, zero FS writes & symlink/package skip, global/per-folder DB exclusion enforcement, live `ContentHash` / `DuplicateStatus` / `hash_on_scan` hydration, scoped group replacement, Unicode/emoji/leading-dot/255B edge cases | `r_dup_01`..`r_dup_06`, `bug_026_*`, `bug_027_*`, `bug_028_*`, `bug_029_*`, `edge_06_duplicate_scan_*` |
| `P6-T03` (`R-DUP-07..10`, `BUG-030`) | Keeper selection, undoable Move to Review (with global auto-recorded path exclusion), Trash requiring confirmation & never trashing all copies + hash check, CSV/JSON export w/ formula/RFC-4180 escaping & `PathAuthority` reveal guard | `r_dup_07`..`r_dup_10`, `bug_030_*`, `edge_06_export_csv_*` |
| `P6-T04` (`R-SMART-01..04`) | Pattern-customizable Screenshot Organizer, strictly filename-based Invoice/Receipt Organizer w/ custom keywords, Clean Downloads per-category presets, Old Files age thresholds (never auto-deletes, installed disabled), Unicode/double-ext/255B edge cases | `r_smart_01`..`r_smart_04`, `edge_06_smart_rules_*`, `UI-SMART-01` |
| `P6-T05` (`R-IO-01..06`, `BUG-031`) | Versioned config export/import round-trip, schema version rejection, path traversal/protected root validation, imported rules forced disabled (`RuleOrigin::Import`), rule `source_folders` UUID↔path remapping, `'other'` & built-in collision protection, non-mutating diff preview (`Merge` & `Replace`), transactional merge/replace apply w/ orphan `duplicate_groups` cleanup | `r_io_01`..`r_io_06`, `bug_031_*` |
| `P6-T06` (`R-NOTIF-01..04`, `BUG-032`) | Per-type & master notification preferences, time-window batching, cooldown throttling, path redaction, bounded history & read/clear, runtime `DestinationUnavailable` / `PermissionIssue` notifications via `notify_plan_outcome` | `r_notif_01`..`r_notif_04`, `bug_032_*` |
| `P6-T07` (`R-STATS-01..03`, `UI-DASH-01..02`) | Zero-hardcoding empty DB metrics, populated today/week/all-time/undone/duplicate/trend queries, `ANALYZE`/`VACUUM` maintenance (`db_maintenance`), Overview dashboard UI | `r_stats_01`..`r_stats_03`, `UI-DASH-01`..`UI-DASH-02` (`src/pages/overview/overview.test.tsx`) |
| `P6-T08` (`UI-DUP-01..03`, `UI-CAT-01..02`, `r_cat_04`, `BUG-033`) | Duplicates workflow UI (+ full scan button & empty-state CTA), custom categories CRUD/validation/reorder (`r_cat_04`) & live classifier resolution, Settings tabs (Monitoring, Organization, Performance, Notifications w/ live events & mark-all-read, Privacy w/ Exclusions, Backup w/ Replace preview & 5 MiB guard) | `r_cat_04`, `edge_06_custom_category_*`, `UI-DUP-01`..`UI-DUP-03` (`src/pages/duplicates/duplicates.test.tsx`, 3 tests), `UI-CAT-01`..`UI-CAT-02` (`src/pages/categories/categories.test.tsx`, 3 tests), `src/pages/settings/settings.test.tsx` (9 tests) |

---

## 3. Checklist results

### A. Data safety (`SAFETY_MODEL.md` S1–S11)
- [x] `duplicates::finder::scan_duplicates` performs zero filesystem writes, skips symlinks and macOS bundles/packages, and never trashes all copies of a duplicate group (`r_dup_06`, `r_dup_09`, which also requires explicit confirmation and verifies content hash match before trashing).
- [x] Smart Organizer presets (`rules/smart.rs`) never delete files (Old Files moves to an Archive subfolder rather than trashing) and always install `enabled = false`.
- [x] Config import (`config_io/import.rs`) rejects unknown/malformed schema versions, relative/traversal/NUL/forbidden paths via `PathAuthority` / `RootPolicy`, and forces all imported rules disabled with `RuleOrigin::Import` (`r_io_02`..`r_io_04`).
- [x] `BUG-026`: Global and per-folder DB exclusions (`Path`, `Glob`, `Extension`, including auto-recorded `Duplicate Review` exclusions from `dup_action`) are uniformly enforced across `IgnoreRules::check_file`/`check_dir`/`is_path_excluded`, `FolderScanSpec::load_for_folder`, `duplicates::finder`, `commands/preview`, `engine/scheduler`, `engine/monitor` (`refresh_exclusions`), `engine/pipeline`, and `onboarding`.

### B. Correctness
- [x] Streaming hasher (`duplicates/hasher.rs`) matches canonical BLAKE3 test vectors across boundary sizes and streams a 2 GB sparse file inside a fixed 64 KiB buffer (`r_hash_01`..`r_hash_03`); SQLite hash cache invalidates on `dev`, `ino`, `size`, or `mtime_ns` changes (`r_hash_04`).
- [x] `BUG-027`: `hydrate_candidates_for_rules` populates `PlanCandidate.content_hash` and `PlanCandidate.is_duplicate` across `commands/preview`, `engine/scheduler`, and `engine/pipeline::process_file` whenever active rules use `Condition::ContentHash` or `Condition::DuplicateStatus`, or when `PerformanceSettings.hash_on_scan` is enabled — performing zero DB queries or disk reads when neither applies (`r_plan_11_dry_run_over_10k_files_performs_zero_writes` unchanged).
- [x] `NotificationService` respects master and per-event preferences, batches burst notifications inside configured time windows, enforces cooldown throttling, redacts paths, and bounds retained history (`r_notif_01`..`r_notif_04`).
- [x] `repo_stats` returns exact SQLite aggregations (`0` on empty DB without mock fallbacks, excludes undone/reversed operations, and executes `ANALYZE`/`VACUUM` safely) (`r_stats_01`..`r_stats_03`).

### C. Concurrency & resources
- [x] `hydrate_candidates_for_rules` offloads `RealFs.stat` and streaming BLAKE3 hashing onto `tokio::task::spawn_blocking` and uses size prefiltering when only `DuplicateStatus` is inspected so unique-size files are never hashed.
- [x] `dup_scan`, `plan_create`, `scheduler`, `pipeline` reconcile, and `onboarding::preview` run directory scans off Tokio's async command threads via `spawn_blocking`.

### D. Security
- [x] Zero XSS sinks (`dangerouslySetInnerHTML`, `eval(`) present in `src/`; config import re-validates every folder and rule destination path against `RootPolicy` before applying (`r_io_03`).
- [x] `cargo audit` (515 dependencies) reported **0 vulnerabilities** (`3` pre-existing upstream unmaintained/unsound allowed warnings on transitive crates `paste`, `proc-macro-error`, and `glib`).

### E. UI / theme
- [x] `OverviewPage`, `DuplicatesPage`, `CategoriesPage`, `SmartOrganizerDialog`, and all `SettingsPage` sections (`Monitoring`, `Organization`, `Performance`, `Notifications`, `Privacy`, `Backup`) pass `pnpm check:tokens` with zero hardcoded design values outside `src/styles/theme.css`.
- [x] Navigation items in `src/app/navigation.ts` enable `overview` (default `/` landing route), `duplicates`, and `categories` while keeping unimplemented `/license` gated safely.

### F. Docs
- [x] `docs/BUGS.md`, `docs/PLAN_TRACKER.md`, `docs/ARCHITECTURE.md`, `docs/CHANGELOG.md`, `README.md`, and `docs/audits/phase-6.md` synchronized with verified code, routes, commands, test counts (`299` Rust · `98` TS), and audit results.

---

## 4. Bugs found and fixed during audit

| ID | Severity | Area | Summary | Regression test |
|---|---|---|---|---|
| `BUG-026` | S1 | `engine/{ignore,reconcile,monitor,pipeline,scheduler}.rs`, `duplicates/finder.rs`, `commands/{exclusions,duplicates,preview}.rs`, `onboarding/mod.rs`, `database/repo_exclusions.rs` | Global and per-folder DB exclusions (`Path`, `Glob`, `Extension`, including auto-recorded `Duplicate Review` folder exclusions) were omitted from `FolderScanSpec` across scans, `duplicates::finder` loaded only `'path'` rows, `IgnoreRules::check_file` missed path/glob exclusions, and `pipeline::process_file` never checked DB exclusions. Unified via `FolderScanSpec::load_for_folder`, `IgnoreRules::apply_db_exclusions`/`is_path_excluded`, and live `FolderMonitor::refresh_exclusions`. | `bug_026_duplicate_scan_honors_global_glob_and_extension_exclusions`, `bug_026_exclusions_glob_extension_and_review_path_respected_across_scans` |
| `BUG-027` | S1 | `rules/models.rs`, `duplicates/cache.rs`, `database/repo_files.rs`, `commands/preview.rs`, `engine/{scheduler,pipeline,planner}.rs` | Rules evaluating `Condition::ContentHash` or `Condition::DuplicateStatus` received hardcoded `content_hash: None, is_duplicate: false` across preview, scheduler, and live pipeline, and `PerformanceSettings.hash_on_scan` was unused during scans. Added `rules_hash_requirements`/`scope_rules`, `repo_files::list_tracked_by_sizes`, and `hydrate_candidates_for_rules` with O(1) skip when unused. | `bug_027_preview_and_pipeline_evaluate_content_hash_and_duplicate_status_rules`, `bug_027_pipeline_evaluates_content_hash_duplicate_status_and_hash_on_scan` |
| `BUG-028` | S2 | `duplicates/finder.rs`, `commands/duplicates.rs` | `dup_scan` subtracted `active_groups` from `totals.total_duplicate_copies` (already `(copy_count - 1)` extra-copy sum) and used `totals.total_groups` instead of `totals.active_groups`, causing ignored-only scans to report non-zero `groupsFound` and fire notifications. Extracted `dup_notification_counts` and fixed `report.groups_found`. | `bug_028_ignored_only_scan_reports_zero_active_groups`, `bug_028_dup_scan_notification_counts_extra_copies_and_skips_ignored_only` |
| `BUG-029` | S1 | `duplicates/finder.rs`, `database/repo_duplicates.rs` | Scoped `scan_duplicates` (`folder_ids = Some(...)`) and empty `folder_ids = Some([])` executed unconditional `DELETE FROM duplicate_groups`, wiping groups in unscanned folders; `load_all_groups` also ran synchronous `symlink_metadata()` on async threads. Scoped `replace_groups_from_scan` + `spawn_blocking` metadata filtering. | `bug_029_scoped_scan_preserves_unscanned_groups_and_handles_empty_folder_ids` |
| `BUG-030` | S1 | `duplicates/actions.rs` | `MoveToReview` recorded a folder-scoped exclusion for the `Duplicate Review` folder instead of a global exclusion (`folder_id = NULL`), leaving cross-folder review dirs exposed to other monitored folders. Promoted `Duplicate Review` exclusions to global scope (upgrading existing scoped rows). | `bug_030_cross_folder_review_exclusion_protects_all_monitored_folders` |
| `BUG-031` | S1 | `config_io/{export,import,mod}.rs`, `commands/config_io.rs`, `src/services/configIo.ts`, `src/bindings.ts` | Config export/import dropped rule `source_folders` across new/replaced UUIDs, widened unresolved per-folder exclusions to global, didn't protect `'other'` category or reject built-in/custom & duplicate-bundle collisions, left orphan `duplicate_groups` in `Replace` mode, skipped `ApplyAnotherRule` cycle validation, and omitted `mode` from `config_import_preview`. | `bug_031_per_folder_exclusion_never_widens_to_global_and_source_folders_remap`, `bug_031_category_other_protected_builtin_collision_and_bundle_duplicates_rejected`, `bug_031_replace_mode_clears_orphan_duplicate_groups`, `bug_031_preview_replace_mode_and_bundle_duplicate_exclusions_or_rules_rejected` |
| `BUG-032` | S1 | `notifications/service.rs`, `engine/{pipeline,scheduler}.rs`, `commands/preview.rs` | `NotificationKind::DestinationUnavailable` (`FR-200`) and planning-time permission issues were never emitted at runtime because only executed items were inspected. Added `NotificationService::notify_plan_outcome` inspecting `StaleReason::BlockedByConflict` (`DestinationUnavailable` / `PermissionDenied`) and wired across `pipeline`, `scheduler`, and `plan_execute`. | `bug_032_plan_outcome_emits_destination_unavailable_and_dedups_per_root`, `bug_032_unmounted_destination_emits_destination_unavailable_notification` |
| `BUG-033` | S2 | `src/pages/{settings,categories,rules,duplicates}/*`, `src/stores/rules.ts` | Fixed unhandled rejections across `PerformanceSection`, `NotificationsSection`, `CategoriesPage`, `OrganizationSection`, `SmartOrganizerDialog`; added Escape closing & failed-save error retention in `SmartOrganizerDialog`, live `appNotificationEvent` + `"Mark all read"` in `NotificationsSection`, and `Replace`-mode preview / 5 MiB size guard / store refreshes in `BackupSection`; added `UI-DUP-03` and `UI-CAT-02`. | `settings.test.tsx`, `categories.test.tsx`, `rules.test.tsx`, `duplicates.test.tsx` |

Additionally, pre-audit cleanup resolved React 19 `react-hooks/set-state-in-effect` lint violations in Settings sections, aligned generated binding type imports in `src/types/settings.ts` and duplicate store/service wrappers, and resolved `clippy::cloned-ref-to-slice-refs` lints in `src-tauri/src/rules/smart.rs` and test helpers.

---

## 5. Process record

1. Completed Phase 6 frontend route wiring (`/overview`, `/duplicates`, `/categories`), `SmartOrganizerDialog`, Settings sections (`Monitoring`, `Organization`, `Performance`, `Notifications`, `Privacy`, `Backup`), Vitest test suites (`overview`, `duplicates`, `categories`, `rules` Smart Organizer, `settings`), and pre-audit lint/clippy/test gates.
2. Executed full Phases 1–6 audit (`docs/AUDIT_PROCESS.md` Checklists A–F) across two independent passes (`BUG-026`–`BUG-027`, followed by `BUG-028`–`BUG-033`, `edge_06_*`, `UI-DUP-03`, and `UI-CAT-02`) in `docs/BUGS.md`.
3. Wrote RED failing regression tests prior to each fix and confirmed failure modes matched the audited root causes.
4. Implemented `BUG-026`–`BUG-033` fixes across backend services, database repositories, commands, `tauri-specta` bindings, and frontend React/Zustand views.
5. Verified `pnpm bindings` (`r_cmd_01_bindings_are_up_to_date`), `pnpm audit --prod` (no vulnerabilities), `pnpm gates` (`eslint --max-warnings 0`, `tsc --noEmit`, `vitest run` — 17 files / 98 tests, `vite build`, `check:tokens`), `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `PROPTEST_CASES=10000 cargo test` (299 passed), 10× consecutive flake run (299 passed, 10/10), and `cargo audit` (0 vulnerabilities), then synchronized all project documentation.

---

## 6. Deferred / owner actions

1. **Upstream crate advisories (3 allowed warnings)**: `paste` (`RUSTSEC-2024-0436`, unmaintained), `proc-macro-error` (`RUSTSEC-2024-0370`, unmaintained), and `glib` (`RUSTSEC-2024-0429`, unsound transitive dependency in Tauri GTK/macOS stack) remain monitored across upstream Tauri dependency updates.
2. **macOS desktop UI & system integration (Phase 7)**: native tray menu, login-item launch at startup, sleep/wake rescan hooks, and external volume mount/unmount handling remain scheduled for Phase 7 (`P7-T01`..`P7-T04`).

---

## 7. Sign-off

- **Phase 6 status:** ✅ Complete (`P6-T01`..`P6-T08`)
- **Next phase:** Phase 7 — macOS integration (`P7-T01`: Menu bar tray + quick actions)

