# Phase 9 Audit — Production release

**Date:** 2026-11-14
**Scope:** End-of-phase release engineering, branding, universal macOS bundle, entitlements & hardened runtime configuration, DMG packaging, SemVer `1.0.0` bump, documentation (`USER_GUIDE.md`, `RELEASE_NOTES.md`, `CHANGELOG.md`, `README.md`, `TESTING_STRATEGY.md`), CI release workflow (`.github/workflows/release.yml`), and clean-environment smoke verification (`P9-T01`..`P9-T08`).

---

## 1. Gate results (final run after all code and doc edits)

| Gate | Command | Result |
|---|---|---|
| Bindings generation & check | `pnpm bindings && git diff --exit-code src/bindings.ts` | ✅ Passed (`r_cmd_01_bindings_are_up_to_date`, `1 passed; 0 failed`) |
| Lint | `pnpm lint` (`eslint . --max-warnings 0`) | ✅ 0 errors, 0 warnings |
| Typecheck | `pnpm typecheck` (`tsc --noEmit -p tsconfig.json`) | ✅ 0 errors |
| Frontend tests | `pnpm test` (`vitest run`) | ✅ 19 test files, 109 tests passed |
| Build | `pnpm build` (`vite build`) | ✅ Clean production bundle (`2245` modules) |
| Token enforcement | `pnpm check:tokens` (`node scripts/check-tokens.ts`) | ✅ Passed (all UI styling from `src/styles/theme.css`) |
| E2E Playwright suite | `pnpm test:e2e` (`playwright test`) | ✅ 5 passed (`18.6s`, Light & Dark `@axe-core/playwright` incl.) |
| Frontend dependency audit | `pnpm audit --prod` | ✅ No known vulnerabilities found |
| Rust format | `cargo fmt --all -- --check` | ✅ Diff clean |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings (`folderflow v1.0.0` + `ff-macos-events v1.0.0`) |
| Rust tests (+proptest 10k) | `PROPTEST_CASES=10000 cargo test --workspace` | ✅ `347` passed (`324` lib + `12` `failure_suite` + `11` `release_smoke`), `0` failed, `0` ignored |
| Rust supply-chain policy | `cargo deny check` | ✅ `advisories ok, bans ok, licenses ok, sources ok` |
| Rust advisory scan | `cargo audit` (591 crate dependencies) | ✅ `warning: 3 allowed warnings found` (`paste`, `proc-macro-error`, `glib`), `0` unallowed vulnerabilities (`ADR-008` `RUSTSEC-2026-0459` exception re-checked on `sqlx 0.9.0`) |
| Universal `.app` + `.dmg` build | `./scripts/release-macos.sh --skip-gates --target universal-apple-darwin` | ✅ Built `dist-release/FolderFlow_1.0.0_universal.dmg` (`18M`), `.app.tar.gz` (`17M`), and `SHA256SUMS.txt` (`lipo -archs` = `x86_64 arm64`; built unsigned `--no-sign` prior to test-only `cargo fmt` on `release_smoke.rs`) |
| Packaged `.dmg` clean-`HOME` smoke | `./scripts/smoke-test-macos.sh --dmg dist-release/FolderFlow_1.0.0_universal.dmg` | ✅ Mounted DMG, verified `x86_64 arm64`, launched first-launch & restart-launch in isolated `HOME`, verified clean uninstall keeps user files intact |

---

## 2. Requirement & task coverage

| Req / Task | Description | Verified by | Status |
|---|---|---|---|
| `P9-T01` (`Q5`) | Brand squircle icon (`assets/icon-source.svg`), `src-tauri/icons/*` (`32x32.png`, `64x64.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.icns`), `assets/dmg-background.png` (`660×400`), generator script `scripts/render-brand-assets.mjs` | `r_rel_02_brand_icons_and_dmg_assets_valid`, visual inspection | ✅ |
| `P9-T02` (`NFR-040` / Apple Silicon + Intel) | Universal binary (`x86_64-apple-darwin` + `aarch64-apple-darwin`), system framework dynamic linking check (`otool -L`) | `release-macos.sh`, `smoke-test-macos.sh` (`x86_64 arm64`) | ✅ |
| `P9-T03` (`Q3`) | Least-privilege empty `src-tauri/Entitlements.plist` (non-sandboxed per `ADR-002`, zero JIT/unsigned-memory exemptions), `hardenedRuntime: true` in `src-tauri/tauri.conf.json`, signing automation in `scripts/release-macos.sh` & `.github/workflows/release.yml` | `r_rel_01`, `r_smoke_01`; live Developer ID signing pending Keychain identity (`0 valid identities found` locally) | ⛔ `Q3` (config/scripts ready; live cert pending) |
| `P9-T04` (`Q3`) | Apple notarization & stapling pipeline (`xcrun notarytool` + `xcrun stapler`) with `if: always()` credential cleanup in `.github/workflows/release.yml` and `scripts/release-macos.sh` | Script & workflow inspection; live notarization pending Apple Developer credentials | ⛔ `Q3` (scripts ready; live credentials pending) |
| `P9-T05` | DMG installer window (`660×400`), background image, universal `.dmg` & `.sha256` checksum output | `r_rel_01`, `r_rel_02`, `smoke-test-macos.sh` | ✅ |
| `P9-T06` | SemVer `1.0.0` parity across `package.json`, `src-tauri/Cargo.toml`, `ff-macos-events/Cargo.toml`, `src-tauri/Cargo.lock`, `tauri.conf.json`, frontend fixtures (`src/test/fixtures.ts`, `tests/e2e/helpers/tauriMock.ts`), `docs/CHANGELOG.md`, `RELEASE_NOTES.md` | `r_rel_01_version_and_bundle_manifests_match`, `r_rel_03_user_guide_and_frontend_version_parity` | ✅ |
| `P9-T07` | Automated clean-environment smoke suite (`R-SMOKE-01..08`), isolated-`HOME` `.dmg` launch script (`scripts/smoke-test-macos.sh`), and macOS 13 / latest VM verification checklist in `docs/USER_GUIDE.md` | `r_smoke_01`..`08` (8/8), `smoke-test-macos.sh` (`✓`); external macOS 13 clean VM interactive run deferred to VM host | 🟨 Local ✅ · macOS 13 VM run pending |
| `P9-T08` | End-user guide covering install, onboarding, rules, preview/undo, offline licensing, troubleshooting, clean VM checklist & uninstall | `docs/USER_GUIDE.md`, `r_rel_03_user_guide_and_frontend_version_parity` | ✅ |

---

## 3. Checklist results

### A. Data safety (`SAFETY_MODEL.md` S1–S11)
- [x] `r_smoke_03_onboarding_zero_writes_and_starter_rules_disabled` confirms first-time onboarding creates zero file mutations and leaves starter rules disabled until explicitly confirmed.
- [x] `r_smoke_04_organize_moves_file_safely_with_ledger_and_activity` and `r_smoke_05_undo_restores_original_file_and_prunes_empty_dest` confirm ledger-backed safe moves and complete undo restoration.
- [x] `r_smoke_08_uninstall_removes_app_data_and_leaves_user_files_intact` and `scripts/smoke-test-macos.sh` verify clean uninstall removes only app support/log directories while leaving user files in monitored folders untouched.
- [x] Runtime FS mutation remains isolated to `engine::safe_fs` (non-test code).

### B. Correctness
- [x] `r_smoke_02` and `r_smoke_06` confirm fresh DB initialization, WAL migrations, default settings, log creation, and state/onboarding persistence + `recover_startup` across restarts.
- [x] `scripts/smoke-test-macos.sh` validates clean DMG mount, `.background` asset presence, `.icns` check, universal binary architectures (`x86_64 arm64`), isolated `HOME` first launch & restart launch, DB + log creation, and uninstall preservation.

### C. Concurrency & resources
- [x] All `12` crash/fault/backpressure integration tests (`tests/failure_suite.rs`) and `324` unit/property tests (`PROPTEST_CASES=10000`) pass without regression.
- [x] Playwright Chromium asset rendering runs purely as a build-time dev script (`scripts/render-brand-assets.mjs`), leaving zero runtime browser or helper processes in the `.app` bundle.

### D. Security
- [x] `src-tauri/Entitlements.plist` is least-privilege empty `<dict/>` (zero `allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation`, or unnecessary entitlements); `hardenedRuntime: true` enabled in `src-tauri/tauri.conf.json`.
- [x] `src-tauri/capabilities/default.json` retains zero `fs`, `shell`, `http`, or `opener` webview capabilities.
- [x] `git grep -n "std::process::Command\|tokio::process::Command\|reqwest" -- src-tauri/src/ src-tauri/crates/` confirms zero shell command invocations in runtime crates and `reqwest::Client` only in `src-tauri/src/security/license.rs`; `r_smoke_07` verifies offline Ed25519 activation and core organization work with zero network access.
- [x] `git grep`/CI release workflow uses `if: always()` temporary keychain cleanup in `.github/workflows/release.yml`.

### E. UI / theme
- [x] `pnpm check:tokens` passes; Vitest (`109` tests) and Playwright (`5` E2E specs including Light & Dark `@axe-core/playwright`) pass after the `1.0.0` version bump across fixtures and mocks.
- [x] `r_rel_02` validates all configured `.png`/`.icns` brand icons and DMG background dimensions (`660×400`).

### F. Docs
- [x] Created `RELEASE_NOTES.md` and `docs/USER_GUIDE.md`; updated `docs/CHANGELOG.md` (`[1.0.0]`), `docs/DECISIONS.md` (`ADR-019`), `README.md`, `docs/TESTING_STRATEGY.md`, `docs/PLAN_TRACKER.md`, and `docs/audits/phase-9.md`.

---

## 4. Bugs found during Phase 9

None in product runtime code. Formatting drift introduced when authoring `src-tauri/tests/release_smoke.rs` was caught by `cargo fmt --all -- --check`, fixed via `cargo fmt --all`, and re-verified with the full Rust and macOS smoke suites.

---

## 5. Deferred / owner actions

1. **Developer ID signing, Apple notarization, and Gatekeeper stapling (`Q3`, `P9-T03`, `P9-T04`, `A22`)**: No code-signing identity is installed in this environment (`security find-identity -v -p codesigning` → `0 valid identities found`). Once Apple Developer ID Application certificate and App Store Connect notary credentials are configured (locally or in GitHub Secrets), run `./scripts/release-macos.sh` or tag `v1.0.0` to trigger `.github/workflows/release.yml` and verify `codesign --verify --deep --strict` + `spctl --assess --type exec`.
2. **External clean macOS 13 (Ventura) + latest macOS VM interactive smoke pass (`P9-T07`)**: No hypervisor (`tart`, `utmctl`, `qemu-system-aarch64`) is installed on this host (`macOS 27.0.1` `arm64`). Follow the interactive VM checklist in `docs/USER_GUIDE.md` §12 on clean macOS 13 and latest macOS VMs once signed/notarized `.dmg` artifacts are produced.
3. **Upstream crate advisories & `Q1` online licensing**: Same documented status as Phase 7 & 8 (`paste`, `proc-macro-error`, `glib` allowed warnings; `ADR-008` `RUSTSEC-2026-0459` SQLite exception on `sqlx 0.9.0`; `Q1` online licensing vendor still optional behind offline Ed25519).

---

## 6. Sign-off

- [x] All S0/S1/S2/S3 product issues clear
- [x] All automated gates green after final edits (`347` Rust tests, `109` Vitest tests, `5` Playwright E2E tests, `cargo deny`, `cargo audit`, `pnpm audit`, `smoke-test-macos.sh`)
- [x] `Q3` signing/notarization and external macOS 13 VM interactive verification honestly documented in `docs/PLAN_TRACKER.md` and `docs/audits/phase-9.md`

