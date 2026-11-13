# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: SemVer.

## [Unreleased]

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
