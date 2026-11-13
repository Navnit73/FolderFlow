# FolderFlow — Architecture

> Source of truth for module boundaries, command interfaces, routes and shared components. Update in the same change as the code.

## 1. Principles

1. **Rust owns all filesystem work and business logic.** React renders state and sends intents.
2. **One mutating filesystem module** (`safe_fs`). Everything else gets a read-only view.
3. **Pure core, thin shell.** Rule evaluation, templates, planning and state transitions are pure functions over injected facts (`FileFacts`, `Clock`) so they are trivially testable.
4. **Bounded everything.** Every channel has a capacity; every worker pool has a limit.
5. **No duplicated logic.** The frontend never re-implements rule evaluation, template rendering or validation; it calls Rust (e.g. `template_preview`).

## 2. Process model

```
┌──────────────────────────────── FolderFlow.app (single process) ───────────────────────────────┐
│                                                                                                 │
│  WebView (React)  ──invoke(typed cmd)──▶  commands/  ──▶  services (engine, rules, db…)         │
│        ▲                                                         │                              │
│        └──────────── typed events (emit) ◀───────────────────────┘                              │
│                                                                                                 │
│  Tray (menu bar) ──────────────────────▶  same services (works with no window open)            │
│                                                                                                 │
│  Background runtime (tokio):                                                                    │
│   Watcher ─▶ Debounce ─▶ Stability ─▶ Classify ─▶ Evaluate ─▶ Plan ─▶ Execute (safe_fs) ─▶ Ledger│
│      ▲                                                                                          │
│   Reconciler (interval / wake / start)        Scheduler ──▶ (same Plan/Execute path)            │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Pipeline channels (capacities are settings-driven defaults):

| Stage | Channel | Capacity | Workers |
|---|---|---|---|
| Watcher → Debounce | mpsc | 4096 | 1 |
| Debounce → Stability | mpsc | 1024 | 1 (timer wheel) |
| Stability → Evaluate | mpsc | 512 | 2 |
| Evaluate → Execute | mpsc | 256 | `max_concurrent_ops` (default 2) |

When a channel is full, upstream drops to "mark folder dirty → reconcile later" rather than buffering unboundedly.

## 3. Repository layout

```text
folderflow/
├── CLAUDE.md                     working agreement for AI/dev sessions
├── docs/                         source of truth (PRD, tracker, audits…)
├── src/                          React frontend
│   ├── main.tsx, App.tsx
│   ├── styles/theme.css          ★ THE ONLY place design values live
│   ├── components/ui/            shadcn primitives (token-only)
│   ├── components/               app components
│   ├── layouts/AppLayout.tsx
│   ├── pages/<screen>/
│   ├── stores/                   Zustand (UI state + cached server state)
│   ├── services/                 thin wrappers over generated bindings
│   ├── bindings.ts               ★ generated from Rust (do not edit)
│   ├── hooks/, types/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs               calls lib::run()
│   │   ├── lib.rs                builder, state, plugin + command registration only
│   │   ├── app_state.rs          AppState + AppPaths, built in Tauri setup()
│   │   ├── error.rs              AppError, UiError (stable ErrorCode)
│   │   ├── logging.rs            tracing → daily-rotated files, redact_path()
│   │   ├── settings/             typed Settings model, validation, SettingsService
│   │   ├── commands/             one file per screen domain; ≤ ~20 lines per handler
│   │   ├── engine/
│   │   │   ├── safe_fs/          ★ only mutating FS code (mod.rs, rename.rs, copy.rs, trash.rs)
│   │   │   ├── fs_view.rs        ReadOnlyFs trait + impl
│   │   │   ├── paths.rs          normalization, approved roots, PathAuthority
│   │   │   ├── watcher.rs, debounce.rs, stability.rs, supervisor.rs
│   │   │   ├── scanner.rs, reconcile.rs, classifier.rs
│   │   │   ├── planner.rs        ExecutionPlan (pure, read-only)
│   │   │   ├── organizer.rs      plan executor
│   │   │   ├── operations.rs     state machine + ledger
│   │   │   ├── conflicts.rs, recovery.rs, undo.rs, scheduler.rs, pipeline.rs
│   │   ├── rules/ models.rs conditions.rs actions.rs evaluator.rs templates.rs validate.rs presets.rs
│   │   ├── duplicates/ hasher.rs cache.rs finder.rs
│   │   ├── database/ pool.rs repo_*.rs
│   │   ├── notifications/ service.rs throttle.rs
│   │   ├── security/ capabilities.rs license.rs keychain.rs
│   │   ├── system/ tray.rs login_item.rs power.rs volumes.rs
│   │   ├── config_io/ export.rs import.rs schema.rs
│   │   └── utils/ time.rs (clock.rs added with the scheduler)
│   ├── migrations/0001_init.sql …
│   ├── capabilities/default.json
│   ├── clippy.toml               disallowed_methods
│   └── tests/                    integration tests (temp dirs only)
├── tests/e2e/                    Playwright
├── scripts/check-tokens.ts         token enforcement (+ check-tokens.test.ts)
└── README.md
```

## 4. Rust module boundaries

| Module | Owns | May depend on | Must NOT |
|---|---|---|---|
| `engine::safe_fs` | rename_excl, copy_excl, trash, mkdir_p, fsync | std, libc | be called by anything except `organizer`, `undo`, `recovery`, `duplicates::actions` |
| `engine::fs_view` | metadata, read_dir, open-for-read | std | mutate |
| `engine::paths` | PathAuthority, normalization, root checks | — | touch disk except `canonicalize` |
| `engine::watcher/debounce/stability/supervisor` | event intake | `notify`, fs_view | evaluate rules |
| `engine::scanner/reconcile` | discovery | fs_view, classifier, database | mutate files |
| `engine::classifier` | category detection | `infer`, categories repo | — |
| `rules::*` | models, evaluation, templates, validation | utils::clock | IO of any kind |
| `engine::planner` | dry-run → `ExecutionPlan` | rules, fs_view, conflicts | access `safe_fs` (enforced: takes `&dyn ReadOnlyFs`) |
| `engine::organizer` | executes a plan | safe_fs, operations, paths | evaluate rules (plan is input) |
| `engine::operations` | state machine + ledger | database | — |
| `engine::recovery/undo` | restore consistency | safe_fs, operations | — |
| `engine::scheduler` | timing + locks | utils::clock | — |
| `duplicates` | hashing, grouping | fs_view, `blake3` | — |
| `database` | SQLx pool + repositories | sqlx | business rules |
| `commands` | validate input → call service → map error | services | contain logic |

Enforced by `clippy.toml` `disallowed_methods` (std::fs::rename, remove_file, remove_dir, remove_dir_all, write, copy, File::create, std::process::Command) with `#[allow]` only inside `safe_fs`.

## 5. Core types (sketch)

```rust
pub struct FileFacts { path, name, stem, ext, size, created, modified, added, is_hidden,
                       mime: Option<String>, category: CategoryId, dev: u64, ino: u64,
                       source_folder: FolderId, hash: Option<Blake3Hash> }

pub enum ConditionNode { All(Vec<ConditionNode>), Any(Vec<ConditionNode>),
                         Not(Box<ConditionNode>), Leaf(Condition) }

pub struct PlanItem { id, file: FileIdentity /* dev, ino, size, mtime_ns */, source: PathBuf,
                      destination: PathBuf, op: OpKind, rule_id, matched: Vec<ConditionRef>,
                      conflicts: Vec<Conflict>, bytes: u64, cross_volume: bool }
pub struct ExecutionPlan { id: Uuid, created_at, items: Arc<[PlanItem]> } // immutable

pub enum OpState { Pending, Validating, InProgress, Verifying, Completed, Failed,
                   RollbackRequired, RolledBack }
```

## 6. Tauri command interface

All commands are `async`, return `Result<T, UiError>`, and are exported to TS via tauri-specta (ADR-004) in `ErrorHandlingMode::Result`, so each binding resolves to `{status:"ok",data}` or `{status:"error",error: UiError}`. Frontend code calls them only through `src/services/*` (`unwrap()` → typed `CommandError`); direct `invoke` is lint-banned. Regenerate with `pnpm bindings`; R-CMD-01 fails CI if stale. Paths in arguments are re-validated by `PathAuthority` on every call.

| Domain | Command | Args → Returns |
|---|---|---|
| app | `app_info` | → `AppInfo` (version, data/log dirs, schema version) |
| settings | `settings_get` | → `Settings` |
| | `settings_update` | `Settings` (full, validated) → `Settings` |
| folders | `folders_list` | → `Vec<MonitoredFolder>` |
| | `folders_pick` | → `Option<PickedFolder>` (native dialog in Rust) |
| | `folders_add` / `folders_update` / `folders_remove` | … → `MonitoredFolder` / `()` |
| | `folders_set_state` | `{id, state: Start\|Pause\|Resume\|Stop\|Restart}` |
| | `folders_scan` | `id` → `ScanSummary` |
| | `folders_permission_status` | `id` → `PermissionStatus` |
| monitoring | `monitoring_status` / `monitoring_set_global` | → `MonitoringStatus` |
| categories | `categories_list/create/update/delete/reorder` | |
| rules | `rules_list/get/create/update/delete/reorder/set_enabled/duplicate` | |
| | `rules_validate` | `RuleDraft` → `Vec<ValidationIssue>` |
| | `rules_presets` | → `Vec<RuleTemplate>` |
| | `template_preview` | `{template, sample: SampleFile}` → `Result<String, TemplateError>` |
| preview | `plan_create` | `PlanScope {folder?, rule?, files?}` → `PlanSummary` (+ `plan_id`) |
| | `plan_items` | `{plan_id, filter, page}` → `Page<PlanItemView>` |
| | `plan_execute` | `{plan_id, selected: Vec<ItemId>}` → `BatchId` (progress via events) |
| | `plan_discard` | `plan_id` |
| activity | `activity_list` | `{filter, page}` → `Page<OperationView>` |
| | `activity_undo` | `UndoTarget::{Last, Ops(Vec), Batch}` → `UndoReport` |
| | `activity_retry` / `activity_export_csv` / `activity_clear` | |
| | `reveal_in_finder` | `op_id, which: Source\|Destination` (path resolved in Rust) |
| review | `review_list` / `review_resolve` | conflicts in "ask" mode |
| duplicates | `dup_scan` / `dup_groups` / `dup_action` / `dup_export` | |
| schedules | `schedules_list/upsert/delete/set_enabled/run_now` | |
| dashboard | `dashboard_stats` | → `DashboardStats` |
| config | `config_export` / `config_import_preview` / `config_import_apply` | |
| license | `license_status/activate/activate_offline/deactivate` | |
| onboarding | `onboarding_state` / `onboarding_set_step` / `onboarding_complete` | |

The frontend never receives a raw "move this path to that path" command. It only references IDs (folder, rule, plan, plan item, operation) that Rust resolves.

### Events (Rust → UI)

| Event | Payload |
|---|---|
| `monitoring://status` | `MonitoringStatus` |
| `folder://health` | `{folder_id, health, last_error?}` |
| `scan://progress` | `{folder_id, files, bytes, done}` |
| `plan://progress` | `{plan_id, evaluated, total}` |
| `batch://progress` | `{batch_id, done, failed, total}` |
| `activity://new` | `OperationView` |
| `review://new` | `ReviewItem` |
| `dup://progress` | `{hashed, total, bytes}` |

## 7. Frontend

### Routes

| Path | Page | Notes |
|---|---|---|
| `/onboarding/:step` | Onboarding | shown when `onboarding_state.complete == false` |
| `/` | Overview | dashboard |
| `/folders`, `/folders/:id` | Folders | |
| `/rules` | Rules | |
| `/rules/new`, `/rules/:id` | RuleBuilder | |
| `/preview` | Preview | `?folder=`, `?rule=` |
| `/activity` | Activity | |
| `/duplicates` | Duplicates | |
| `/scheduler` | Scheduler | |
| `/categories` | Categories | |
| `/settings/:section` | Settings | general, monitoring, organization, performance, appearance, privacy, backup |
| `/license` | License | |

### Shared components (all styled by theme tokens only)

`AppLayout`, `Sidebar`, `SidebarItem`, `StatusDot`, `PageHeader` (title, breadcrumbs, search, actions), `DataTable` (virtualized, sortable, paginated), `EmptyState`, `ErrorState`, `LoadingState`, `ConfirmDialog` (risky actions, typed confirmation for destructive), `PathLabel` (truncating middle, reveal button), `CategoryBadge`, `OpStatusBadge`, `StatTile`, `ConditionRow`, `ConditionGroup`, `ActionRow`, `TemplateInput` (with live preview from Rust), `FolderPickerButton`, `Toast`, `KeyboardShortcut`.

### State

- Zustand stores: `useUi` (theme, density, sidebar), `useMonitoring`, `useFolders`, `useRules`, `usePlan`, `useActivity`. Stores call `services/*` which call generated bindings. Event listeners update stores.
- No business logic in stores — only caching and UI state.
- Stores that save optimistically must never roll back to a locally remembered state on failure; they re-read from Rust (BUG-004).

### Navigation availability

`src/app/navigation.ts` lists every screen with `available`. Unbuilt screens render as disabled sidebar entries labelled "Soon" (no placeholder pages, NFR-044) and are not routed. Flip `available` when a screen ships; Phase 9 requires all `true`.
