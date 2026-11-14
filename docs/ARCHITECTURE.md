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
│   │   │   ├── safe_fs/          ★ only mutating FS code (mod.rs, rename.rs, copy.rs, trash.rs, fault.rs) [P4]
│   │   │   ├── fs_view.rs        ReadOnlyFs trait + RealFs (stat, bounded header read; no follow, non-blocking)
│   │   │   ├── paths.rs          PathAuthority, validate_new_root, NFC normalisation           [P2]
│   │   │   ├── ignore.rs         always-ignored names, partial downloads, cache dirs, settings  [P2]
│   │   │   ├── classifier.rs     extension + infer signature → category/confidence             [P2]
│   │   │   ├── scanner.rs        walkdir, no symlink follow, packages as items, 500-item batches [P2]
│   │   │   ├── reconcile.rs      scan → upsert → delete unseen → report new/changed            [P2]
│   │   │   ├── debounce.rs, stability.rs   pure state machines (caller supplies time)       [P2]
│   │   │   ├── monitor.rs        MonitorService + one supervised task per folder (watcher, backoff, health) [P2]
│   │   │   ├── planner.rs        ExecutionPlan (pure, read-only, PreviewStore)              [P5]
│   │   │   ├── organizer.rs      transactional move/copy/rename pipeline                       [P4]
│   │   │   ├── operations.rs     state machine + write-ahead ledger (`LedgerToken`)            [P4]
│   │   │   ├── conflicts.rs, recovery.rs, undo.rs, activity.rs                                 [P4]
│   │   │   ├── scheduler.rs, pipeline.rs                                                       [P5]
│   │   ├── categories/        built-in table + custom category service                        [P2]
│   │   ├── folders/           monitored-folder registry, probe_access, overlap rule          [P2]
│   │   ├── onboarding/        6-step onboarding state + non-mutating scan preview             [P5]
│   │   ├── system/            macOS integration via objc2 (System Settings link)             [P2]
│   │   ├── events.rs          typed events + TauriObserver + CompositeObserver                 [P2/P5]
│   │   ├── rules/ models.rs conditions.rs evaluator.rs templates.rs validate.rs presets.rs smart.rs service.rs [P3/P6]
│   │   ├── duplicates/ hasher.rs cache.rs finder.rs actions.rs                            [P6]
│   │   ├── database/ pool.rs repo_*.rs                                                   [P1–P6]
│   │   ├── notifications/ mod.rs service.rs                                              [P6]
│   │   ├── security/ capabilities.rs license.rs keychain.rs
│   │   ├── system/ tray.rs login_item.rs power.rs volumes.rs
│   │   ├── config_io/ export.rs import.rs schema.rs                                      [P6]
│   │   └── utils/ time.rs clock.rs                                                               [P3]
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
| | `folders_add_with_picker` | `recursive` → `Option<MonitoredFolder>` (native picker runs in Rust; `None` = cancelled) |
| | `folders_add_known` | `kind: Downloads\|Desktop\|Documents\|Pictures\|Movies\|Music, recursive` → `MonitoredFolder` |
| | `folders_update` | `id, FolderPatch` → `MonitoredFolder` (restarts monitoring; scope change resets baseline) |
| | `folders_remove` | `id` → `()` (stops monitoring; files untouched) |
| | `folders_action` | `id, Pause\|Resume\|Rescan\|Restart` → `()` |
| | `folders_pick_subfolder` | `id` → `Option<String>` (picker inside the folder; re-validated on save) |
| | `folders_summary` | `id` → `FolderSummary` (files, bytes, per category) |
| | `folders_errors` | `id` → `Vec<WatcherErrorView>` (latest 50) |
| | `folders_access` | `id` → `FolderAccess` |
| | `open_privacy_settings` | → `bool` (fixed System Settings URL via NSWorkspace) |
| monitoring | `monitoring_status` | → `MonitoringStatus { paused, folders: [FolderRuntime] }` |
| | `monitoring_set_paused` | `paused` → `MonitoringStatus` |
| categories | `categories_list/create/update/delete/set_enabled/reorder/pick_destination_folder` | built-in + custom category CRUD, ordering, destination folder picker [P2/P6] |
| rules | `rules_list/get/create/create_from_preset/update/delete/reorder/set_enabled/duplicate` | CRUD, priority reorder, duplicate, preset install |
| | `rules_smart_catalog` / `rules_create_smart_preset` | Smart Organizer template catalog & disabled preset installation [P6] |
| | `rules_validate` | `(RuleDraft, current_rule_id)` → `Vec<ValidationIssue>` |
| | `rules_presets` | → `Vec<RuleTemplate>` (7 starter templates, all disabled) |
| | `template_preview` | `TemplatePreviewInput` → `TemplatePreviewOutput` |
| | `rules_test_folder` | `(RuleDraft, folder_id)` → `RuleTestReport` (dry-run against scanned files) |
| | `rules_pick_destination_folder` | → `Option<String>` (native folder picker; registers approved destination root) |
| preview | `plan_create` | `PlanScope {folder?, rule?, files?}` → `PlanSummary` (`planId` cached in `PreviewStore`) |
| | `plan_items` | `(plan_id, PlanItemFilter, limit, offset)` → `PlanItemsPage` |
| | `plan_execute` | `(plan_id, selected_item_ids?, ConflictPolicy)` → `PlanExecutionReport` |
| | `plan_discard` | `plan_id` → `bool` |
| activity | `activity_list` | `{filter, page}` → `Page<OperationView>` |
| | `activity_undo` | `UndoTarget::{Last, Ops(Vec), Batch}` → `UndoReport` |
| | `activity_retry` / `activity_export_csv` / `activity_clear` | |
| | `reveal_in_finder` | `op_id, which: Source\|Destination` (path resolved in Rust) |
| review | `review_list` / `review_resolve` | conflicts in "ask" mode |
| duplicates | `dup_scan/groups/group_details/action/export/reveal_file/pick_review_folder` | duplicate discovery, grouping, keeper, undoable review move w/ exclusion, trash, export [P6] |
| schedules | `schedules_list/upsert/delete/set_enabled/run_now` | SQLite schedules, toggle, manual run (`ScheduleRunOutcome`) |
| dashboard | `dashboard_stats` / `db_maintenance` | SQLite stats aggregation & `ANALYZE`/`VACUUM` maintenance [P6] |
| exclusions | `exclusions_list/create/delete` | global & per-folder `path`/`glob`/`extension` exclusion rules [P6] |
| config | `config_export/pick_import_file/import_preview/import_apply` | versioned JSON export, dry-run diff preview, merge/replace import [P6] |
| notifications | `notifications_list/mark_read/clear/test` | batched/throttled notification center history & test dispatch [P6] |
| license | `license_status/activate/activate_offline/deactivate` | |
| onboarding | `onboarding_state` / `onboarding_set_step` / `onboarding_scan_preview` / `onboarding_complete` / `onboarding_reset` | persistent wizard state + non-mutating starter scan |

The frontend never receives a raw "move this path to that path" command. It only references IDs (folder, rule, plan, plan item, operation) that Rust resolves.

### Events (Rust → UI)

| Event | Payload |
|---|---|
| `folder-status-event` ✅ | `FolderRuntime` — emitted on every folder state change |
| `folder-activity-event` ✅ | `{folderId, kind: stable\|removed}` — no paths; UI refetches (debounced) |
| `dup-progress-event` ✅ | `DupProgressEvent` — progress updates during duplicate scanning |
| `app-notification-event` ✅ | `AppNotificationEvent` — batched/throttled notification delivery |
| `scan://progress` | `{folder_id, files, bytes, done}` (planned) |
| `plan://progress` | `{plan_id, evaluated, total}` |
| `batch://progress` | `{batch_id, done, failed, total}` |
| `activity://new` | `OperationView` |
| `review://new` | `ReviewItem` |

## 7. Frontend

### Routes

| Path | Page | Notes |
|---|---|---|
| `/onboarding`, `/onboarding/:step` | Onboarding ✅ | 6-step setup flow when `onboarding_state.completed == false` |
| `/overview` (and `/` redirect) | Overview ✅ | real SQLite dashboard stats, category storage breakdown, trend charts, active folder summary |
| `/folders`, `/folders/:id` | Folders ✅ | list + detail (status, overview by category, settings form, recent problems) |
| `/rules` | Rules ✅ | priority-ordered list, starter preset & Smart Organizer menus, enable toggle, reorder, duplicate, delete |
| `/rules/new`, `/rules/:id` | RuleBuilder ✅ | nested condition tree editor, actions list, live validation, template preview, dry-run folder test |
| `/preview` | Preview ✅ | scope/filter dry-run plan, conflict checks, selective execution w/ ⌘↩ confirmation |
| `/activity` | Activity ✅ | paginated operation ledger, filters, undo last/selected/row/batch, conflict resolution (`Needs Review`), retry, CSV export, clear history, reveal in Finder |
| `/duplicates` | Duplicates ✅ | duplicate scan, wasted-space summary, keeper/review/trash/ignore actions, CSV/JSON export, review folder config |
| `/scheduler` | Scheduler ✅ | interval/daily/weekly/launch/manual schedules, quiet hours, catch-up policy, run now |
| `/categories` | Categories ✅ | built-in & custom categories, extension/color editor, reorder, default destination picker |
| `/settings/:section` | Settings ✅ | general, appearance, monitoring, organization, performance, notifications, privacy (exclusions), backup, about |
| `/license` | License | planned (Phase 8/9) |

### Shared components (all styled by theme tokens only)

`AppLayout`, `Sidebar`, `SidebarItem`, `StatusDot`, `PageHeader` (title, breadcrumbs, search, actions), `DataTable` (virtualized, sortable, paginated), `EmptyState`, `ErrorState`, `LoadingState`, `ConfirmDialog` (risky actions, typed confirmation for destructive), `PathLabel` (truncating middle, reveal button), `CategoryBadge`, `OpStatusBadge`, `StatTile`, `ConditionRow`, `ConditionGroup`, `ActionRow`, `TemplateInput` (with live preview from Rust), `FolderPickerButton`, `Toast`, `KeyboardShortcut`.

### State

- Zustand stores: `useUi` (theme, density, sidebar), `useMonitoring`, `useDashboard`, `useFolders`, `useRules`, `usePreview`, `useActivity`, `useDuplicates`, `useCategories`, `useSchedules`, `useOnboarding`, `useSettings`. Stores call `services/*` which call generated bindings. Event listeners update stores.
- No business logic in stores — only caching and UI state.
- Stores that save optimistically must never roll back to a locally remembered state on failure; they re-read from Rust (BUG-004).

### Navigation availability

`src/app/navigation.ts` lists every screen with `available`. Unbuilt screens render as disabled sidebar entries labelled "Soon" (no placeholder pages, NFR-044) and are not routed. Flip `available` when a screen ships; Phase 9 requires all `true`.

## 8. Folder monitor (Phase 2)

```
FSEvents (notify) ──try_send──▶ bounded channel (4096) ──▶ folder task
         │ full → overflow flag ───────────────────────────────▲ reconcile
folder task:  debounce (dedupe, quiet 500 ms, ≤10k paths) ─▶ re-stat
              ├─ missing  → forget record → on_removed
              ├─ new dir  → reconcile (recursive folders)
              └─ file/package (not ignored) → stability tracker (≤10k)
              stability: N identical (size, mtime, dev, ino) checks → classify → upsert → on_stable
supervisor:   probe_access → Unavailable / PermissionDenied (re-check every 30 s)
              watcher start/runtime error → Error, exponential backoff 1 s … 5 min
              periodic reconcile (settings), health tick (30 s), Rescan command, overflow → reconcile
```

- One task per folder is the **only writer** of that folder's `file_records`, so scans never race.
- `MonitorService` owns tasks; `stop` sends Stop and cancels after `stop_timeout` (BUG-010).
- Global pause and per-folder pause are tracked separately; resume-all does not resume folders the user paused individually.
- Stable files are recorded and reported to `MonitorObserver`; Phase 5 attaches the rule pipeline there.
