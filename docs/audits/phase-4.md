# Phase 4 Audit — Safe operations

| Field | Value |
|---|---|
| Phase | 4 |
| Audit date | 2026-11-14 |
| Status | ✅ Signed off |
| Base commit | `0b9a363` (Phase 3–4 work uncommitted at audit time) |

## 1. Gate results (final run after all fixes)

| Gate | Result |
|---|---|
| cargo fmt --check | ✅ |
| cargo clippy --all-targets --all-features -D warnings | ✅ 0 issues |
| cargo test | ✅ 216 / 216 — **run 10× consecutively, 10/10 green** |
| proptest 10k cases (`PROPTEST_CASES=10000 cargo test`) | ✅ 60,000+ generated cases across `r_ops_05_proptest_random_transition_sequences_never_reach_illegal_state`, `r_conf_proptest_candidate_path_respects_component_limits_and_uniqueness`, `r_conf_05_proptest_concurrent_conflicting_moves_never_overwrite_or_lose_files`, and Phase 2–3 proptests — 0 failures |
| cargo audit | ✅ exit 0 (0 vulnerabilities; 3 allowed transitive Tauri warnings) |
| pnpm lint / typecheck / build | ✅ |
| pnpm test | ✅ 69 / 69 (10 test files) |
| pnpm check:tokens | ✅ |
| Phase 1–3 tests | ✅ all still pass with zero regressions |

## 2. Requirement coverage

| Req | Implemented by | Tests |
|---|---|---|
| FR-090 atomic same-volume move (`renamex_np(RENAME_EXCL)`) | `engine/safe_fs/rename.rs`, `engine/organizer.rs` | R-SFS-01..02, R-MOVE-01..02 |
| FR-091 staged cross-volume move (`.folderflow-<opid>.partial` → `fsync` → BLAKE3 + size + source identity verify → `rename_excl` → `remove_verified_source`) | `engine/safe_fs/copy.rs`, `engine/safe_fs/mod.rs`, `engine/organizer.rs` | R-SFS-03..04/06/08, R-MOVE-03..05/07 |
| FR-092 copy & rename pipelines | `engine/organizer.rs` | R-COPY-01..03, R-REN-01..03 |
| FR-093 conflict resolution (`auto_rename`, `skip`, `ask` / `review_queue`, `keep_both`) | `engine/conflicts.rs`, `engine/organizer.rs`, `engine/activity.rs` | R-CONF-01..05 (+ 2 proptests), UI-ACT-05 |
| FR-094 disk space pre-check (`statvfs`, `size + max(size*0.10, 10 MB)`) | `engine/safe_fs/mod.rs`, `engine/organizer.rs` | R-MOVE-08, R-COPY-03 |
| FR-095 write-ahead operation ledger (`PRAGMA synchronous = FULL`) & batch tracking (`operation_batches`) | `database/repo_operations.rs`, `engine/operations.rs` | R-OPS-01..05 (+ proptest), R-LEDG-01..04 |
| FR-100–101 undo (last, selected, batch, restore structure, occupied-path conflict, copy-undo BLAKE3 & original check) | `engine/undo.rs`, `commands/activity.rs`, `ActivityPage.tsx` | R-UNDO-01..07, BUG-019, UI-ACT-02..03 |
| FR-160–163 Activity log screen, filters, retry failed, CSV export, clear history, reveal in Finder | `engine/activity.rs`, `commands/activity.rs`, `system/mod.rs`, `ActivityPage.tsx` | UI-ACT-01..05, BUG-020 |
| NFR-001–003 zero silent overwrite, zero data loss at any fault/crash point, Trash-only deletion | `engine/safe_fs/*`, `engine/organizer.rs`, `engine/recovery.rs` | R-SFS-01..10, R-MOVE-01..08, R-RECOV-01..08, BUG-018 |
| NFR-030 startup crash recovery for every non-terminal state + orphan `.folderflow-*.partial` sweep | `engine/recovery.rs`, `app_state.rs` | R-RECOV-01..08, BUG-018 |
| NFR-032 corrupted SQLite database backed up (`.corrupt-<epoch>.bak`) and recreated cleanly | `database/pool.rs` | R-RECOV-09 |

## 3. Checklist results

### A. Data safety (Mandatory Phase 4 Safety Sign-Off — `SAFETY_MODEL.md` S1–S11)
- [x] **S1 — No silent overwrite.** Every move, rename, staged-copy commit, undo, and Trash relocation uses `SafeFs::rename_excl` (`libc::renamex_np(RENAME_EXCL)`) or `SafeFs::copy_excl` (`OpenOptions::create_new(true)` / `O_CREAT | O_EXCL`). `std::fs::rename` and `std::fs::copy` remain banned by `clippy.toml` (`R-SFS-01..03`, `R-SEC-02`).
- [x] **S2 — No data loss on interruption.** Fault injection at every mutating step (`FaultyFs::fail_at_step` / `fail_next_op` / `PartialCopy`) and startup recovery (`recover_startup`) verified across all non-terminal states (`PENDING`, `VALIDATING`, `IN_PROGRESS`, `VERIFYING`, `ROLLBACK_REQUIRED`) that either the source file is intact or the verified destination is intact — never neither (`R-MOVE-04..05`, `R-RECOV-01..08`, `BUG-018`).
- [x] **S3 — No permanent deletion.** Only `SafeFs::remove_verified_source` calls `std::fs::remove_file`, and its signature requires a module-private `VerifiedSourceToken` that can only be minted by `SafeFs::verify_copied` after verifying size, BLAKE3 digest, and unchanged source `(dev, ino, size, mtime_ns)`. All other removals (partial staging cleanup, copy-undo, orphan `.partial` sweep) call `SafeFs::move_to_trash` (`R-SFS-05/08`, `R-UNDO-03..04`, `R-RECOV-04/06/08`).
- [x] **S4 — Write-ahead ledger.** `Ledger::begin_in_progress` commits `IN_PROGRESS` with `PRAGMA synchronous = FULL` and mints a module-private `LedgerToken` required by `Organizer`'s execution methods; `r_ledg_01_intent_is_committed_before_first_fs_mutation` proves via `FaultyFs::set_on_mutation` hook that the row is already committed in SQLite before step 1 executes.
- [x] **S5 — Scoped paths & symlink defence.** Every source and destination path is validated through `PathAuthority::authorize_existing` / `authorize_destination`, `SafeFs::ensure_no_symlink_ancestors`, and `O_NOFOLLOW` before mutation (`R-SFS-07`, `R-MOVE-06`, `BUG-021`).
- [x] **S7 — Stable files only.** `Organizer` records `(dev, ino, size, mtime_ns)` before mutation and re-verifies `EntryIdentity::same_version` before committing (`R-MOVE-05/07`).
- [x] **S8 — Honest Undo.** Every undo is a first-class ledgered operation (`undo_move`, `undo_copy`, `undo_rename`) in an `undo` batch, sets `reversed_by` on the original row, refuses to overwrite an occupied original path unless `auto_rename_conflicts: true` is explicitly chosen (`R-UNDO-05`), and refuses to trash a copy whose BLAKE3 hash changed or whose original file is missing (`R-UNDO-04`).

### B. Correctness
- [x] Legal operation state machine transitions enforced both in Rust (`OpStatus::can_transition_to`) and in SQLite (`WHERE id = ? AND status = ?`), verified by 10,000-case property test (`R-OPS-01..05`).
- [x] Race-safe conflict resolution (`claim_with_rename_excl`) handles `AutoRename`, `KeepBoth`, `Skip`, and `Ask` (`review_queue`) without TOCTOU `exists()` checks, preserving compound extensions (`tar.gz`, `tar.xz`, etc.) and 255-byte component limits (`R-CONF-01..05` + 2 proptests).
- [x] CSV export neutralizes spreadsheet formula injection (`=`, `+`, `-`, `@`, `\t`, `\r`) and paginates across all matching rows (`activity_service_exports_csv_retries_and_resolves_reviews`, `BUG-020`).

### C. Concurrency & resources
- [x] Streaming copy and BLAKE3 hashing use a fixed 64 KiB buffer (`STREAM_BUF_BYTES = 64 * 1024`) with `O_NOFOLLOW | O_NONBLOCK`.
- [x] All tests isolate Trash into `SafeFs::with_trash_dir(root.join(".Trash"))` so running `cargo test` never touches the developer's real `~/.Trash` (`TESTING_STRATEGY.md` §4).
- [x] Rust test suite (`216` tests) run 10× consecutively with 0 failures (~2.5s per run).

### D. Security
- [x] `reveal_in_finder` validates the operation ID against the ledger, resolves the existing file or ancestor directory, and uses AppKit `NSWorkspace::selectFile_inFileViewerRootedAtPath` directly (no `open` CLI or shell command).
- [x] `cargo audit` passes with 0 vulnerabilities.

### E. UI / theme
- [x] `pnpm check:tokens` passes; `ActivityPage.tsx` uses semantic tokens from `src/styles/theme.css` exclusively.
- [x] Empty, filtered-empty, loading, error (+ retry/dismiss), occupied-path undo conflict alert, pending `Needs Review` queue, and `Clear Activity History` confirmation dialog implemented and tested (`UI-ACT-01..05`).

### F. Docs
- [x] `PLAN_TRACKER.md`, `ARCHITECTURE.md`, `SAFETY_MODEL.md`, `TESTING_STRATEGY.md`, `DECISIONS.md` (ADR-016), `BUGS.md` (BUG-018..021), `CHANGELOG.md`, `README.md`, and `audits/phase-4.md` updated and in sync.

## 4. Bugs found and fixed during audit

| BUG ID | Sev | Summary | Regression test | Status |
|---|---|---|---|---|
| BUG-018 | S1 | In `Organizer::execute_staged_copy_or_cross_volume_move`, when `claim_with_rename_excl` auto-renamed `staging` to `name (N).ext` (`final_path != dst`) and `remove_verified_source` failed (or crashed before `COMPLETED`), the ledger row in `VERIFYING` still had `destination_path == dst` instead of `final_path`; startup recovery (`recover_verifying`) then inspected `dst` instead of `final_path` and rolled back instead of completing the move. | `engine::recovery::tests::bug_018_cross_vol_auto_rename_fault_on_remove_source_recovers_to_completed` | ✅ Fixed |
| BUG-019 | S2 | `UndoService::undo` (`UndoTarget::Ops`) did not deduplicate `op_ids` and checked `row.reversed_by` only on the pre-loop snapshot, so duplicate operation IDs in a batch selection attempted a second undo on an already-reversed operation and recorded a spurious error. | `engine::undo::tests::bug_019_undo_ops_deduplicates_ids_without_spurious_error` | ✅ Fixed |
| BUG-020 | S2 | `ActivityService::export_csv` queried only a single page (`limit: 500, offset: 0`), silently truncating exported CSV files at 500 operations when more than 500 matching records existed. | `engine::activity::tests::bug_020_export_csv_paginates_beyond_500_rows` | ✅ Fixed |
| BUG-021 | S1 | `SafeFs::mkdir_p` rejected macOS root-level system symlinks (`/var -> private/var`, `/tmp -> private/tmp`, `/etc -> private/etc`) when passed a non-canonicalized path such as `/var/folders/...` from `tempfile::TempDir`, causing `database::open` to fail on uncanonicalized `/var/folders` paths. | `engine::safe_fs::tests::bug_021_mkdir_p_allows_macos_root_system_links_while_rejecting_other_symlinks` | ✅ Fixed |

## 5. Process record

- Red-first confirmed for every Phase 4 task:
  - `P4-T01` & `P4-T02` (`engine/safe_fs/*`): 10 tests (`r_sfs_01..10`) failed on stub `todo!()` before implementation.
  - `P4-T03` & `P4-T04` (`engine/operations.rs`, `database/repo_operations.rs`): 9 tests (`r_ops_01..05` + proptest, `r_ledg_01..04`) failed before implementation.
  - `P4-T05`, `P4-T06`, `P4-T07` (`engine/organizer.rs`, `engine/conflicts.rs`): 19 tests (`r_move_01..08`, `r_copy_01..03`, `r_ren_01..03`, `r_conf_01..05`) failed before implementation.
  - `P4-T08` & `P4-T09` (`engine/undo.rs`, `engine/recovery.rs`, `database/pool.rs`): 16 tests (`r_undo_01..07`, `r_recov_01..09`) failed before implementation.
  - `P4-T10` (`src/pages/activity/activity.test.tsx`): 5 UI tests (`UI-ACT-01..05`) failed against stub `ActivityPage.tsx` before implementation.
  - `BUG-018..021`: all regression tests written and verified failing before applying fixes.

## 6. Deferred / owner actions

| Item | Reason | Target |
|---|---|---|
| Visual QA of Activity screen (`/activity`) in `pnpm tauri dev` and verifying `Reveal in Finder` opens Finder window | Native Finder window cannot be asserted in headless test runner | Owner, now |
| Preview diff screen (`/preview`), Apply Selected / Apply All batch execution, and Automation wiring (live watcher → rule evaluation → `Organizer`) | Planned | Phase 5 (`P5-T01..T09`) |

## 7. Sign-off

- [x] All S0/S1/S2 bugs fixed (`BUG-018`, `BUG-019`, `BUG-020`, `BUG-021`)
- [x] All gates green after fixes; 10k proptests green; Rust suite 10/10 consecutive runs green
- [x] Mandatory Phase 4 safety checklist (`SAFETY_MODEL.md` S1–S8) verified and signed off
- [x] Tracker and docs updated

