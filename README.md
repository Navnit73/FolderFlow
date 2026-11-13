# FolderFlow

**Your files organized automatically.** An offline-first, privacy-first automatic file organizer for macOS 13+, built with Tauri 2, Rust and React.

> Status: **Phase 1 (foundation) complete.** Next: Phase 2 — filesystem engine. See [docs/PLAN_TRACKER.md](docs/PLAN_TRACKER.md).

## Documentation

| | |
|---|---|
| [PRD](docs/PRD.md) | What we're building and why |
| [Plan tracker](docs/PLAN_TRACKER.md) | Phase-by-phase progress |
| [Architecture](docs/ARCHITECTURE.md) | Modules, commands, routes |
| [Database](docs/DATABASE.md) | SQLite schema |
| [Safety model](docs/SAFETY_MODEL.md) | How files are never lost or overwritten |
| [Testing strategy](docs/TESTING_STRATEGY.md) | TDD process and test catalog |
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
pnpm gates                                   # lint, typecheck, test, build, check:tokens
cd src-tauri && cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test && cargo audit
```

`pnpm build` must run before the first `cargo` build: Tauri embeds `dist/` at compile time.

App data: `~/Library/Application Support/com.folderflow.desktop/` · Logs: `~/Library/Logs/com.folderflow.desktop/`

### Troubleshooting

**`ld: tapi error: … unknown architecture arm64e.x1-macos`** — your Command Line Tools linker is older than the default macOS SDK (ENV-001). Either update the Command Line Tools / Xcode, or point builds at an older installed SDK:

```bash
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
```
# FolderFlow
