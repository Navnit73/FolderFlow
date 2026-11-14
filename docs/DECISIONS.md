# Architecture Decision Records

Append-only. Status: Proposed → Accepted → Superseded (by ADR-x).

| ADR | Title | Status | Date |
|---|---|---|---|
| 001 | Tauri 2 + Rust core + React UI | Accepted | 2026-11-13 |
| 002 | Direct distribution (Developer ID), not Mac App Store | Accepted | 2026-11-13 |
| 003 | Single theme file controls all UI | Accepted | 2026-11-13 |
| 004 | Generate TS bindings from Rust (tauri-specta) | Accepted | 2026-11-13 |
| 005 | No-overwrite primitives via `renamex_np(RENAME_EXCL)` | Accepted | 2026-11-13 |
| 006 | SQLx with runtime-checked queries + migrations | Accepted | 2026-11-13 |
| 007 | pnpm as package manager | Accepted | 2026-11-13 |
| 008 | Temporary cargo-audit exception RUSTSEC-2026-0459 (bundled SQLite) | Accepted — revisit every phase | 2026-11-13 |
| 009 | Toolchain pins: Tauri 2.x, TS 6.0, Rust MSRV 1.94, bundle id `com.folderflow.desktop` | Accepted | 2026-11-13 |
| 010 | `notify` 8.x (FSEvents) with our own debounce/stability, not notify-debouncer | Accepted | 2026-11-13 |
| 011 | macOS APIs via `objc2` (safe bindings), never by spawning processes | Accepted | 2026-11-13 |
| 012 | IPC numbers for bytes/timestamps exported as TS `number` (`#[specta(type = f64)]`) | Accepted | 2026-11-13 |
| 013 | Monitored folders may not overlap | Accepted | 2026-11-13 |
| 014 | First scan and scope changes are a baseline, not "new files" | Accepted | 2026-11-13 |
| 015 | Pure rule engine (`regex`, `glob`, `chrono`) over `FileFacts` and injectable `Clock` | Accepted | 2026-11-14 |
| 016 | `blake3` streaming content verification and `safe_fs` capability tokens | Accepted | 2026-11-14 |
| 017 | Native macOS Keychain, Launch at Login, UNUserNotificationCenter, NSWorkspace bridge, and Ed25519 licensing | Accepted | 2026-11-14 |
| 018 | Phase 8 QA harness: Criterion benchmarks, Playwright E2E + axe-core WCAG matrix, and `cargo-deny` policy | Accepted | 2026-11-14 |
| 019 | Phase 9 production release: SemVer `1.0.0`, universal binary, least-privilege Hardened Runtime entitlements, reproducible brand assets, and gated Developer ID signing/notarization | Accepted | 2026-11-14 |

---

### ADR-001 — Tauri 2 + Rust core + React UI
Spec-mandated. Small binary, Rust owns filesystem logic, web UI for fast iteration.

### ADR-002 — Direct distribution, not Mac App Store
**Context:** MAS requires App Sandbox; monitoring arbitrary user folders and external drives under the sandbox needs security-scoped bookmarks for every folder and complicates background operation and login items.
**Decision:** Ship Developer-ID-signed, notarized DMG with hardened runtime, non-sandboxed. Rely on TCC prompts for Desktop/Documents/Downloads and the user-selected folder picker. Still store folder paths only after explicit user selection (least privilege by convention).
**Consequence:** MAS later would require a bookmark layer; `PathAuthority` is the single place that would change.

### ADR-003 — Single theme file
**Decision:** `src/styles/theme.css` holds every design value (colors light/dark, typography, spacing, radii, density, motion, z-index, category/status colors) as CSS custom properties, and maps them into Tailwind v4 via `@theme inline`. Components use only token-backed utilities (`bg-surface`, `text-fg-muted`, `rounded-md`). `scripts/check-tokens.ts` fails CI on hex/rgb/hsl literals, Tailwind palette classes (`text-red-500`) and arbitrary values (`p-[13px]`) outside that file.
**Consequence:** Rebranding or re-theming = editing one file.

### ADR-004 — TS bindings from Rust
**Decision:** tauri-specta `=2.0.0-rc.29` + specta `=2.0.0-rc.28` + specta-typescript `=0.0.15`, pinned exactly (pre-release; upstream requires `=` pins). `ErrorHandlingMode::Result` so errors are typed `UiError` values, not thrown. `src/bindings.ts` is committed; R-CMD-01 fails if it drifts; regenerate with `pnpm bindings`. u64/i64 are rejected by the exporter (BigInt safety); see ADR-012.
**Fallback if specta breaks on a Tauri update:** `ts-rs` for types + a thin hand-written invoke wrapper, keeping R-CMD-01.

### ADR-005 — No-overwrite primitives
`std::fs::rename` maps to `rename(2)`, which replaces an existing destination. Use `renamex_np(from, to, RENAME_EXCL)` (macOS 10.12+) via `libc`; file creation via `OpenOptions::create_new(true)`. Ban the std variants via clippy outside `safe_fs`.

### ADR-006 — SQLx runtime queries
**Decision:** sqlx 0.9, `sqlx::migrate!` (embedded at compile time) + runtime `query`/`query_as`, every repository function covered by tests against in-memory and temp-file DBs. Compile-time `query!` macros rejected for now (need live DB or `.sqlx` offline data in CI). Revisit if query bugs appear.

### ADR-007 — pnpm
Faster, strict dependency resolution. Available on the dev machine.

### ADR-008 — Temporary audit exception: RUSTSEC-2026-0459
**Context:** SQLite < 3.54 (libsqlite3-sys < 0.39) can return uninitialised memory when a process opens an attacker-crafted database together with a crafted `-wal` file. The newest sqlx-sqlite (0.9.0) requires libsqlite3-sys `<0.38`, so no released sqlx can take the fix.
**Assessment:** FolderFlow opens exactly one SQLite file: its own DB in its private app-data directory, created by itself. It never opens user-supplied SQLite files (import/export is versioned JSON, FR-191). Planting a crafted DB/WAL there already requires write access to the user's Library, i.e. full account compromise. Residual risk: low.
**Decision:** ignore the advisory in `src-tauri/.cargo/audit.toml` with a pointer here.
**Re-checked 2026-11-13 (Phase 2 audit):** latest sqlx is still 0.9.0 → exception stays.
**Exit:** every phase audit re-checks for an sqlx release allowing libsqlite3-sys ≥ 0.39 and removes the ignore immediately. Do not add features that open foreign SQLite files while this exception exists.

### ADR-009 — Toolchain pins
- **Tauri 2.x** (crate 2.12): Tauri 3 is alpha; the spec mandates Tauri 2.
- **TypeScript ~6.0**: TS 7 (native compiler) is out, but typescript-eslint supports `<6.1`.
- **Rust MSRV 1.94**: required by sqlx 0.9. With a lower `rust-version`, Cargo's MSRV-aware resolver silently picked sqlx 0.8.6, which pulled a vulnerable SQLite and an `rsa` advisory.
- **Bundle identifier `com.folderflow.desktop`**: identifiers ending in `.app` conflict with the macOS bundle extension (Tauri rejects/warns). Planning docs said `com.folderflow.app`; corrected.

### ADR-010 — Watcher: notify 8 + own pipeline
notify 9 is a release candidate; 8.x is stable and uses FSEvents on macOS. We do not use `notify-debouncer-*`: debounce, dedupe, stability and overflow handling are explicit, tested state machines (`engine/debounce.rs`, `engine/stability.rs`) because every event is re-stat'ed anyway and overflow must fall back to a reconcile, not be dropped. The watcher is created through an injectable `WatcherFactory` so supervisor/backoff behaviour is testable.

### ADR-011 — macOS APIs via objc2
Opening System Settings (and later Reveal in Finder, sleep/wake notifications) uses `objc2-app-kit`/`objc2-foundation`, whose bindings for these calls are safe Rust — no `unsafe`, no `Command`, no opener plugin. URLs are compile-time constants.

### ADR-012 — Large numbers over IPC
specta rejects `i64`/`u64` (BigInt safety). Byte counts and epoch-millisecond timestamps are exported with `#[specta(type = f64)]` and arrive as JS numbers — exact up to 2^53 (≈ 9 PB / year 287 396). Generated TS types are `number | null` (JSON maps NaN/∞ to null); the UI treats null as 0.

### ADR-013 — Monitored folders may not overlap
Adding a folder inside (or containing) an already-monitored folder is rejected. Overlap would let two folders' rules act on the same file and double-process events. Users who want different behaviour for a subfolder use exclusions or rule conditions instead.

### ADR-014 — Baseline semantics
The monitor reports only files that are new or changed since the folder's last complete scan. The first scan, and any change to a folder's scope (recursive, include/exclude extensions, excluded subfolders), records a baseline and reports nothing, so enabling FolderFlow never mass-processes files that were already there. See SAFETY_MODEL.md § New-file semantics.

### ADR-015 — Pure rule engine (`regex`, `glob`, `chrono`) over `FileFacts` and injectable `Clock`
**Decision:** The `rules` module performs zero filesystem or database I/O; it evaluates `ConditionNode` trees, renders path/naming templates, and runs rule chains purely over injected `FileFacts` and a `Clock` trait (`SystemClock` in production, `FixedClock` with configurable UTC offset and DST transitions in tests). Direct dependencies `regex 1` (linear-time NFA/DFA with explicit compile size limits for FR-048), `glob 0.3` (for FR-047), and `chrono 0.4` (for local calendar date calculations in FR-045/FR-070) are promoted from existing transitive dependencies in `Cargo.lock`.

### ADR-016 — `blake3` streaming content verification and `safe_fs` capability tokens
**Decision:** `engine::safe_fs` is the sole mutating filesystem module (enforced by `clippy.toml` `disallowed_methods`). It uses `blake3` 1.8 for 64 KiB streaming content hashing during `copy_excl` and post-copy verification (`verify_copied`). Unlinking a cross-volume source (`remove_verified_source`) requires a capability token (`VerifiedSourceToken`) that can only be constructed after `verify_copied` confirms the destination's size and BLAKE3 digest match the streamed copy and the source's `(dev, ino, size, mtime_ns)` is unchanged. Trash operations use `objc2_foundation::NSFileManager::trashItemAtURL_resultingItemURL_error` in production and an isolated temp `.Trash` directory in tests so automated tests never touch the user's `~/.Trash`.

### ADR-017 — Native macOS Keychain, Launch at Login, UNUserNotificationCenter, NSWorkspace bridge, and Ed25519 licensing
**Decision:**
- Keychain credentials use `security-framework` (`MacOsKeychain` in `security/keychain.rs`) behind `KeychainBackend` so tests run against `InMemoryKeychain` without touching the developer's login keychain.
- Launch at login uses Apple's modern `SMAppService` API via `smappservice-rs` (`system/login_item.rs`), guarded by an `NSBundle::mainBundle().bundleIdentifier()` check so unbundled `cargo test` binaries never fail trying to access main-app login item APIs.
- Native notifications use `objc2-user-notifications` + `block2` (`notifications/native.rs`) wired into `CompositeNotificationSink` with `UNUserNotificationCenter`, honoring system settings and app batching/cooldown rules.
- Sleep/wake and external volume mount/unmount observers (`NSWorkspace` notification center with `block2` callbacks producing `!Send` AppKit tokens kept on the main thread) live inside a dedicated leaf workspace crate (`src-tauri/crates/ff-macos-events`) so the main crate retains `#![deny(unsafe_code)]` while keeping all native lifecycle bridges clean and isolated.
- Offline licensing uses `ed25519-dalek` 3 + `base64` 0.22 (`FF1.*` tokens and `.fflicense` JSON documents, verified locally with zero network calls); optional online licensing goes strictly through `reqwest` (rustls only, no default TLS) inside `security/license.rs` (`HttpJsonLicenseProvider`), with `UnconfiguredOnlineProvider` active until product online licensing vendor `Q1` is selected.

### ADR-018 — Phase 8 QA harness: Criterion benchmarks, Playwright E2E + axe-core WCAG matrix, and `cargo-deny` policy
**Decision:**
- Benchmarks (`P8-T04`, `NFR-022`/`023`) use `criterion` `0.8.2` (dev-dependency only, `src-tauri/benches/folderflow_benches.rs`) alongside a dedicated FSEvents idle probe (`src-tauri/examples/bench_idle_monitor.rs`) timed with macOS `/usr/bin/time -l`, keeping heavy multi-second loops outside `cargo test`.
- End-to-end UI workflows and accessibility audits (`P8-T03`, `P8-T06`) run via Playwright (`tests/e2e/app.spec.ts`) and `@axe-core/playwright` against the built `vite preview` bundle backed by a typed stateful `window.__TAURI_INTERNALS__` mock (`tests/e2e/helpers/tauriMock.ts`), plus an automated Light/Dark token + overlay contrast matrix in `scripts/check-tokens.test.ts` (`UI-A11Y-01`).
- Supply-chain policy (`P8-T05`) is enforced via `src-tauri/deny.toml` (`cargo deny check`) banning wildcard path deps and unknown registries/git sources, allowing curated OSS licenses for `aarch64-apple-darwin` / `x86_64-apple-darwin`, and tracking upstream `RUSTSEC-2026-0459` (`ADR-008`, `sqlx 0.9.0` re-verified 2026-11-14) and `RUSTSEC-2024-0436` (transitive unmaintained `paste 1.0.15`).

### ADR-019 — Phase 9 production release: SemVer `1.0.0`, universal binary, least-privilege Hardened Runtime entitlements, reproducible brand assets, and gated Developer ID signing/notarization
**Decision:**
- **SemVer `1.0.0` (`P9-T06`):** Bump all workspace and frontend package manifests (`package.json`, `src-tauri/Cargo.toml`, `src-tauri/crates/ff-macos-events/Cargo.toml`, `src-tauri/tauri.conf.json`, `Cargo.lock`, frontend test fixtures) to `1.0.0`, paired with `docs/CHANGELOG.md` and `RELEASE_NOTES.md`.
- **Brand & DMG assets (`P9-T01`, `P9-T05`):** Replace the interim SVG with a production macOS squircle icon (`assets/icon-source.svg`) and matching DMG background (`assets/dmg-background.png`), rendered deterministically via `scripts/render-brand-assets.mjs` (reusing the workspace's existing Playwright Chromium dependency — zero new npm packages) and `pnpm exec tauri icon`.
- **Universal binary (`P9-T02`):** Target `universal-apple-darwin` (`aarch64-apple-darwin` + `x86_64-apple-darwin`) with `minimumSystemVersion: "13.0"` and `NSSupportsAutomaticGraphicsSwitching: true` in `src-tauri/Info.plist`.
- **Least-privilege Hardened Runtime (`P9-T03`, `P9-T04`):** Enable `"hardenedRuntime": true` and `"entitlements": "Entitlements.plist"` in `src-tauri/tauri.conf.json`. Because FolderFlow uses direct non-sandboxed distribution (`ADR-002`), out-of-process system `WKWebView`, and statically linked Rust code without third-party plugins, `src-tauri/Entitlements.plist` intentionally grants **zero** sandbox or JIT/unsigned-memory/library-validation exemptions (`<dict/>`). Automated signing (`codesign`), notarization (`xcrun notarytool`), and stapling (`xcrun stapler`) live in `scripts/release-macos.sh` and `.github/workflows/release.yml`; live Developer ID signing/notarization remains gated until `Q3` Apple Developer ID credentials are supplied.
- **Verification (`P9-T07`, `P9-T08`):** `src-tauri/tests/release_smoke.rs` (`R-REL-01..03`, `R-SMOKE-01..08`) tests release manifest invariants and the 8 clean-environment lifecycle scenarios end-to-end in temp dirs, while `scripts/smoke-test-macos.sh` validates the packaged `.dmg` and universal `.app` bundle under an isolated temporary `HOME`. User documentation ships in `docs/USER_GUIDE.md`.
