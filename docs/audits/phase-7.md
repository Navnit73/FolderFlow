# Phase 7 Audit — macOS integration

**Date:** 2026-11-14
**Scope:** Full Phases 1–7 verification and Phase 7 end-to-end audit covering macOS menu bar tray + close-to-tray background keep-alive (`system/tray.rs`, `engine/pipeline.rs` `BusyGuard`), Launch at Login via `SMAppService` (`system/login_item.rs`, `commands/settings.rs`), `NSWorkspace` sleep/wake & external volume mount/unmount observers (`crates/ff-macos-events`, `system/coordinator.rs`, `engine/monitor.rs`), native macOS notifications via `UNUserNotificationCenter` (`notifications/native.rs`), macOS Keychain credential storage (`security/keychain.rs`), offline Ed25519 + `.fflicense` & provider-agnostic online licensing (`security/license.rs`, `commands/license.rs`), desktop keyboard shortcuts (`src/hooks/useGlobalShortcuts.ts`, `search` attributes), and `/license` / `GeneralSection` / `Sidebar` / `AppLayout` UI (`P7-T01`..`P7-T08`, `BUG-034`, `edge_07_*`).

---

## 1. Gate results (final run after all code fixes)

| Gate | Command | Result |
|---|---|---|
| Bindings generation & check | `pnpm bindings` (`UPDATE_BINDINGS=1 cargo test --lib r_cmd_01`) | ✅ `src/bindings.ts` up to date (`r_cmd_01_bindings_are_up_to_date` passed) |
| Lint | `pnpm lint` (`eslint . --max-warnings 0`) | ✅ 0 errors, 0 warnings |
| Typecheck | `pnpm typecheck` (`tsc --noEmit -p tsconfig.json`) | ✅ 0 errors |
| Frontend tests | `pnpm test` (`vitest run`) | ✅ 19 test files, 108 tests passed |
| Build | `pnpm build` (`vite build`) | ✅ Clean production bundle |
| Token enforcement | `pnpm check:tokens` | ✅ Passed (all UI styling from `src/styles/theme.css`) |
| Rust format | `cargo fmt --all -- --check` | ✅ Diff clean |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings (`folderflow` + `ff-macos-events`) |
| Rust tests (+proptest 10k) | `PROPTEST_CASES=10000 cargo test --workspace` | ✅ 317 passed; 0 failed (0 ignored; 27.09s) |
| Flake check (10×) | `for i in $(seq 1 10); do cargo test --workspace --quiet; done` | ✅ 10/10 runs passed (317 passed each run, 6.28s–8.84s/run) |
| Security audit (Rust & npm) | `cargo audit` (571 crates) + `pnpm audit --prod` | ✅ 0 vulnerabilities found (`cargo audit`: 3 pre-existing allowed warnings `paste`, `proc-macro-error`, `glib`, plus documented SQLite `RUSTSEC-2026-0459` per `ADR-008`; `pnpm audit --prod`: no known vulnerabilities) |

---

## 2. Requirement coverage

| Req / Task | Description | Verified by |
|---|---|---|
| `P7-T01` (`FR-021`, `FR-150..152`, `UI-TRAY`) | Menu bar tray live states (`Active`, `Paused`, `Working` via `AutomationPipeline::is_busy()`, `Error`), status summary + Pause All / Resume Monitoring / Organize Now / Preview Pending / Recent Activity / Open FolderFlow / Settings… / Quit FolderFlow, `HideToBackground` on window close vs `ExitApp` on Quit, `RunEvent::Reopen` window reveal, `startMinimized` & `showMenuBarIcon` toggles | `r_tray_01_snapshot_reflects_active_paused_working_error_and_contains_all_required_items`, `r_tray_02_window_close_hides_without_quitting_and_keeps_monitoring_running`, `edge_07_tray_zero_folders_and_close_vs_quit_disposition`, `license.test.tsx` (`trayNavigateEvent`) |
| `P7-T02` (`FR-022`, `FR-023`) | Launch at login via `SMAppService` (`smappservice-rs`), `NSBundle` unbundled test guard, `login_item_status` & `open_login_items_settings` IPC, live `settings_update` sync & approval warning banner | `r_login_01_sync_toggles_smappservice_and_surfaces_errors`, `settings.test.tsx` (`GeneralSection`) |
| `P7-T03` (`FR-132`, `FR-021`) | `NSWorkspace` sleep/wake & mount/unmount observer bridge in isolated leaf crate `ff-macos-events`, `DidWake` fast reconcile & `MonitorService::recheck`, `VolumeUnmounted` → `Unavailable`, `VolumeMounted` → auto-resume & reconcile, sibling mount prefix (`/Backup` vs `/Backup 2`) & disabled folder isolation | `r_sys_01_did_wake_triggers_reconcile_and_detects_missed_files`, `r_sys_02_volume_unmount_marks_external_folder_unavailable_and_mount_resumes_and_reconciles`, `r_sys_03_macos_workspace_observer_starts_and_drops_cleanly`, `edge_07_volume_mount_and_unmount_isolate_sibling_prefix_and_disabled_folders` |
| `P7-T04` (`FR-200..203`) | `MacOsNativeNotifier` via `objc2-user-notifications` (`UNUserNotificationCenter`) composed with UI notification sink (`CompositeNotificationSink`), honoring per-type prefs, batching, cooldowns, and unbundled test safety | `r_notif_05` (`notifications::native::tests`), `r_notif_01`..`r_notif_04` |
| `P7-T05` (`FR-212`) | `MacOsKeychain` (`security-framework`) + deterministic `InMemoryKeychain`, service/account isolation, invalid input & UTF-8 / locked Keychain error mapping (`NFR-013`) | `r_key_01_store_retrieve_overwrite_and_delete_round_trip`, `r_key_02_services_and_accounts_are_isolated_and_invalid_rejected` |
| `P7-T06` (`FR-210..215`, `Q1`, `BUG-034`) | Offline Ed25519 signature verification (`FF1.*` tokens & `.fflicense` JSON documents, UTF-8 BOM/zero-width sanitization, 64 KiB + symlink cap), Keychain persistence & zero background network checks after activation, lifetime vs timed expiration, provider-agnostic online adapter (`HttpJsonLicenseProvider` verified + `UnconfiguredOnlineProvider` active until `Q1` vendor chosen) | `r_lic_01`..`r_lic_06`, `bug_034_offline_license_handles_bom_and_keychain_error_never_leaks_secret` |
| `P7-T07` (`FR-220`, `UI-KEYS-01..02`) | Global & contextual keyboard shortcuts (`⌘N`, `⇧⌘O`, `⌘O`, `⌘F` focus `[data-ff-search]`, `⌘Z` outside inputs/textareas/selects/contenteditable only, `⌘R` active store refresh + `ff:refresh`, `⌘,`, plus `⌘↩` on `/preview`), suppressed when `aria-modal` dialog is open | `src/hooks/useGlobalShortcuts.test.tsx` (4 tests) |
| `P7-T08` (`UI-LIC-01`) | `/license` page, Sidebar license badge (`Licensed ✓` / `Unlicensed`), `.fflicense` file import picker, Deactivate confirmation dialog, prior valid license preservation on failed activation error, `GeneralSection` tray/minimized/login controls | `src/pages/license/license.test.tsx` (5 tests), `src/pages/settings/settings.test.tsx` (`GeneralSection`) |

---

## 3. Checklist results

### A. Data safety (`SAFETY_MODEL.md` S1–S11)
- [x] Static grep confirmed **zero** mutating filesystem calls (`std::fs::{rename,remove_file,remove_dir,remove_dir_all,copy,write}`, `tokio::fs::*`) outside `engine::safe_fs`, read-only `engine/fs_view.rs`, and test fixtures (`test_support.rs`, test modules). `ff-macos-events` performs zero filesystem writes.
- [x] `.fflicense` file reads inspect `symlink_metadata`, verify regular files, and cap reads to `MAX_LICENSE_FILE_BYTES` (`64 KiB`).
- [x] `VolumeMounted` / `VolumeUnmounted` path matching normalizes macOS `/private/var` & `/private/tmp` symlinks and checks component-wise `PathBuf::starts_with` so `/Volumes/Backup` never matches `/Volumes/Backup 2` and disabled folders stay untouched (`edge_07_volume_mount_and_unmount_isolate_sibling_prefix_and_disabled_folders`).

### B. Correctness
- [x] `BUG-034`: Offline license input sanitization now strips leading/trailing Unicode BOM (`\u{FEFF}`) and zero-width format characters across both `FF1.*` tokens and `.fflicense` JSON documents before routing or parsing.
- [x] `UI-KEYS-02`: `⌘Z` inside `<input>` and `<textarea>` preserves native text editing undo (`defaultPrevented === false`, zero `activity_undo` calls), and all global shortcuts yield cleanly when `[role="dialog"][aria-modal="true"]` is mounted.
- [x] `license.test.tsx` verifies a failing activation key surfaces `role="alert"` without clearing or corrupting an already active stored license.

### C. Concurrency & resources
- [x] `NSWorkspace` observer tokens are `!Send` AppKit objects held on the main thread inside `WorkspaceObserverGuard`; workspace notifications are forwarded over a bounded channel to Tokio runtime tasks without holding mutexes across awaits.
- [x] `license_pick_and_activate_offline_file` executes synchronous file dialog + disk read inside `tokio::task::spawn_blocking`.
- [x] `AutomationPipeline::is_busy()` tracks active reconciles via an RAII `BusyGuard` over `Arc<AtomicU64>`, ensuring tray `Working` indicators reset reliably even on early errors.

### D. Security
- [x] `src-tauri/capabilities/default.json` remains strictly least-privilege (`core:app:default`, `core:event:default`, `core:window:default`, `core:window:allow-start-dragging` — zero `fs`, `shell`, `http`, or `opener` permissions).
- [x] Static grep confirmed **zero** `std::process::Command` / `tokio::process::Command` calls across runtime crates and **zero** network clients (`reqwest`) outside `security/license.rs`.
- [x] Keychain and validation errors never include raw license payloads, signatures, or secret keys (`bug_034` assertion + `mask_license_key` masking); `src/` contains zero `dangerouslySetInnerHTML` or `eval(` calls.
- [x] `cargo audit` (571 crates) reported **0 vulnerabilities** (`3` pre-existing upstream allowed warnings `paste`, `proc-macro-error`, `glib`); `ADR-008` exception (`RUSTSEC-2026-0459`) re-verified (`sqlx` remains `0.9.0`); new Phase 7 dependencies documented in `docs/DECISIONS.md` (`ADR-017`).
- [x] `pnpm audit --prod` reported **0 known vulnerabilities**.

### E. UI / theme
- [x] `LicensePage`, `GeneralSection`, `Sidebar` license indicator, and search inputs (`[data-ff-search]`) pass `pnpm check:tokens` with zero hardcoded colors or dimensions outside `src/styles/theme.css`.
- [x] All routes in `src/app/navigation.ts` (`overview`, `folders`, `rules`, `preview`, `activity`, `duplicates`, `scheduler`, `categories`, `settings`, `license`) are now live and verified.

### F. Docs
- [x] `docs/BUGS.md` (`BUG-034`), `docs/PLAN_TRACKER.md` (`P7-T01`..`P7-T08`), `docs/ARCHITECTURE.md`, `docs/DECISIONS.md` (`ADR-017`), `docs/CHANGELOG.md`, `README.md`, and `docs/audits/phase-7.md` synchronized with verified code, test counts (`317` Rust · `108` TS), and audit results.

---

## 4. Bugs found and fixed during audit

| ID | Severity | Area | Summary | Regression test |
|---|---|---|---|---|
| `BUG-034` | S2 | `src-tauri/src/security/license.rs` | `str::trim()` / `char::is_whitespace()` do not strip UTF-8 BOM (`U+FEFF`) or zero-width format characters, misrouting BOM-prefixed `FF1.*` tokens to the online provider and rejecting BOM-prefixed `.fflicense` JSON documents. Added `sanitize_license_input` and `is_strip_char`. | `security::license::tests::bug_034_offline_license_handles_bom_and_keychain_error_never_leaks_secret` |

Additional edge-case guards added during audit: `edge_07_volume_mount_and_unmount_isolate_sibling_prefix_and_disabled_folders` (`system/coordinator.rs`) and `edge_07_tray_zero_folders_and_close_vs_quit_disposition` (`system/tray.rs`), plus frontend integration/unit coverage (`src/pages/license/license.test.tsx`, `src/hooks/useGlobalShortcuts.test.tsx`, `src/pages/settings/settings.test.tsx`).

---

## 5. Deferred / owner actions

1. **Online licensing vendor decision (`Q1` in `docs/PRD.md`)**: `LicenseProvider` and `HttpJsonLicenseProvider` (verified in `r_lic_05`) are implemented and Keychain-backed, while `UnconfiguredOnlineProvider` remains active in `AppState` until an online vendor endpoint (`Q1`) is chosen. Offline Ed25519 `FF1.*` tokens and `.fflicense` files work end-to-end today.
2. **Upstream crate advisories (3 allowed warnings)**: `paste` (`RUSTSEC-2024-0436`), `proc-macro-error` (`RUSTSEC-2024-0370`), and `glib` (`RUSTSEC-2024-0429`) remain monitored; `ADR-008` (`RUSTSEC-2026-0459`, bundled SQLite under `sqlx 0.9.0`) re-checked and remains unchanged.
3. **Phase 8 QA & Phase 9 notarized release gates (`Q3`, `Q5`)**: coverage measurement, failure suite (`R-FAIL-01..12`), Playwright E2E, Criterion benchmarks, accessibility audit, and Developer ID signing/notarization/app icon assets remain scheduled for Phases 8 and 9.

---

## 6. Sign-off

- [x] All S0/S1 fixed
- [x] All gates green after code fixes (`317` Rust tests, `108` Vitest tests, 10× flake pass, `cargo audit` & `pnpm audit` clean)
- [x] Tracker & architecture docs updated (`P7-T01`..`P7-T08` ✅; online vendor `Q1` explicitly tracked)

