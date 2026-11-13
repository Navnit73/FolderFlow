# Phase 1 Audit — Foundation

| Field | Value |
|---|---|
| Phase | 1 |
| Audit date | 2026-11-13 |
| Status | ✅ Signed off (one item awaiting product-owner acknowledgement: ADR-008) |
| Commit | (repo not yet under git) |

## 1. Gate results (final run after all fixes)

| Gate | Result |
|---|---|
| cargo fmt --check | ✅ |
| cargo clippy --all-targets --all-features -D warnings | ✅ |
| cargo test | ✅ 24 / 24 |
| cargo audit | ✅ exit 0 — 1 advisory ignored with ADR-008; 3 "unmaintained/unsound" warnings in transitive Tauri deps (paste, proc-macro-error, glib — glib is Linux-only and not linked on macOS) |
| pnpm lint | ✅ |
| pnpm typecheck | ✅ |
| pnpm test | ✅ 38 / 38 |
| pnpm build | ✅ |
| pnpm check:tokens | ✅ |
| Previous phases' tests | n/a (Phase 0 had no code) |
| Real app launch | ✅ `pnpm tauri dev` starts; DB created at `~/Library/Application Support/com.folderflow.desktop/folderflow.db`; log line `FolderFlow started version="0.1.0"` written. Screenshot not captured (command sandbox cannot enumerate windows) — visual check left to the product owner. |

Build note: on this machine Rust must be built with `SDKROOT=…/MacOSX26.5.sdk` (ENV-001).

## 2. Requirement coverage

| Req | What | Tests |
|---|---|---|
| NFR-040 | Single theme file, enforced | UI-THEME-01..05, check-tokens repo scan, `cn` tests |
| NFR-041 | Light/dark/system + density | UI-THEME-01..03, UI-SHELL-03, App bootstrap |
| NFR-011 | Least-privilege capabilities | R-SEC-01 |
| NFR-012 | Strict CSP | R-SEC-01b |
| NFR-031 | WAL, FKs | R-DB-02, R-DB-03 |
| NFR-013 | Log redaction, no contents | R-LOG-02, R-LOG-02b, R-ERR-02 |
| NFR-050 | CI gates | `.github/workflows/ci.yml` (not yet executed — no remote) |
| FR-180–185 (storage) | Typed, validated, persisted settings | R-SET-01..05, model tests |
| FR-184 (UI) | Appearance settings | UI-SHELL-03 |
| S1/S3/S11 prep | Dangerous calls banned | R-SEC-02 + manual probe below |

## 3. Checklist results

### A. Data safety
- [x] No mutating filesystem call exists outside logging/DB setup (`create_dir_all` for app-owned dirs only).
- [x] Clippy ban **verified to fire**: temporarily added `std::fs::write` + `std::fs::rename` to `utils/time.rs`; `cargo clippy -D warnings` failed with `use of a disallowed method`; probe removed.
- [ ] Ledger / PathAuthority / dry-run checks — not applicable until Phases 2–5.

### B. Correctness
- [x] Every Phase 1 test ID has a passing test.
- [x] Errors typed; logged once at the command boundary (`From<AppError> for UiError`); UI shows message + hint, never internals (R-ERR-02).
- [x] Corrupted settings row cannot block startup (R-SET-04).
- [x] Concurrency of optimistic UI saves reviewed → **BUG-004 found and fixed**.

### C. Concurrency & resources
- [x] DB pool bounded (4 connections); logging uses a non-blocking writer with a retained guard.
- [x] No blocking IO on the UI thread: DB opened in `setup()` via `block_on` before the window loads (startup only, bounded); all commands are async.

### D. Security
- [x] Capabilities: `core:app/event/window` defaults + start-dragging only. No fs/shell/http/opener plugins.
- [x] CSP: no remote origins, no unsafe script sources in production; `freezePrototype` on.
- [x] Direct `invoke` from the frontend is lint-banned; only generated, typed commands are used.
- [x] `unsafe_code = "deny"` crate-wide.
- [x] cargo audit: **BUG-002** fixed (sqlx 0.8 → 0.9 removed `rsa` advisory, SQLite upgraded 0.30 → 0.37). Remaining RUSTSEC-2026-0459 accepted via **ADR-008** (not reachable: app opens only its own DB). **Needs product-owner acknowledgement.**

### E. UI / theme
- [x] `check:tokens` passes; it rejects hex/rgb/hsl, palette classes, arbitrary values, extra stylesheets; dark/system parity enforced.
- [x] Motion now uses theme tokens (**BUG-005**).
- [x] Empty/loading/error states exist as shared components; About + bootstrap have error + retry.
- [x] Keyboard: segmented controls are radio groups with arrow-key support; focus ring from theme.
- [x] No placeholder screens: unbuilt screens are disabled "Soon" items, not routes.
- [ ] Visual review in light/dark/compact — **not done by Claude** (no screenshot possible); owner to check with `pnpm tauri dev`.

### F. Docs
- [x] Tracker, ARCHITECTURE (layout, commands, error mode, navigation rule), DATABASE (path), DESIGN_SYSTEM (enforcement, motion), PRD (script name), DECISIONS (ADR-004/006 accepted, ADR-008/009 added), BUGS, CHANGELOG, README updated.

## 4. Bugs found

| BUG ID | Sev | Summary | Regression guard | Status |
|---|---|---|---|---|
| BUG-001 | S1 | Bundle id ended in `.app` | config + ADR-009 | ✅ |
| BUG-002 | S1 | MSRV pin resolved vulnerable sqlx/SQLite | cargo audit gate | ✅ (1 documented exception) |
| BUG-003 | S2 | Component created during render; setState in effect | react-hooks lint rules | ✅ |
| BUG-004 | S2 | Out-of-order settings saves rolled UI back to stale state | `settings.test.ts` BUG-004 | ✅ |
| BUG-005 | S3 | Motion bypassed theme tokens | check in DESIGN_SYSTEM §6 | ✅ |
| BUG-006 | S3 | Unreachable clippy ban path warning | `allow-invalid` | ✅ |
| BUG-007 | S2 | Info logs silently dropped by filter (found by launching the real app) | `bug_007_…` test | ✅ |

## 5. Process deviations (honest record)

- **TDD order:** R-CMD-01, BUG-004 and BUG-007 were written red-first and confirmed failing for the right reason. Most other Phase 1 tests were written in the same pass as the code rather than strictly before it. From Phase 2 on, each task's tests are committed red before implementation.
- **CI not executed:** the workflow exists but the folder is not a git repository yet.

## 6. Deferred items

| Item | Sev | Reason | Target |
|---|---|---|---|
| NFR-032 corrupted DB → backup/recreate | S2 | Needs `safe_fs` | P4-T09 |
| Remove ADR-008 ignore | S2 | Waiting for sqlx release allowing libsqlite3-sys ≥ 0.39 | Re-check every phase |
| Visual QA of shell | S3 | No screenshot capability here | Owner, now |

## 7. Sign-off

- [x] All S0/S1 fixed (BUG-002 residual accepted via ADR-008, pending owner ack)
- [x] All gates green after fixes
- [x] Tracker updated
