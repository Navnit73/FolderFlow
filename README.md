# FolderFlow

**Your files organized automatically.** An offline-first, privacy-first automatic file organizer for macOS 13+, built with Tauri 2, Rust and React.

> Status: **v1.0.0 — Phase 9 (Production release) release engineering & smoke checks complete** (live Developer ID signing/notarization `Q3` gated on Apple Developer credentials). See [docs/PLAN_TRACKER.md](docs/PLAN_TRACKER.md).

## Documentation

| | |
|---|---|
| [User guide](docs/USER_GUIDE.md) | Installation, onboarding, rules, undo, troubleshooting, VM runbook & uninstall |
| [Release notes](RELEASE_NOTES.md) | v1.0.0 highlights, system requirements & SHA-256 verification |
| [PRD](docs/PRD.md) | What we're building and why |
| [Plan tracker](docs/PLAN_TRACKER.md) | Phase-by-phase progress |
| [Architecture](docs/ARCHITECTURE.md) | Modules, commands, routes |
| [Database](docs/DATABASE.md) | SQLite schema |
| [Safety model](docs/SAFETY_MODEL.md) | How files are never lost or overwritten |
| [Testing strategy](docs/TESTING_STRATEGY.md) | TDD process and test catalog |
| [Benchmarks](docs/BENCHMARKS.md) | `1k`–`100k` empirical baselines & `NFR-022` budgets |
| [Audit process](docs/AUDIT_PROCESS.md) | End-of-phase bug re-audit |
| [Design system](docs/DESIGN_SYSTEM.md) | UI rules; theme lives in [`src/styles/theme.css`](src/styles/theme.css) |
| [Decisions](docs/DECISIONS.md) · [Bugs](docs/BUGS.md) · [Changelog](docs/CHANGELOG.md) | |

## Development

Prerequisites: Rust ≥ 1.94, Node ≥ 20 (24 recommended — `check:tokens` runs TypeScript directly), pnpm, Xcode Command Line Tools.

```bash
pnpm install
pnpm tauri dev          # run the app (Vite on :1420 + Rust)
pnpm bindings           # regenerate src/bindings.ts after changing Rust commands/types
```

Gates (all must pass before a task is done — see [CLAUDE.md](CLAUDE.md)):

```bash
pnpm gates && pnpm test:e2e && pnpm audit --prod
cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --workspace && cargo deny check && cargo audit
```

`pnpm build` must run before the first `cargo` build: Tauri embeds `dist/` at compile time.

App data: `~/Library/Application Support/com.folderflow.desktop/` · Logs: `~/Library/Logs/com.folderflow.desktop/`

### Troubleshooting

**`ld: tapi error: … unknown architecture arm64e.x1-macos`** — your Command Line Tools linker is older than the versioned macOS SDK selected by `xcrun` (ENV-001). `src-tauri/.cargo/config.toml` sets `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk` automatically for Cargo builds; if running outside Cargo, either update Command Line Tools / Xcode or export:

```bash
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk
```
