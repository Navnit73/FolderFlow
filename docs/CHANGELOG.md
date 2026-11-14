# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: SemVer.

## [Unreleased]

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
