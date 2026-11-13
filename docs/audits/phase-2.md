# Phase 2 Audit — Filesystem engine

| Field | Value |
|---|---|
| Phase | 2 |
| Audit date | 2026-11-13 |
| Status | ✅ Signed off |
| Base commit | `0b9a363` (Phase 2 work uncommitted at audit time) |

## 1. Gate results (final run after all fixes)

| Gate | Result |
|---|---|
| cargo fmt --check | ✅ |
| cargo clippy --all-targets --all-features -D warnings | ✅ 0 issues |
| cargo test | ✅ 101 / 101 — **run 10× consecutively, 10/10 green** (plus 15/15 earlier after the BUG-013 fix) |
| cargo audit | ✅ exit 0 — ADR-008 exception re-checked (latest sqlx still 0.9.0, still caps libsqlite3-sys < 0.38) |
| pnpm lint / typecheck / build | ✅ |
| pnpm test | ✅ 52 / 52 |
| pnpm check:tokens | ✅ |
| Phase 1 tests | ✅ all still pass (2 shell/App expectations intentionally updated: default screen is now Folders) |
| Real app launch | ✅ starts, monitor + dialog plugin + 17 commands + 2 events wired; Vite dev-reload issue found and fixed (ENV-002). Adding a folder through the native picker could not be driven from here — **owner to verify by hand** (see §6). |

## 2. Requirement coverage

| Req | Implemented by | Tests |
|---|---|---|
| FR-002 folder selection | `folders_add_with_picker`, `folders_add_known` (dialog runs in Rust) | UI-FOLD-02, R-FOLD-01 |
| FR-003 permission denial | `probe_access`, PermissionDenied UiError + hint, `open_privacy_settings`, Info.plist strings | R-PERM-01/02, UI-FOLD-04, BUG-011 |
| FR-010/011 multiple folders + per-folder settings | `FolderService`, `FolderSettings`, overlap rule (ADR-013) | R-FOLD-01..05, 3-folder test |
| FR-012 create/modify/rename/move/delete/new dir | monitor | R-WATCH-01..06 |
| FR-013 debounce/dedupe | `debounce.rs` | debounce tests, overflow test |
| FR-014 reconciliation | `reconcile.rs`, interval/rescan/overflow/start | R-RECON-01..04, overflow test |
| FR-015 stability | `stability.rs` | R-STAB-01..05 |
| FR-016 ignore list | `ignore.rs` | ignore tests, R-SCAN-04, R-STAB-05 |
| FR-017 no symlink follow / loops | scanner, fs_view, paths | R-SCAN-05, R-PATH-04/05, fs_view tests |
| FR-018 start/pause/resume/stop/restart | `MonitorService`, `folders_action`, `monitoring_set_paused` | pause/resume, global pause, UI-FOLD-03 |
| FR-019 errors + statistics | `watcher_errors`, `folders_errors`, `folders_summary`, runtime counts | R-SUP-01, UI-FOLD-04 |
| FR-020 watcher restart with backoff | supervisor | R-SUP-01 |
| FR-022/023/024 restart, drive disconnect, permission change, rename/delete | supervisor + diff reconcile | R-RECON-03, R-SUP-02/03/04 |
| FR-025 watcher resource limits | bounded channel + overflow → reconcile | overflow test |
| FR-030–035 classification | `classifier.rs`, `categories/` | R-CLS-01..08, R-CAT-01..03b |
| NFR-005 / S7 / S8 path authority | `paths.rs` | R-PATH-01..08b + proptest |
| NFR-021 bounded pipeline | channel 4096, debouncer ≤10k, tracker ≤10k, batches 500, reconcile channel 2 | R-SCAN-07, overflow test |
| Exit: zero writes | — | `r_mon_monitoring_makes_zero_filesystem_writes` |

## 3. Checklist results

### A. Data safety
- [x] No mutating filesystem call in product code. `grep` for `std::fs::(rename|copy|write|remove_|File::create)` outside `test_support.rs` → only `create_dir_all` for app-owned log/DB dirs. Clippy bans active.
- [x] **Hardening from this audit:** `std::fs::set_permissions`, `std::fs::hard_link` and `std::os::unix::fs::symlink` were not banned. Only tests used them, but nothing prevented product code from doing so. Now clippy-banned (R-SEC-02 asserts it); tests use `test_support::{set_mode, symlink}`.
- [x] Monitoring is read-only — snapshot test proves path/size/mtime/inode unchanged across a full session.
- [x] PathAuthority: traversal, symlink escape, sibling-prefix, NFC/NFD, forbidden roots; proptest 512 random paths never escape.
- [x] Header reads: `O_NOFOLLOW | O_NONBLOCK`, regular files only, ≤ 8 KiB — a FIFO cannot hang the scanner (tested).
- [x] **New-file semantics reviewed for Phase 5 impact** → BUG-008 and BUG-009 found (both S1) and fixed; documented in SAFETY_MODEL.md.
- [ ] Ledger / dry-run — Phases 4–5.

### B. Correctness
- [x] Edge-case names: unicode NFC/NFD (paths normalised everywhere), spaces, leading dots, no extension, double extension, 255-byte components.
- [x] Disguised executables (`invoice.pdf` that is Mach-O) → Other/Low, also when a custom category claims the extension.
- [x] Time: debounce/stability are pure with caller-supplied time; scan stamps monotonic per folder (BUG-013).

### C. Concurrency & resources
- [x] Every queue bounded; overflow degrades to reconcile, never unbounded buffering or silent loss (60-file flood with max_pending 5 → 60/60 reported).
- [x] One writer per folder's records; scans run on `spawn_blocking` with backpressure.
- [x] Stop is bounded and cancels (BUG-010); command sends to busy tasks never block.
- [x] Flakiness: suite run 25× in total during the audit; the only failure found was a real bug (BUG-013).
- [ ] Known cost: a new directory in a recursive folder triggers a full-folder reconcile (debounced). Fine for typical folders; to be measured in Phase 8 benchmarks.

### D. Security
- [x] Capabilities unchanged (no dialog/fs/shell permission for the webview; the picker runs in Rust).
- [x] No command accepts a path for an operation. `folders_update` accepts excluded-subfolder paths as configuration; Rust re-validates each one (must exist, be inside the folder).
- [x] No `Command`; System Settings opened via objc2 NSWorkspace with a constant URL (ADR-011). `unsafe_code = deny` still holds (no unsafe added outside one test-only `mkfifo`).
- [x] Events carry no file paths (`folder-activity-event` has folder id + kind only).

### E. UI / theme
- [x] `check:tokens` passes; new primitives (DropdownMenu, ConfirmDialog, Switch, TextField, StatusDot, PathLabel) use tokens only; category colors via static token→class map.
- [x] Empty, loading, error (+retry), action-error (+dismiss) and success ("Saved") states on Folders and detail.
- [x] Keyboard: menu arrow keys/Escape, dialog focuses Cancel and closes on Escape, switches are `role="switch"`, icon buttons have labels; status uses dot **and** text.
- [x] Destructive action copy states files are untouched.
- [ ] Visual review in light/dark/compact — owner (no screenshot capability here).

### F. Docs
- [x] Tracker, ARCHITECTURE (layout, commands, events, routes, §8 monitor), DATABASE (0002, NFC, scan stamps, category seeding), SAFETY_MODEL (ignore list, new-file semantics, read-only proof), TESTING_STRATEGY (Phase 2 catalog + flakiness rule), DECISIONS (ADR-010…014, ADR-004/008 updated), BUGS, CHANGELOG, README.

## 4. Bugs found

| BUG ID | Sev | Summary | Regression test | Status |
|---|---|---|---|---|
| BUG-008 | S1 | Incomplete scan deleted records → old files resurfaced as new | `bug_008_…` | ✅ |
| BUG-009 | S1 | Scope change made existing files look new | `bug_009_…` | ✅ |
| BUG-010 | S2 | Stop could hang/leak a busy task | `bug_010_…` | ✅ |
| BUG-011 | S2 | Missing permission purpose strings | `bug_011_…` | ✅ |
| BUG-012 | S3 | Lower-cased dates | UI test `BUG-012` | ✅ |
| BUG-013 | S2 | Same-ms / backwards-clock scans left stale records (found as flaky test) | `bug_013_…` | ✅ |
| ENV-002 | — | Dev-only Vite reload aborted IPC | `optimizeDeps.include` | ✅ |

## 5. Process record

- Red-first confirmed for every module: path authority (10 red), categories + classifier (13), ignore + scanner (13), debounce + stability (7), folders + permissions (7), reconcile (4), monitor (14), UI folders (10), BUG-008…013 (each red before its fix).
- Not red-first: `fs_view` (thin wrapper, written with its tests) and the two exit-criteria tests (3-folder, zero-writes), which verify behaviour that already existed — recorded honestly.
- One test-authoring error (scanner name ordering) was fixed in the test, not the code.

## 6. Deferred / owner actions

| Item | Reason | Target |
|---|---|---|
| Manually add a folder via the picker in `pnpm tauri dev`; deny then grant Downloads access to see the prompt text and the "No access" → "Open System Settings" flow | Native dialogs/TCC prompts can't be automated here | Owner, now |
| Visual QA of Folders screens (light/dark/compact) | No screenshot capability | Owner, now |
| Category editing commands + screen | Planned | P6-T08 |
| Monitoring settings UI must restart folders on save | Planned | P5/P6 |
| Sleep/wake → `rescan_all` | Native hook | P7-T03 |
| Measure reconcile cost on 10k–100k files | Benchmarks | P8-T04 |

## 7. Sign-off

- [x] All S0/S1 fixed (BUG-008, BUG-009)
- [x] All gates green after fixes; Rust suite stable over repeated runs
- [x] Tracker and docs updated
