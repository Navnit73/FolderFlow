# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: SemVer.

## [Unreleased]

## [1.0.0] - 2026-11-14

### Added — Phase 9 (Production release)
- Production macOS brand & installer assets (`assets/icon-source.svg`, `assets/dmg-background.png`, `scripts/render-brand-assets.mjs`, `src-tauri/icons/*`): replaced interim icon with a macOS squircle FolderFlow iconset (`icon.icns` + Retina PNGs) and a custom `660×400` DMG drag-to-Applications background.
- Universal binary & bundle metadata (`src-tauri/tauri.conf.json`, `src-tauri/Info.plist`, `src-tauri/Entitlements.plist`): configured `universal-apple-darwin` (`arm64` + `x86_64`), `minimumSystemVersion: "13.0"`, `NSSupportsAutomaticGraphicsSwitching`, copyright metadata, `hardenedRuntime: true`, and least-privilege `Entitlements.plist` (non-sandboxed per `ADR-002` with zero JIT/unsigned-memory exemptions).
- Release automation & clean-environment smoke verification (`scripts/release-macos.sh`, `scripts/smoke-test-macos.sh`, `.github/workflows/release.yml`, `src-tauri/tests/release_smoke.rs`): automated universal `.app`/`.dmg` build, SHA-256 checksums, Developer ID `codesign`/`notarytool`/`stapler` hooks (gated on `Q3` Apple Developer ID credentials), and end-to-end clean-environment smoke suite (`R-REL-01..03`, `R-SMOKE-01..08`).
- SemVer `1.0.0` alignment (`package.json`, `src-tauri/Cargo.toml`, `src-tauri/crates/ff-macos-events/Cargo.toml`, `src-tauri/tauri.conf.json`, frontend fixtures), `RELEASE_NOTES.md`, and end-user documentation (`docs/USER_GUIDE.md`).

### Added — Phase 8 (Quality assurance)
- Spec §24.3 failure suite (`src-tauri/tests/failure_suite.rs`, `R-FAIL-01`..`12`): verified crash/fault recovery for interrupted moves/copies, corrupted DB + ledger errors, unavailable destinations, vanished sources, mid-execution destination collisions, `ENOSPC`, `EACCES`, unsafe/malformed config imports, watcher startup/runtime failures with restart backoff, event bursts/dedupe, and active download stability + stale snapshot aborts.
- Performance benchmarks (`src-tauri/benches/folderflow_benches.rs`, `src-tauri/examples/bench_idle_monitor.rs`, `docs/BENCHMARKS.md`): empirical `1k`/`10k`/`50k`/`100k` Criterion measurements (`BENCH-01`..`05`) plus Darwin `/usr/bin/time -l` heavy RSS (`BENCH-06`) and real FSEvents idle CPU/RSS (`BENCH-07`), with initial `NFR-022` regression budgets derived from measured baselines.
- Playwright E2E & WCAG 2.1 AA accessibility harness (`playwright.config.ts`, `tests/e2e/app.spec.ts`, `tests/e2e/helpers/tauriMock.ts`, `scripts/check-tokens.test.ts` `UI-A11Y-01`): `E2E-01`..`06` covering onboarding, folders, rules/builder, preview/execute, activity/review/CSV/undo, settings/shortcuts/license, plus `@axe-core/playwright` (`wcag2a`/`2aa`/`21a`/`21aa`) across all 20 routes + onboarding in both Light and Dark modes.
- Security & coverage hardening (`src-tauri/deny.toml`, `src-tauri/Cargo.toml` pinned path version for `ff-macos-events`, additional unit/integration coverage pushing `engine/safe_fs/` to `95.07%`, `engine/` to `92.90%`, `rules/` to `95.37%`, and total crate line coverage to `86.63%`).

### Fixed — Phase 8 audit
- BUG-035 & BUG-036 (see `docs/BUGS.md`): remediated Light/Dark WCAG 2.1 AA `< 4.5:1` contrast failures (including `--ff-bg-selected` + `-subtle` badge overlays via `--ff-blue-400`, `--ff-red-400`, `--ff-fg-subtle`, `--ff-danger-solid`), added explicit `<label>` accessibility labels to `CategoriesPage` enable switches and `Settings` `SettingRow` toggle switches, and resolved a `clippy::await_holding_lock` issue in `src-tauri/tests/failure_suite.rs`.

### Added — Phase 7 (macOS integration)
- Menu bar tray & background keep-alive (`src-tauri/src/system/tray.rs`): dynamic 18×18 template status glyphs (`Active`, `Paused`, `Working`, `Error`), status summary, Pause All / Resume Monitoring / Organize Now / Preview Pending / Recent Activity / Open FolderFlow / Settings… / Quit FolderFlow (`FR-150`..`152`), closing main window hides to menu bar keep-alive until explicit Quit (`FR-021`), `startMinimized` and `showMenuBarIcon` settings (`GeneralSection.tsx`), and Dock reopen handling (`RunEvent::Reopen`).
- Launch at login (`src-tauri/src/system/login_item.rs`, `src-tauri/src/commands/settings.rs`, `GeneralSection.tsx`): `SMAppService` integration via `smappservice-rs` with `NSBundle` safety guard for unbundled tests, `login_item_status` / `open_login_items_settings` commands, and pending-approval deep-link in Settings → General.
- Sleep/wake & volume lifecycle (`src-tauri/crates/ff-macos-events`, `src-tauri/src/system/coordinator.rs`): `NSWorkspace` sleep/wake & mount/unmount observer bridge triggering `MonitorService::recheck`, fast reconciliation of missed files on wake, automatic `Unavailable` transition on external volume unmount, and immediate monitor resume + reconcile on remount (preserving sibling mount boundaries and disabled folders).
- Native macOS notifications (`src-tauri/src/notifications/native.rs`): `MacOsNativeNotifier` using `UNUserNotificationCenter` composed with UI event sink (`CompositeNotificationSink`), honoring notification preferences and batching/cooldown schedules (`R-NOTIF-05`).
- macOS Keychain & License activation (`src-tauri/src/security/{keychain,license}.rs`, `src-tauri/src/commands/license.rs`, `/license`): `MacOsKeychain` (`security-framework`) + `InMemoryKeychain`, `LicenseService` supporting offline Ed25519-signed `FF1.*` tokens & `.fflicense` JSON documents, zero background network calls after activation (`NFR-010`), Keychain persistence, Deactivate confirmation flow, and provider-agnostic `LicenseProvider` (`HttpJsonLicenseProvider` tested + `UnconfiguredOnlineProvider` until `Q1` vendor is selected), surfaced on `/license` (`UI-LIC-01`) and the sidebar license badge.
- Desktop keyboard shortcuts (`src/hooks/useGlobalShortcuts.ts`, `UI-KEYS-01..02`, `FR-220`): `⌘N`, `⇧⌘O`, `⌘O`, `⌘F` (focuses `[data-ff-search]`), `⌘Z` (strictly skips `<input>`, `<textarea>`, `<select>`, and `[contenteditable]`), `⌘R` (in-app route store refresh + `ff:refresh`), and `⌘,`, suppressed cleanly when `aria-modal` dialogs are open.

### Fixed — Phase 7 audit
- BUG-034 (see docs/BUGS.md): stripped UTF-8 BOM (`U+FEFF`) and zero-width format characters across `sanitize_license_input` / `is_strip_char` in `src-tauri/src/security/license.rs` so BOM-prefixed `FF1.*` tokens and `.fflicense` JSON files validate offline without misrouting to the online provider; verified Keychain errors never expose secret tokens (`NFR-013`). Also added edge-case coverage in `coordinator.rs` (sibling volume mount prefixes + disabled folders) and `tray.rs` (zero-folder state & close-vs-quit disposition).

### Added — Phase 6 (Advanced features)
- Overview dashboard (`/overview`, `src-tauri/src/database/repo_stats.rs`, `src-tauri/src/commands/dashboard.rs`): real SQLite-backed storage breakdown by category, today/week/all-time organization counts, reclaimed duplicate space, trend series, active monitoring summary, and SQLite `ANALYZE`/`VACUUM` maintenance (`db_maintenance`).
- Streaming BLAKE3 hasher, persistent cache, & Duplicates workflow (`/duplicates`, `src-tauri/src/duplicates/{hasher,cache,finder,actions}.rs`, `src-tauri/src/commands/duplicates.rs`): 64 KiB fixed-buffer streaming hashing (2 GB sparse file verified), `(dev, ino, size, mtime_ns)` SQLite hash cache invalidation, size prefilter → 3-window partial hash → full hash grouping, progress events (`dup://progress`), keeper selection, undoable Move to Review (with auto-recorded path exclusion), Trash w/ confirm, Ignore/Unignore, Reveal in Finder, and CSV/JSON export.
- Categories screen (`/categories`, `src-tauri/src/commands/categories.rs`): create/edit/enable/disable/reorder/delete custom categories with extension & color customization, default destination folder picker, and custom rule-action support.
- Smart Organizers (`src-tauri/src/rules/smart.rs`): pattern-customizable Screenshot Organizer, filename/keyword-based Invoice & Receipt Organizer, per-category Clean Downloads catalog (`rules_smart_catalog`), and Age-based Old Files Organizer (`rules_create_smart_preset`, all installed disabled by default).
- Versioned Config Export / Import & Backup (`src-tauri/src/config_io/{export,import,schema}.rs`, `src-tauri/src/commands/config_io.rs`): exported JSON schema with validation, path containment & traversal checks, dry-run diff preview (`config_import_preview`), merge/replace transactional apply (`config_import_apply`), and imported rules forced disabled for user review.
- Notification Service (`src-tauri/src/notifications/{service,throttle}.rs`, `src-tauri/src/commands/notifications.rs`) & complete Settings tabs (`/settings/{monitoring,organization,performance,notifications,privacy,backup}`): batched/throttled notification delivery with per-event preferences, path redaction, unread/read tracking, Global Exclusions CRUD (`exclusions_list/create/delete`), and full Monitoring/Organization/Performance/Notifications/Privacy/Backup settings UI.

### Fixed — Phase 6 audit
- BUG-026 … BUG-027 (see docs/BUGS.md): loaded global and per-folder DB exclusions (`Path`, `Glob`, `Extension`, including `Duplicate Review` exclusions) across `FolderScanSpec`, `IgnoreRules::check_file`/`check_dir`/`is_path_excluded`, `duplicates::finder`, `commands/preview`, `scheduler`, `pipeline`, `monitor` (`refresh_exclusions`), and `onboarding`; hydrated `ContentHash`, `DuplicateStatus`, and `PerformanceSettings.hash_on_scan` via shared `hydrate_candidates_for_rules` in `preview`, `scheduler`, and `pipeline`.
- BUG-028 … BUG-033 (see docs/BUGS.md): fixed `dup_scan` extra-copy notification math & ignored-only scan skip; scoped `duplicate_groups` replacement to scanned `folder_ids` (plus `spawn_blocking` metadata checks in `load_all_groups`); promoted `MoveToReview` Duplicate Review exclusions to global scope; fixed config export/import rule `source_folders` UUID→path remapping, `'other'` & built-in collision protection, cycle validation on `ApplyAnotherRule`, `Replace`-mode orphan `duplicate_groups` cleanup & accurate `config_import_preview(..., mode)` diffs, and never widening unresolved per-folder exclusions/rules; wired `NotificationService::notify_plan_outcome` to emit deduplicated `DestinationUnavailable` / `PermissionIssue` notifications across `pipeline`, `scheduler`, and `plan_execute`; fixed Phase 6 UI error handling, live notification updates + mark-all-read, `SmartOrganizerDialog` save recovery & Escape handling, and `BackupSection` `Replace`-mode preview/5 MiB guard/store refreshes (`edge_06_*`, `UI-DUP-03`, `UI-CAT-02`).

### Added — Phase 5 (Preview & automation)
- Preview screen (`/preview`): non-mutating dry-run plan creation via `engine/planner.rs` (`&dyn ReadOnlyFs` + `PreviewStore`), breakdown by category and rule, folder/rule scope filters, search, conflict badges and blocked-item safeguards, per-item selection, conflict resolution policy selector, and ⌘↩ confirmation execution with pre-execution revalidation (`R-PLAN-01..11`, `R-EXEC-01..05`).
- Real-time automation pipeline (`src-tauri/src/engine/pipeline.rs`) wired to `MonitorService` via `CompositeObserver` (`src-tauri/src/events.rs`), bounded 4096-event intake queue with dirty-folder reconciliation under burst overload, rule/folder schedule filtering, and `file_records` sync after moves/renames.
- Scheduler service (`src-tauri/src/engine/scheduler.rs`, `database/repo_schedules.rs`) & Scheduler screen (`/scheduler`): interval, daily, weekly (selected weekdays), on-launch, and manual schedules, DST-aware next-run computation, quiet hours (`HH:MM` overnight and same-day), `Skip`/`RunOnce` catch-up policies, per-scope concurrency lock, enable/disable toggle, and Run Now.
- Six-step Onboarding wizard (`/onboarding/:step`, `src-tauri/src/onboarding/mod.rs`) & Settings → General (`GeneralSection`): known-folder setup with macOS Privacy guidance, organization preference, starter template selection, non-mutating scan preview, explicit confirmation before enabling starter rules, relaunch resume, and Re-run setup wizard.

### Fixed — Phase 5 audit
- BUG-022 … BUG-025 (see docs/BUGS.md): unified candidate scanning through `FolderScanSpec::from_folder` with live `MonitoringSettings`, fixed `folder_id` schedule scoping and `Realtime` quiet hours in `AutomationPipeline`, excluded disabled folders from unscoped previews, added ancestor permission walk in planner, prevented double-execution of completed preview items, propagated monitoring updates from `settings_update` to `MonitorService`, and offloaded sync folder scans with `spawn_blocking`.

### Added — Phase 4 (Safe operations)
- Activity screen (`/activity`): paginated write-ahead ledger view with search/status/type filters, one-click Undo Last / Undo Selected / per-row Undo / Undo Batch (with empty folder pruning), occupied-path Undo Conflict alert with "Restore with Auto-Rename", pending `Needs Review` conflict resolution queue (`Keep Both` / `Skip`), Retry Failed operation, formula-injection-safe CSV export, Clear History confirmation dialog, and Reveal in Finder (`NSWorkspace`).
- Safe filesystem layer (`src-tauri/src/engine/safe_fs/`): the sole mutating FS module using `renamex_np(RENAME_EXCL)`, streaming 64 KiB `copy_excl` (`O_CREAT | O_EXCL` + BLAKE3 + `fsync`), `VerifiedSourceToken`-gated `remove_verified_source`, `NSFileManager` Trash integration (`SafeFs::production`) with isolated temp-Trash override (`SafeFs::with_trash_dir`), `statvfs` disk space pre-check, and deterministic fault-injection wrapper (`FaultyFs`).
- Write-ahead operation ledger (`engine/operations.rs`, `database/repo_operations.rs`), transactional move/copy/rename pipeline (`engine/organizer.rs`), race-safe conflict resolver (`engine/conflicts.rs`), honest undo service (`engine/undo.rs`), startup crash recovery + orphan `.folderflow-*.partial` sweep (`engine/recovery.rs`), and corrupted SQLite database backup + recreation (`database/pool.rs`, `NFR-032`).

### Fixed — Phase 4 audit
- BUG-018 … BUG-021 (see docs/BUGS.md): persisting auto-renamed `destination_path` in `VERIFYING` before cross-volume `remove_verified_source`, deduplicating `op_ids` in `UndoTarget::Ops`, paginating `export_csv` beyond 500 rows, and allowing macOS root-level `/var`, `/tmp`, and `/etc` system links in `SafeFs::mkdir_p`.

### Added — Phase 3 (Rule engine)
- Rules screen (`/rules`): priority-ordered list, enable/disable switches, priority reorder, duplicate, delete with confirmation, and starter rule templates menu (Organize PDFs, Organize Screenshots, Organize Images, Organize Videos, Organize Archives, Organize Invoices & Receipts, Clean Old Downloads — all created disabled by default).
- Visual Rule Builder (`/rules/new`, `/rules/:id`): nested `ALL`/`ANY`/`NOT` condition groups (up to depth 5), 11 condition families (text/regex, extension group, category, decimal file size `1 KB = 1000 B`, local-timezone date operators, location, glob, hidden, empty, duplicate status, BLAKE3 hash), 16 safe action types (no shell/command execution), naming preset chips, native destination folder picker, live template preview, inline validation errors/warnings, and dry-run "Test rule on folder".
- Pure Rust rule engine (`src-tauri/src/rules/`): versioned JSON condition/action envelopes (`"v": 1`), timezone- and DST-aware `Clock` trait (`SystemClock` + `FixedClock`), linear-time regex validation with `256 KiB` size cap, variable sanitization and `PathAuthority` root containment on dynamic templates, priority evaluation across 5 execution modes, per-pass move/action limits, `ApplyAnotherRule` cycle & depth guards, and destination loop detection (`FR-054`).

### Fixed — Phase 3 audit
- BUG-014 … BUG-017 (see docs/BUGS.md): UTF-8 char-boundary truncation when duplicating long multi-byte rule names, literal `{`/`}` braces in `AddPrefix`/`AddSuffix` actions, multi-hop `ApplyAnotherRule` cycle validation at save time, and `NOT(InsideSubdirectory)` loop-guard checks inside `ANY` condition groups.

### Added — Phase 2 (Filesystem engine)
- Folders screen: add Downloads/Desktop/Documents/Pictures/Movies/Music or any folder via the native picker; live status (Watching, Paused, Unavailable, No access, Error); pause/resume/rescan per folder and globally; remove (files untouched); detail page with per-category storage, settings (name, enabled, include subfolders, skip/only extensions, skipped subfolders, stability override) and recent problems; "Open System Settings" when access is missing.
- Sidebar monitoring indicator.
- Rust engine (read-only): path authority with symlink/traversal protection and forbidden system roots; extension + content-signature classifier with 13 built-in categories and custom categories; scanner with ignore rules, packages as single items, bounded batches; FSEvents watcher with debounce, stability checks, reconciliation, supervisor with backoff and health states; migration 0002.
- Info.plist purpose strings for folder-access prompts.

### Fixed — Phase 2 audit
- BUG-008 … BUG-013 (see docs/BUGS.md), including two cases where existing files could later have been treated as new.

### Added — Phase 1 (Foundation)
- Tauri 2 app (`com.folderflow.desktop`) with React 19, TypeScript 6 (strict), Vite 8, Tailwind 4, Zustand, React Router.
- Single theme file `src/styles/theme.css` drives all UI; `pnpm check:tokens` enforces it (no hardcoded colors/sizes, dark/system parity).
- App shell: sidebar (unbuilt screens shown as disabled "Soon"), page header with window drag region, hash routing.
- Settings → Appearance (System/Light/Dark, Comfortable/Compact) and Settings → About (version, schema, data and log folders).
- Rust core: `AppError` → `UiError` with stable codes; tracing logs to `~/Library/Logs/com.folderflow.desktop` with daily rotation, retention and path redaction; SQLite (sqlx 0.9, WAL, foreign keys) with full initial schema; typed, validated settings stored per section with corruption fallback.
- Typed Rust → TypeScript bindings (tauri-specta) with a staleness test.
- Security baseline: least-privilege capabilities, strict CSP, `unsafe_code = deny`, clippy bans on overwrite/delete/command APIs.
- CI workflow (GitHub Actions, macOS) running every gate.

### Fixed — Phase 1 audit
- BUG-001 … BUG-007 (see docs/BUGS.md), including dropped info logs and an out-of-order settings save race.

### Added — Phase 0 (Planning)
- PRD, phase plan tracker, architecture, database design, filesystem safety model, TDD testing strategy, audit process, decision log, bug log.
