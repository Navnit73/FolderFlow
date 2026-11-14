# Phase 8 Audit — Quality assurance

**Date:** 2026-11-14
**Scope:** End-to-end Phase 8 quality assurance, performance benchmarking, security & dependency hardening, WCAG 2.1 AA accessibility verification, and full Phases 1–8 regression re-audit (`P8-T01`..`P8-T07`, `R-FAIL-01`..`12`, `E2E-01`..`06`, `BENCH-01`..`07`, `UI-A11Y-01`, `BUG-035`, `BUG-036`).

---

## 1. Gate results (final run after all code & config fixes)

| Gate | Command | Result |
|---|---|---|
| Bindings generation & check | `pnpm bindings` (`UPDATE_BINDINGS=1 cargo test --lib r_cmd_01`) | ✅ `src/bindings.ts` up to date (`r_cmd_01_bindings_are_up_to_date` passed) |
| Frontend lint | `pnpm lint` (`eslint . --max-warnings 0`) | ✅ 0 errors, 0 warnings |
| Typecheck | `pnpm typecheck` (`tsc --noEmit -p tsconfig.json`) | ✅ 0 errors |
| Frontend unit & token tests | `pnpm test` (`vitest run`) | ✅ 19 test files, 109 passed (0 failed, 0 skipped) |
| Production build | `pnpm build` (`vite build`) | ✅ Clean production bundle |
| Token enforcement | `pnpm check:tokens` | ✅ Passed (zero hardcoded styling outside `src/styles/theme.css`) |
| Playwright E2E + Axe A11y | `pnpm test:e2e` (`playwright test`) | ✅ 5 specs passed (`E2E-01`..`06` + `P8-T06` WCAG 2.1 AA across 20 routes + onboarding in Light & Dark modes, 0 violations) |
| Rust formatting | `cargo fmt --all -- --check` | ✅ Diff clean |
| Rust clippy | `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings (`folderflow`, `ff-macos-events`, `failure_suite`, `folderflow_benches`, `bench_idle_monitor`) |
| Rust suite (+proptest 10k) | `PROPTEST_CASES=10000 cargo test --workspace` | ✅ 336 passed (`324` lib + `12` `failure_suite`), 0 failed, 0 ignored |
| Flake check (10×) | `for i in $(seq 1 10); do cargo test --workspace --quiet; done` | ✅ 10/10 runs passed (`324 + 12 = 336` tests each run) |
| Code coverage | `cargo llvm-cov --summary-only` | ✅ Total `86.63%` lines (`24,085 / 27,803`): `engine/` `92.90%` (`9,741 / 10,486`), `rules/` `95.37%` (`4,742 / 4,972`), `engine/safe_fs/` `95.07%` (`579 / 609`) |
| Supply-chain policy | `cargo deny check` | ✅ `advisories ok, bans ok, licenses ok, sources ok` |
| Vulnerability audit | `cargo audit` (591 crates) + `pnpm audit --prod` | ✅ 0 vulnerabilities (`cargo audit`: 3 informational/upstream allowed warnings `paste`, `proc-macro-error`, `glib`, plus documented SQLite `RUSTSEC-2026-0459` per `ADR-008`; `pnpm audit --prod`: No known vulnerabilities found) |
| Benchmarks | `cargo bench --bench folderflow_benches` + `bench_idle_monitor` | ✅ `BENCH-01`..`07` measured and recorded in [../BENCHMARKS.md](../BENCHMARKS.md) |

---

## 2. Requirement coverage

| Task / Req | Description | Verified by |
|---|---|---|
| `P8-T01` | Rust line coverage targets: `engine/` `>= 85%`, `rules/` `>= 85%`, `engine/safe_fs/` `>= 95%` | `cargo llvm-cov --summary-only` (`engine/` `92.90%`, `rules/` `95.37%`, `engine/safe_fs/` `95.07%`; every file in `engine/` `>= 87.70%` and every file in `rules/` `>= 86.92%`) |
| `P8-T02` | Spec §24.3 failure suite (`R-FAIL-01`..`12`): interrupted move, interrupted copy, corrupted DB + ledger failure, destination unavailable, vanished source, mid-exec colliding dest, `ENOSPC`, `EACCES`, corrupted/unsafe config, watcher error/restart, event flood/duplicate dedupe, active-file stability & stale snapshot guard | `src-tauri/tests/failure_suite.rs` (`12/12` passing) |
| `P8-T03` | Playwright E2E workflows (`E2E-01`..`06`): 6-step zero-write onboarding, folder add/scope toggle/rescan, rule create/validation/test/edit, selective preview + `⌘↩` execute, review resolve + CSV export + Undo Last, Settings Appearance/Density + shortcuts + offline `FF1.*` license activation | `tests/e2e/app.spec.ts` (`4` workflow specs over `tests/e2e/helpers/tauriMock.ts`) |
| `P8-T04` | Criterion benchmarks at `1k / 10k / 50k / 100k` files (`BENCH-01`..`05`) plus `/usr/bin/time -l` heavy RSS (`BENCH-06`) and FSEvents idle CPU/RSS (`BENCH-07`); initial `NFR-022` targets set from empirical baselines | `src-tauri/benches/folderflow_benches.rs`, `src-tauri/examples/bench_idle_monitor.rs`, [../BENCHMARKS.md](../BENCHMARKS.md) |
| `P8-T05` | Security audit: least-privilege `capabilities/default.json` (zero `fs`/`shell`/`http`/`opener`), strict `tauri.conf.json` CSP (`freezePrototype: true`, `object-src 'none'`, `frame-ancestors 'none'`, `form-action 'none'`), static mutating-FS / shell / network `grep` + `clippy.toml` enforcement, `PathAuthority` proptests (`10k` cases), `deny.toml` + `cargo deny check` + `cargo audit` + `pnpm audit --prod` | Directly inspected configs, `cargo deny check`, `cargo audit`, `pnpm audit --prod`, `PROPTEST_CASES=10000` |
| `P8-T06` | WCAG 2.1 AA accessibility (`NFR-043`, `DESIGN_SYSTEM.md §7`): token contrast matrix (`>= 4.5:1`) across Light/Dark + composited `bg-selected` & subtle badges (`UI-A11Y-01`), `@axe-core/playwright` (`wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa`) across all 20 routes + onboarding in both Light & Dark modes, accessible `<Switch>` & `SettingRow` labels, landmark `<main>` + `<nav aria-label="Main">`, keyboard `Tab` focus | `scripts/check-tokens.test.ts` (`UI-A11Y-01`), `src/pages/categories/categories.test.tsx`, `tests/e2e/app.spec.ts` (`P8-T06`: `0` violations across all 20 routes in Light & Dark) |
| `P8-T07` | Full Phases 1–8 regression re-audit, Final Acceptance §30 (`A01`..`A21` verified; `A22` Phase 9 signing/notarization), `ADR-008` SQLite re-check, and documentation synchronization | All gates above + updated tracker/docs below |

---

## 3. Checklist results

### A. Data safety (`SAFETY_MODEL.md` S1–S11)
- [x] Static `grep` across `src-tauri/src/` and `src-tauri/crates/` confirmed **zero** direct mutating filesystem calls (`std::fs::{rename,remove_file,remove_dir,remove_dir_all,copy,write}`, `tokio::fs::*`) outside `engine::safe_fs`, `clippy.toml` ban declarations, and test fixtures (`test_support.rs`, test modules).
- [x] `engine/safe_fs/` achieved `95.07%` line coverage (`579 / 609` lines), including `FaultyFs` fault injection across every move/copy/remove/trash step and `r_fail_01`..`08` failure tests.
- [x] `PROPTEST_CASES=10000` passed `r_path_prop_never_escapes_root`, `r_conf_05_proptest_concurrent_conflicting_moves_never_overwrite_or_lose_files`, `r_tpl_prop_output_never_contains_traversal_or_invalid_chars`, and `r_grp_05_proptest_boolean_algebra_laws`.

### B. Correctness
- [x] All `12` spec §24.3 scenarios (`R-FAIL-01`..`12`) verified deterministically without sleep-flaky hacks.
- [x] All `5` Playwright specs (`E2E-01`..`06` and `P8-T06`) pass cleanly over built `dist/` via `vite preview`.
- [x] Re-audit fixed `tests/failure_suite.rs` (`BUG` in test harness: `MutexGuard` held across `.await` flagged by `clippy::await-holding-lock`).

### C. Concurrency & resources
- [x] `cargo clippy --all-targets --all-features -- -D warnings` verified zero `await_holding_lock` or warning issues across production code, tests, examples, and benches.
- [x] `BENCH-06` measured `146.7 MiB` max RSS during `bench_01_scan/100000` (500-item streaming batches) and `259.4 MiB` max RSS when materializing `100k` `FileFacts` structs.
- [x] `BENCH-07` measured `< 0.1%` steady-state idle CPU (`+9.78M` retired instructions over `10.04 s` real wall time; `13.9 MiB` RSS) with real macOS FSEvents watching.
- [x] `10/10` consecutive `cargo test --workspace` runs passed with zero flakes (`336` tests/run).

### D. Security & supply chain
- [x] `src-tauri/capabilities/default.json` grants only `core:app:default`, `core:event:default`, `core:window:default`, and `core:window:allow-start-dragging` (`NFR-011`).
- [x] `src-tauri/tauri.conf.json` enforces `default-src 'self'`, `script-src 'self'`, `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`, `form-action 'none'`, `freezePrototype: true` (`NFR-012`).
- [x] Static `grep` confirmed zero `Command::new` subprocess invocations and zero `reqwest` usage outside `security/license.rs` (`NFR-010`).
- [x] `cargo deny check` passed (`advisories ok, bans ok, licenses ok, sources ok`); pinned `ff-macos-events` version `"0.1.0"` to eliminate wildcard path ban; documented transitive `paste` (`RUSTSEC-2024-0436`, no upstream upgrade available).
- [x] `ADR-008` re-check (`2026-11-14`): `cargo search sqlx --limit 3` confirms `0.9.0` remains the latest stable `sqlx` release, so `RUSTSEC-2026-0459` exception remains documented in `.cargo/audit.toml`, `deny.toml`, and `docs/DECISIONS.md`.
- [x] `pnpm audit --prod`: `No known vulnerabilities found`.

### E. UI / theme & Accessibility (`NFR-043`, `BUG-035`, `BUG-036`)
- [x] `UI-A11Y-01` verifies `>= 4.5:1` contrast across Light and Dark text, subtle badges (composited over both `--ff-bg`/`--ff-bg-sidebar`/`--ff-bg-subtle`/`--ff-bg-muted` and `--ff-bg-selected`), solid action buttons (`--ff-danger-solid`), and all 13 category tokens.
- [x] `P8-T06` `@axe-core/playwright` (`wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa`) passes with **0 violations** across Onboarding plus all `20` routed views (including all 9 `/settings/*` tabs) in both Light and Dark themes.
- [x] All `Switch` controls across `CategoriesPage` and `Settings/*` (`SettingRow htmlFor`) expose accessible labels for screen readers and clickable labels.

### F. Docs
- [x] Created [../BENCHMARKS.md](../BENCHMARKS.md) (`BENCH-01`..`07` + initial `NFR-022` regression budgets).
- [x] Updated [../PLAN_TRACKER.md](../PLAN_TRACKER.md), [../TESTING_STRATEGY.md](../TESTING_STRATEGY.md), [../BUGS.md](../BUGS.md) (`BUG-035`, `BUG-036`), [../DECISIONS.md](../DECISIONS.md) (`ADR-018`), [../ARCHITECTURE.md](../ARCHITECTURE.md), [../CHANGELOG.md](../CHANGELOG.md), and [../../README.md](../../README.md).

---

## 4. Bugs found and fixed during Phase 8

| ID | Severity | Area | Summary | Regression guard |
|---|---|---|---|---|
| `BUG-035` | S2 | `src/styles/theme.css`, `src/components/ui/button.tsx` | Light and Dark semantic tokens (`--ff-fg-subtle`, `--ff-accent`, Dark `--ff-danger` solid button fill, status badge colors, and Light category tokens) failed WCAG 2.1 AA `< 4.5:1` contrast. Adjusted palette/semantic tokens and split `--ff-danger-solid` / `--ff-danger-solid-hover`. | `scripts/check-tokens.test.ts` (`UI-A11Y-01`) |
| `BUG-036` | S2 | `src/pages/categories/CategoriesPage.tsx`, `src/pages/settings/SettingRow.tsx`, `src/pages/settings/*.tsx`, `src/styles/theme.css` | Category enable switches (`#cat-enable-*`) lacked accessible labels (`button-name` critical WCAG 4.1.2 violation); `SettingRow` rendered `<p>` instead of `<label htmlFor>` leaving Settings switches unlabeled; Dark-mode `--ff-blue-400`, `--ff-red-400`, and `--ff-fg-subtle` fell to `4.19:1`–`4.30:1` when rendered over `--ff-bg-selected` + `-subtle` badge overlays. Added `<label>` associations, optional `ariaLabel` on `Switch`, lightened Dark blue/red/subtle tokens, and extended `UI-A11Y-01` to verify `--ff-bg-selected` overlays. | `categories.test.tsx`, `scripts/check-tokens.test.ts` (`UI-A11Y-01`), `tests/e2e/app.spec.ts` (`P8-T06`) |

---

## 5. Deferred / Phase 9 items

1. **Online licensing vendor decision (`Q1`)** — `UnconfiguredOnlineProvider` remains active until vendor integration details are provided (`ADR-017`).
2. **Upstream crate advisories** — `RUSTSEC-2026-0459` (SQLite in `sqlx 0.9.0`, `ADR-008`), `RUSTSEC-2024-0436` (`paste` unmaintained), `RUSTSEC-2024-0370` (`proc-macro-error` unmaintained), `RUSTSEC-2024-0429` (`glib` Linux-only transitive gtk dep).
3. **Phase 9 release deliverables (`A22`, `Q3`, `Q5`)** — Brand icon assets (`Q5`), Universal binary build, Developer ID signing / hardened runtime / entitlements & notarization (`Q3`), DMG installer, clean macOS 13+ VM smoke tests, and `docs/USER_GUIDE.md`.

---

## 6. Sign-off

- [x] All S0/S1 bugs resolved; Phase 8 S2 accessibility & token bugs fixed and regression-tested (`BUG-035`, `BUG-036`)
- [x] All gates green (`336` Rust tests with `PROPTEST_CASES=10000`, `10/10` flake runs, `109` Vitest tests, `5` Playwright E2E/A11y specs, `cargo llvm-cov` targets exceeded, `cargo deny check`, `cargo audit`, `pnpm audit --prod`)
- [x] Final acceptance criteria §30 `A01`–`A21` verified ✅ (`A22` remains for Phase 9 signing/notarization)

