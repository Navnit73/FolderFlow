# Bug Log

Every bug found in development, audit or testing goes here **before** it is fixed. Each fix must include a regression test (or an automated gate) whose name contains the bug ID (e.g. `fn bug_007_conflict_rename_race()`).

Severity: S0 data loss/overwrite/escape · S1 wrong behavior/crash/security · S2 degraded · S3 cosmetic. See [AUDIT_PROCESS.md](AUDIT_PROCESS.md).

| ID | Phase found | Sev | Summary | Regression guard | Fixed in | Status |
|---|---|---|---|---|---|---|
| BUG-001 | 1 | S1 | Planned bundle identifier `com.folderflow.app` ends in `.app`, which conflicts with the macOS bundle extension. | `tauri.conf.json` identifier = `com.folderflow.desktop`; docs corrected (ADR-009) | P1 | ✅ Fixed |
| BUG-002 | 1 | S1 | `rust-version = 1.90` made Cargo's MSRV-aware resolver pick sqlx 0.8.6 → bundled SQLite with RUSTSEC-2026-0459 plus an `rsa` advisory (RUSTSEC-2023-0071) in the lockfile. | `cargo audit` gate in CI; MSRV 1.94 + sqlx 0.9 (ADR-009). Remaining SQLite advisory has a documented exception (ADR-008). | P1 | ✅ Fixed (1 documented exception) |
| BUG-003 | 1 | S2 | About section created a component during render and called setState synchronously inside an effect (remounts every render; cascading renders). | `react-hooks/static-components` + `react-hooks/set-state-in-effect` lint rules (lint gate); About retry test | P1 | ✅ Fixed |
| BUG-004 | 1 | S2 | Settings store: when saves complete out of order (earlier save fails after a later save succeeds), the UI rolled back to a stale local state that disagreed with the database. | `src/stores/settings.test.ts` › "BUG-004: a failed save that resolves after a later successful save…" | P1 | ✅ Fixed |
| BUG-005 | 1 | S3 | Animation durations used Tailwind's built-in `duration-150` instead of theme tokens; `--animate-duration-fast` mapping in theme.css was a no-op. | Components use `duration-(--ff-duration-base)`; DESIGN_SYSTEM §6 documents it | P1 | ✅ Fixed |
| BUG-006 | 1 | S3 | `clippy.toml` banned `tokio::process::Command::new`, which is unreachable without tokio's `process` feature → clippy config warning. | `allow-invalid = true` keeps the ban active if the feature is ever enabled | P1 | ✅ Fixed |
| BUG-007 | 1 | S2 | Default log filter enabled only `folderflow_lib`, so every info event logged with a `folderflow::…` target (startup, settings, commands) was silently dropped; log file stayed empty. Found by launching the real app. | `logging::tests::bug_007_default_filter_keeps_folderflow_targets_and_drops_noise` | P1 | ✅ Fixed |
| BUG-008 | 2 | S1 | An incomplete scan (a subfolder briefly unreadable) deleted that subfolder's records; when readable again its old files were reported as new — Phase 5 would have organized pre-existing files. | `reconcile::tests::bug_008_scan_errors_do_not_delete_records_or_resurface_old_files` | P2 | ✅ Fixed |
| BUG-009 | 2 | S1 | Changing a folder's scope (Include subfolders, include/exclude extensions, excluded subfolders) made every file entering scope look new. | `folders::tests::bug_009_scope_changes_reset_the_scan_baseline` | P2 | ✅ Fixed |
| BUG-010 | 2 | S2 | Stopping/removing a folder during a long scan waited 5 s then left the task running; `send` could also block on a full command queue. | `monitor_tests::bug_010_stop_is_bounded_when_the_task_is_busy` | P2 | ✅ Fixed |
| BUG-011 | 2 | S2 | No Info.plist purpose strings: macOS folder-access prompts (Downloads, Desktop, Documents, removable/network volumes) would show no explanation. | `security::config_tests::bug_011_info_plist_explains_every_folder_permission` | P2 | ✅ Fixed |
| BUG-012 | 2 | S3 | Folder detail lower-cased the last-scan text ("13 nov 2026"). | `folders.test.tsx` › "BUG-012: last-scan dates keep their capitalisation" | P2 | ✅ Fixed |
| BUG-013 | 2 | S2 | Scan stamps were wall-clock ms: two scans in the same ms, or a backwards clock jump, left records of deleted files behind. Found as a 1-in-5 flaky test. | `reconcile::tests::bug_013_scan_stamps_increase_even_if_the_clock_goes_backwards` | P2 | ✅ Fixed |

### Environment issues (not product bugs)

| ID | Summary | Workaround |
|---|---|---|
| ENV-001 | On the dev Mac, Command Line Tools 26.6's linker cannot read the macOS 27.0 SDK (`tapi error: unknown architecture arm64e.x1-macos`), so every Rust link fails. | Build with `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk`, or update Command Line Tools / install Xcode matching SDK 27. See README → Troubleshooting. |
| ENV-002 | In `tauri dev`, Vite re-optimised a newly imported dependency on first load and reloaded the page, aborting two in-flight IPC calls ("IPC custom protocol failed"). Dev-only. | `optimizeDeps.include` lists the Tauri API modules in `vite.config.ts`. |
