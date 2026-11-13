# FolderFlow — Product Requirements Document

| Field | Value |
|---|---|
| Product | FolderFlow — automatic file organizer for macOS |
| Tagline | Your files organized automatically. |
| Doc status | v1.0 — Approved baseline for build |
| Last updated | 2026-11-13 |
| Source of truth | This file (requirements). Progress lives in [PLAN_TRACKER.md](PLAN_TRACKER.md). |

> **Priority order for every trade-off:** correctness and user-data safety → performance → visual polish.

Every requirement has a stable ID (`FR-xxx`, `NFR-xxx`). The plan tracker, tests, and audits reference these IDs. Do not renumber; deprecate instead.

---

## 1. Problem

Mac users accumulate hundreds of files in Downloads, Desktop and Documents. Manual sorting is tedious and gets skipped. Existing tools are either too technical (scripts, Automator), too expensive or dated, or cloud-based (privacy risk). Users want automation they can **trust**: it should never lose, overwrite or silently delete a file, and they should see what will happen before it happens.

## 2. Target users

| Persona | Need | Key features |
|---|---|---|
| **Busy professional** | Downloads folder never becomes a dump; invoices end up in one place. | Starter rules, invoice organizer, background monitoring. |
| **Creative / designer** | Screenshots, exports, assets sorted by type and date. | Screenshot organizer, date templates, design category. |
| **Developer / power user** | Precise, nested rules; per-folder schedules; import/export. | Rule builder with nested groups, regex, scheduler, JSON config. |
| **Cautious user** | Wants to see every change and undo anything. | Preview/dry-run, activity log, undo. |

## 3. Goals and non-goals

### Goals
1. Organize files automatically and continuously, entirely offline.
2. Never lose, overwrite or silently delete a user file.
3. Show a complete preview before any automatic organization is enabled.
4. Let non-technical users build powerful rules visually.
5. Make every operation auditable and, where possible, reversible.
6. Feel like a premium, native macOS app.

### Non-goals (v1)
- Cloud sync, accounts, or any upload of file contents.
- Content understanding (OCR, AI classification). Invoice detection is filename-based only.
- Arbitrary shell/script execution from rules.
- Automatic permanent deletion of any file.
- Windows / Linux support.
- Mac App Store distribution (see [DECISIONS.md](DECISIONS.md) ADR-002).

## 4. Platform

| Item | Requirement |
|---|---|
| OS | macOS 13 Ventura and later |
| Architecture | Apple Silicon (arm64) primary; Intel (x86_64) via universal binary where feasible |
| Distribution | Direct download, Developer ID signed, notarized, DMG |
| Permissions | Native folder selection + TCC prompts; no Full Disk Access by default |
| Background | Menu bar app; keeps running when the main window closes; launch at login |

---

## 5. Functional requirements

### 5.1 Onboarding (FR-001 – FR-009)
| ID | Requirement |
|---|---|
| FR-001 | Welcome step explains what the app does, how automation works, that files stay local, no internet needed, and that the user is in control. |
| FR-002 | Folder permission step lets the user choose Downloads, Desktop, Documents, Pictures, Movies, Music and custom folders via the native folder picker. |
| FR-003 | Permission denial is detected and explained, with steps to grant access in System Settings and a "Try again" action. |
| FR-004 | Organization preference step: by type, by date, by category, custom rules, keep original structure. |
| FR-005 | Starter rules step: PDFs, screenshots, images, videos, archives, invoices, downloads older than 30 days. All starter rules are created **disabled** or presented for review. |
| FR-006 | Initial scan shows total files, folders, categories, estimated disk usage, and files matching suggested rules. |
| FR-007 | No file is moved, renamed, copied or created during onboarding. |
| FR-008 | Final step shows the full preview and requires explicit confirmation before enabling automatic organization. |
| FR-009 | Onboarding can be resumed if quit midway and re-run from Settings. |

### 5.2 Folder monitoring (FR-010 – FR-029)
| ID | Requirement |
|---|---|
| FR-010 | Monitor multiple folders at once, including external volumes. |
| FR-011 | Per folder: name, path, enabled, recursive, included extensions, excluded extensions, excluded subfolders, last scanned, health, file count. |
| FR-012 | Detect create, modify, rename, move, delete of files and creation of directories. |
| FR-013 | Debounce and deduplicate watcher events. |
| FR-014 | Periodic reconciliation scans recover missed events (watcher events are never assumed reliable). |
| FR-015 | Stability check: a file is processed only after size and mtime are unchanged across N consecutive checks spaced by a configurable interval. |
| FR-016 | Ignore: hidden files (configurable), `.DS_Store`, `._*`, `.localized`, temporary/partial downloads (`.download`, `.crdownload`, `.part`, `.partial`, `.tmp`, `.opdownload`), app cache dirs, excluded paths, and FolderFlow's own staging files. |
| FR-017 | Do not follow symlinks by default; detect and prevent symlink loops. |
| FR-018 | Controls: start, pause, resume, stop, restart — per folder and globally. |
| FR-019 | Watcher errors and monitoring statistics are visible per folder. |
| FR-020 | Watcher restarts automatically after errors with exponential backoff. |
| FR-021 | Monitoring continues when the main window is closed. |
| FR-022 | Handle sleep/wake, restart, logout/login: reconcile on resume. |
| FR-023 | Handle external drive disconnect/reconnect: mark folder "Unavailable", resume and reconcile on reconnect. |
| FR-024 | Handle folder permission revocation, rename and deletion: mark health, surface to user, never crash. |
| FR-025 | Handle watcher resource limits gracefully (fall back to polling reconciliation and report). |

### 5.3 Classification (FR-030 – FR-039)
| ID | Requirement |
|---|---|
| FR-030 | Built-in categories: Documents, Images, Videos, Audio, Archives, Spreadsheets, Presentations, Applications & Installers, Design, Development, Fonts, E-books, Other (extension lists per spec §5.1). |
| FR-031 | Classification combines extension, magic-byte signature (`infer`), and metadata. Files are never executed. |
| FR-032 | When extension and signature disagree, or confidence is low, classify as Other/Unknown and do not auto-move unless a rule explicitly targets it. |
| FR-033 | `.app` bundles are treated as a single item, never traversed as a folder of files. |
| FR-034 | Custom categories: name, description, icon, color token, extensions, default destination, priority, enabled. |
| FR-035 | Custom categories take precedence over built-ins by priority; conflicts are deterministic. |

### 5.4 Rules (FR-040 – FR-069)
| ID | Requirement |
|---|---|
| FR-040 | Rule fields: name, description, enabled, priority, source folders, condition tree, actions, execution settings, created/updated, last execution, stats. |
| FR-041 | Condition groups: ALL (AND), ANY (OR), NOT, nested to a bounded depth (max 5). |
| FR-042 | File attribute conditions: extension, MIME type, category, size, name, full path, parent folder, source folder, hidden. |
| FR-043 | Text operators: contains, not contains, starts with, ends with, equals, not equals, matches regex (case-insensitive by default, toggle). |
| FR-044 | Size operators: >, <, =, between; units B/KB/MB/GB (1 KB = 1000 B, shown in UI). |
| FR-045 | Date conditions: created/modified before/after, older/newer than N days, within range, today, yesterday, this week, this month, this year — evaluated in the user's local time zone. |
| FR-046 | Location conditions: source folder equals/contains, inside directory, inside subdirectory. |
| FR-047 | Advanced conditions: content hash, duplicate status, empty file, filename pattern (glob), extension group. |
| FR-048 | Regex patterns are validated at save time with size limits (no catastrophic patterns; Rust `regex` is linear-time). |
| FR-050 | Actions: move, copy, rename, add prefix, add suffix, remove text, replace text, change extension (only when safe and explicit), create destination folder, move into dynamic folder, add date to filename, skip, stop processing, notify, mark for review, apply another rule. |
| FR-051 | No shell / script / command execution action exists. |
| FR-052 | Priority ordering; execution modes: first match, continue matching, stop after successful move, stop after a specific action, explicit sequential. |
| FR-053 | Loop detection: a file may be moved at most once per evaluation pass; max N actions per file per pass; "apply another rule" chains have cycle detection and a depth limit. |
| FR-054 | Destination inside a monitored source folder that would re-trigger the same rule is flagged at save time and guarded at runtime. |
| FR-055 | Rules are importable/exportable (versioned JSON). Imported rules are disabled until the user confirms. |
| FR-056 | Visual rule builder: add, edit, delete, reorder, group, ungroup conditions; validation with inline errors. |
| FR-057 | Live "test this rule" against a chosen folder (dry-run). |

### 5.5 Templates (FR-070 – FR-079)
| ID | Requirement |
|---|---|
| FR-070 | Variables: `{year}` `{month}` `{month_name}` `{day}` `{date}` `{extension}` `{category}` `{original_name}` `{source_folder}`. |
| FR-071 | Date variables use a configurable date source (modified / created / added / now), default *modified*. |
| FR-072 | Generated paths are validated: no traversal (`..`), no absolute injection from variables, no empty components, no invalid macOS names (`:`, NUL, leading/trailing spaces stripped), component ≤ 255 bytes, path ≤ 1024 bytes. |
| FR-073 | Destinations must resolve inside an approved root (user-chosen destination roots or monitored folders). |
| FR-074 | Circular destination configurations are rejected. |
| FR-075 | Live template preview with a sample file in the rule builder. |
| FR-076 | Naming templates: `{date}_{original_name}`, `{category}_{original_name}`, `{year}-{month}_{original_name}`, custom. |

### 5.6 Preview / dry-run (FR-080 – FR-089)
| ID | Requirement |
|---|---|
| FR-080 | Per file: name, current location, proposed destination, size, category, matched rule, matched conditions, planned actions, conflicts, status. |
| FR-081 | Preview a file, a folder, or all active rules. Filter by category/rule; search filenames. |
| FR-082 | Detect duplicate destinations, permission problems, unavailable destinations, conflicting rules. |
| FR-083 | Totals: files affected, bytes transferred (cross-volume), conflicts. |
| FR-084 | Dry-run performs **zero** filesystem writes (enforced by architecture and tests). |
| FR-085 | Dry-run produces an immutable, versioned execution plan with file identity snapshots. |
| FR-086 | Before executing each plan item, revalidate identity (dev+inode), size, mtime and destination; stale items are skipped and reported. |
| FR-087 | User can deselect individual plan items before executing. |

### 5.7 Safe operations (FR-090 – FR-109)
| ID | Requirement |
|---|---|
| FR-090 | Move pipeline: validate source, permissions, destination, conflicts, availability → record intent → execute → verify → record completion. |
| FR-091 | Same-volume moves use atomic no-overwrite rename (`renamex_np` with `RENAME_EXCL`). |
| FR-092 | Cross-volume moves: staged copy to a temp name on the destination volume → fsync → verify (size + BLAKE3) → exclusive rename into place → remove original. Never described as atomic. |
| FR-093 | Conflict policies: auto-rename `name (1).ext`, skip, ask, keep both. Default: auto-rename. Overwrite only with explicit per-operation consent. |
| FR-094 | "Ask" policy in background mode queues the item as *Needs review* instead of blocking. |
| FR-095 | Operation ledger stores every field in spec §9.3 including reversibility. |
| FR-096 | Undo: last, selected, batch, restore previous folder structure. Supported for move, rename, and copy (copy undo removes the copy only if its hash still matches). |
| FR-097 | Undo verifies the original path is free; if occupied, asks the user; never overwrites the newer file. |
| FR-098 | Persistent states: PENDING, VALIDATING, IN_PROGRESS, VERIFYING, COMPLETED, FAILED, ROLLBACK_REQUIRED, ROLLED_BACK. |
| FR-099 | Startup recovery inspects all non-terminal operations and reconciles against the actual filesystem. |
| FR-100 | Recovery covers crash, shutdown, interrupted copy, permission change, destination disconnect, DB failure, partial operations. |
| FR-101 | Retry failed operations where safe (revalidated first). |

### 5.8 Duplicates (FR-110 – FR-119)
| ID | Requirement |
|---|---|
| FR-110 | Detection by name, by size, by content hash (BLAKE3), same content / different names. Size is the prefilter. |
| FR-111 | Streaming hashing with a fixed buffer; never load whole files. |
| FR-112 | Hash cache keyed by (dev, inode, size, mtime_ns); invalidated on any change. |
| FR-113 | Dashboard: groups, paths, sizes, total wasted space, hash, copy count. |
| FR-114 | Actions: review, reveal in Finder, compare details, keep selected, move duplicates to review folder, ignore, export report (CSV/JSON). |
| FR-115 | No automatic deletion. Moving to Trash (never permanent delete) only after explicit confirmation. |

### 5.9 Smart organizers (FR-120 – FR-129)
| ID | Requirement |
|---|---|
| FR-120 | Screenshot organizer with default macOS patterns ("Screenshot …", "Screen Shot …", "CleanShot …") and custom patterns. |
| FR-121 | Invoice organizer by configurable filename keywords (Invoice, Receipt, Bill, Payment, Tax, GST). Filename-based only. |
| FR-122 | Download organizer presets per category. |
| FR-123 | Old file organizer: 7/15/30/60/90/180/365 days → archive folder. Never deletes. |
| FR-124 | All smart organizers are implemented as ordinary rule templates (no special engine paths). |

### 5.10 Scheduler (FR-130 – FR-139)
| ID | Requirement |
|---|---|
| FR-130 | Modes: real-time, 5/15/30 min, hourly, daily, weekly, custom interval, on launch, manual. |
| FR-131 | Per-folder and per-rule schedules; enable/pause; preferred time; days of week; quiet hours. |
| FR-132 | Runs without the main window. |
| FR-133 | Missed runs during sleep follow an explicit catch-up policy: skip, run once, (never "run all"). |
| FR-134 | No overlapping runs of the same rule set (per-scope lock). |

### 5.11 Dashboard (FR-140 – FR-149)
| ID | Requirement |
|---|---|
| FR-140 | Stats: total organized, today, this week, active folders, active rules, pending ops, failed ops, storage managed, duplicates found, duplicate space. All database-backed. |
| FR-141 | Sections: monitoring overview (with quick pause/resume), recent activity, rule performance, storage by category, quick actions (add folder, create rule, organize now, preview, find duplicates). |

### 5.12 Menu bar (FR-150 – FR-155)
| ID | Requirement |
|---|---|
| FR-150 | Items: status, active/paused, pause all, resume, organize now, preview pending, recent activity, open FolderFlow, settings, quit. |
| FR-151 | Status icon reflects state: active, paused, working, error. |
| FR-152 | Works when the main window is closed or never opened. |

### 5.13 Activity log (FR-160 – FR-169)
| ID | Requirement |
|---|---|
| FR-160 | Columns: file, operation, original, destination, rule, timestamp, status, actions. |
| FR-161 | Filters: date range, operation type, category, rule, success, failed, reversed, filename search. Server-side paginated. |
| FR-162 | Actions: undo, reveal source, reveal destination, details, retry (safe), export CSV, clear history (confirm). |
| FR-163 | Operation history is separate from diagnostic logs. |

### 5.14 Categories screen (FR-170 – FR-172)
| ID | Requirement |
|---|---|
| FR-170 | List built-in and custom categories; edit custom; toggle built-ins; reorder priority. |

### 5.15 Settings (FR-180 – FR-189)
| ID | Requirement |
|---|---|
| FR-180 | General: launch at login, auto-start monitoring, start minimized, notifications, menu bar icon, confirm bulk ops. |
| FR-181 | Monitoring: stability interval, recursive default, reconciliation interval, ignore hidden, ignore temp, max concurrent ops. |
| FR-182 | Organization: default destination, duplicate handling, naming template, execution mode, auto-create folders. |
| FR-183 | Performance: CPU preference, background processing, hashing settings, scan concurrency, DB maintenance (VACUUM/ANALYZE). |
| FR-184 | Appearance: light, dark, system; compact / comfortable density. |
| FR-185 | Privacy: local-only statement, diagnostics preference (off by default), data export, clear app data (confirm). |

### 5.16 Import / export (FR-190 – FR-194)
| ID | Requirement |
|---|---|
| FR-190 | Import/export rules, folders, categories, exclusions, settings; full configuration backup; activity CSV. |
| FR-191 | Versioned JSON with `schema_version`; validated before applying; diff shown before import. |
| FR-192 | Imported rules are never auto-enabled. |

### 5.17 Notifications (FR-200 – FR-204)
| ID | Requirement |
|---|---|
| FR-200 | Native notifications for: success summary, failures, permission issues, destination unavailable, duplicates found, large batch done, monitoring stopped unexpectedly. |
| FR-201 | Per-event-type preferences. |
| FR-202 | Batching and throttling — never one notification per file. |

### 5.18 License (FR-210 – FR-216)
| ID | Requirement |
|---|---|
| FR-210 | Lifetime license: activate, status, validate, deactivate, recover. |
| FR-211 | Offline activation via signed license file / signed key (Ed25519 public key embedded). |
| FR-212 | License key stored in macOS Keychain. |
| FR-213 | Monitoring and organization never depend on a license server being reachable. |
| FR-214 | Clear activation error messages. |
| FR-215 | Device activation limits if the provider supports it. |

### 5.19 Keyboard shortcuts (FR-220)
| ID | Requirement |
|---|---|
| FR-220 | ⌘N new rule · ⇧⌘O add folder · ⌘O open folder · ⌘F search · ⌘Z undo last operation (only outside text inputs) · ⌘R refresh · ⌘, settings · ⌘↩ execute selected preview batch (after confirmation). |

---

## 6. Non-functional requirements

### Safety (highest priority)
| ID | Requirement |
|---|---|
| NFR-001 | No code path may overwrite an existing file without explicit per-operation consent. Enforced by using exclusive-create / exclusive-rename primitives only. |
| NFR-002 | No code path permanently deletes a user file except the original after a verified cross-volume copy. Duplicates go to Trash only with confirmation. |
| NFR-003 | Every filesystem mutation is preceded by a persisted operation intent (write-ahead ledger). |
| NFR-004 | Dry-run code paths have no access to the mutating filesystem interface (compile-time separation). |
| NFR-005 | All paths from the frontend are validated, normalized and authorized by Rust against approved roots. |

### Security & privacy
| ID | Requirement |
|---|---|
| NFR-010 | No network calls in core processing. Only the license module may use the network, and only on user action. |
| NFR-011 | Tauri capabilities restricted to the minimum; no `fs`/`shell` plugin exposure to the webview. |
| NFR-012 | Strict CSP; no remote scripts. |
| NFR-013 | Logs never contain file contents; paths are logged at debug level only and can be redacted. |

### Performance
| ID | Requirement |
|---|---|
| NFR-020 | UI thread never performs filesystem work. |
| NFR-021 | Bounded queues and concurrency limits on watcher → stability → evaluation → execution pipeline. |
| NFR-022 | Benchmarks recorded for 1k / 10k / 50k / 100k files: scan time, event latency, rule throughput, query time, hash MB/s, RSS, CPU. Targets set from first measurement, not promised in advance. |
| NFR-023 | Idle CPU near zero when no events; memory bounded independent of folder size (streaming). |

### Reliability
| ID | Requirement |
|---|---|
| NFR-030 | App recovers all non-terminal operations on start. |
| NFR-031 | DB uses WAL mode, foreign keys ON, `synchronous=NORMAL` (FULL for ledger writes). |
| NFR-032 | Corrupted DB → backup, recreate, surface to user; never block file safety. |

### UX
| ID | Requirement |
|---|---|
| NFR-040 | All UI colors, surfaces, borders, text, radii, spacing, typography, motion come from the single theme file [`src/styles/theme.css`](../src/styles/theme.css). Enforced by `scripts/check-tokens.ts` (`pnpm check:tokens`) in CI. |
| NFR-041 | Light, dark, system appearance; compact / comfortable density. |
| NFR-042 | Every screen has empty, loading, error and success states. |
| NFR-043 | Keyboard accessible; visible focus rings; WCAG AA contrast for text tokens. |
| NFR-044 | No placeholder buttons, no fake stats, no mock data in production screens. |

### Quality
| ID | Requirement |
|---|---|
| NFR-050 | CI gates: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `cargo audit`, `npm run lint`, `typecheck`, `test`, `build`, token check. |
| NFR-051 | Every bug fix ships with a regression test referenced in [BUGS.md](BUGS.md). |
| NFR-052 | Every phase closes with an audit recorded in [audits/](audits/). |

---

## 7. Screens

| Screen | Route | Key FRs |
|---|---|---|
| Onboarding | `/onboarding/:step` | FR-001–009 |
| Overview (Dashboard) | `/` | FR-140–141 |
| Monitored Folders | `/folders`, `/folders/:id` | FR-010–025 |
| Rules | `/rules` | FR-040–057 |
| Rule Builder | `/rules/new`, `/rules/:id` | FR-040–076 |
| Preview | `/preview` | FR-080–087 |
| Activity | `/activity` | FR-160–163, FR-096 |
| Duplicates | `/duplicates` | FR-110–115 |
| Scheduler | `/scheduler` | FR-130–134 |
| Categories | `/categories` | FR-034–035, FR-170 |
| Settings | `/settings/:section` | FR-180–185, FR-190–192 |
| License | `/license` | FR-210–215 |

## 8. Release / acceptance criteria

Production-ready only when every item in spec §30 is verified and checked in [PLAN_TRACKER.md](PLAN_TRACKER.md) § "Final acceptance":
multi-folder monitoring in background, survives restart, correct classification, custom rules with nested AND/OR, dynamic templates, accurate preview, dry-run never modifies files, safe move/copy/rename, no silent overwrites, undo, crash recovery, hash-based duplicates, scheduled automation, menu bar, fully offline, migrations, failures logged and surfaced, tests passing, benchmarks recorded, signed and notarized.

## 9. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| FSEvents coalesces/drops events | Missed files | Reconciliation scans (FR-014); tests never rely on single events. |
| `std::fs::rename` silently overwrites on POSIX | Data loss | Only `renamex_np(RENAME_EXCL)` via a single audited `safe_fs` module; clippy `disallowed_methods` bans `std::fs::rename`/`remove_file` elsewhere. |
| TCC prompts confuse users | Abandoned onboarding | Explain before prompting; detect denial; deep-link to System Settings. |
| Rule loops move files repeatedly | Churn / user distrust | FR-053/054 + per-file recent-move guard. |
| Stale preview plans | Wrong moves | Identity revalidation (FR-086). |
| Cross-volume interruption | Duplicate or partial file | Staged temp names + recovery (FR-092/099). |
| Licensing provider undecided | Phase 7 blocked | Offline-signed key design is provider-agnostic; see open questions. |

## 10. Open questions

| # | Question | Owner | Status |
|---|---|---|---|
| Q1 | Which license provider (e.g. Dodo Payments license keys, Lemon Squeezy, Paddle, self-hosted)? Affects activation API only; offline signed-key validation is the same. | Product | Open — needed before Phase 7 |
| Q2 | Pricing and device activation limit? | Product | Open |
| Q3 | Apple Developer ID account available for signing/notarization? | Product | Open — needed before Phase 9 |
| Q4 | Universal binary or separate Intel build? | Eng | Default: universal |
| Q5 | App icon / brand assets source? | Design | Open — needed before Phase 9 |
