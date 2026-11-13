# FolderFlow — Database Design

> SQLite via SQLx. Source of truth for schema. Migration files in `src-tauri/migrations/` must match this doc.

## Conventions

- Location: `~/Library/Application Support/com.folderflow.desktop/folderflow.db`
- `PRAGMA journal_mode=WAL; foreign_keys=ON; synchronous=NORMAL; busy_timeout=5000;`
- Ledger writes that precede a filesystem mutation use a transaction committed with `synchronous=FULL` semantics (explicit `PRAGMA synchronous=FULL` on the ledger connection).
- IDs: UUID v7 as `TEXT` (time-ordered → good index locality).
- Timestamps: `INTEGER` Unix epoch **milliseconds, UTC**. File times keep nanoseconds in `*_ns` columns where identity matters.
- Booleans: `INTEGER` 0/1 with `CHECK`.
- JSON columns: validated by Rust (serde) before write; carry `"v": <n>` for versioning.
- The DB is **never** the source of truth for whether a file exists — always re-stat.
- Migrations are append-only. Never edit a released migration.

## Migration plan

| File | Phase | Contents |
|---|---|---|
| `0001_init.sql` | 1 | All tables below |
| later | as needed | Additive changes; each with a test that migrates a populated v(n-1) DB |

## Schema (`0001_init.sql`)

```sql
CREATE TABLE settings (
  key         TEXT PRIMARY KEY,
  value       TEXT NOT NULL,              -- JSON
  updated_at  INTEGER NOT NULL
);

CREATE TABLE monitored_folders (
  id             TEXT PRIMARY KEY,
  name           TEXT NOT NULL,
  path           TEXT NOT NULL UNIQUE,
  volume_uuid    TEXT,                    -- for reconnect matching
  enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  recursive      INTEGER NOT NULL DEFAULT 0 CHECK (recursive IN (0,1)),
  settings_json  TEXT NOT NULL DEFAULT '{"v":1}', -- include/exclude exts, excluded subfolders, stability override
  health         TEXT NOT NULL DEFAULT 'unknown'
                 CHECK (health IN ('unknown','healthy','paused','unavailable','permission_denied','error')),
  last_error     TEXT,
  file_count     INTEGER NOT NULL DEFAULT 0,
  last_scanned_at INTEGER,
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);

CREATE TABLE destination_roots (          -- approved roots for PathAuthority
  id          TEXT PRIMARY KEY,
  path        TEXT NOT NULL UNIQUE,
  created_at  INTEGER NOT NULL
);

CREATE TABLE categories (
  id               TEXT PRIMARY KEY,
  name             TEXT NOT NULL UNIQUE,
  description      TEXT,
  icon             TEXT NOT NULL,         -- lucide icon name
  color_token      TEXT NOT NULL,         -- e.g. 'cat-documents' (theme token, never a hex)
  extensions_json  TEXT NOT NULL,
  destination_path TEXT,
  priority         INTEGER NOT NULL DEFAULT 100,
  builtin          INTEGER NOT NULL DEFAULT 0 CHECK (builtin IN (0,1)),
  enabled          INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  created_at       INTEGER NOT NULL,
  updated_at       INTEGER NOT NULL
);

CREATE TABLE rules (
  id               TEXT PRIMARY KEY,
  name             TEXT NOT NULL,
  description      TEXT,
  enabled          INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0,1)),
  priority         INTEGER NOT NULL,
  source_folders_json TEXT NOT NULL DEFAULT '[]',
  conditions_json  TEXT NOT NULL,
  actions_json     TEXT NOT NULL,
  execution_mode   TEXT NOT NULL DEFAULT 'first_match'
                   CHECK (execution_mode IN ('first_match','continue','stop_after_move','stop_after_action','sequential')),
  origin           TEXT NOT NULL DEFAULT 'user' CHECK (origin IN ('user','preset','import')),
  last_run_at      INTEGER,
  stat_matched     INTEGER NOT NULL DEFAULT 0,
  stat_succeeded   INTEGER NOT NULL DEFAULT 0,
  stat_failed      INTEGER NOT NULL DEFAULT 0,
  created_at       INTEGER NOT NULL,
  updated_at       INTEGER NOT NULL
);
CREATE INDEX idx_rules_priority ON rules(enabled, priority);

CREATE TABLE file_records (
  id              TEXT PRIMARY KEY,
  folder_id       TEXT NOT NULL REFERENCES monitored_folders(id) ON DELETE CASCADE,
  path            TEXT NOT NULL UNIQUE,
  filename        TEXT NOT NULL,
  extension       TEXT,
  category        TEXT,
  size            INTEGER NOT NULL,
  dev             INTEGER NOT NULL,
  ino             INTEGER NOT NULL,
  created_at_ns   INTEGER,
  modified_at_ns  INTEGER NOT NULL,
  content_hash    TEXT,                   -- BLAKE3 hex; valid only if hash_* match current
  hash_size       INTEGER,
  hash_mtime_ns   INTEGER,
  last_scanned_at INTEGER NOT NULL
);
CREATE INDEX idx_files_folder   ON file_records(folder_id);
CREATE INDEX idx_files_identity ON file_records(dev, ino);
CREATE INDEX idx_files_size     ON file_records(size);
CREATE INDEX idx_files_hash     ON file_records(content_hash);
CREATE INDEX idx_files_category ON file_records(category);

CREATE TABLE operation_batches (
  id          TEXT PRIMARY KEY,
  trigger     TEXT NOT NULL CHECK (trigger IN ('realtime','schedule','manual','preview','undo','recovery')),
  plan_id     TEXT,
  total       INTEGER NOT NULL DEFAULT 0,
  started_at  INTEGER NOT NULL,
  completed_at INTEGER
);

CREATE TABLE operations (
  id               TEXT PRIMARY KEY,
  batch_id         TEXT REFERENCES operation_batches(id) ON DELETE SET NULL,
  file_record_id   TEXT REFERENCES file_records(id) ON DELETE SET NULL,
  operation_type   TEXT NOT NULL CHECK (operation_type IN ('move','copy','rename','trash','undo_move','undo_copy','undo_rename')),
  source_path      TEXT NOT NULL,
  destination_path TEXT NOT NULL,
  staging_path     TEXT,                  -- cross-volume temp file, for recovery
  original_name    TEXT NOT NULL,
  new_name         TEXT NOT NULL,
  dev              INTEGER, ino INTEGER, size INTEGER, mtime_ns INTEGER, -- identity snapshot
  content_hash     TEXT,                  -- for copy verification / copy undo
  cross_volume     INTEGER NOT NULL DEFAULT 0 CHECK (cross_volume IN (0,1)),
  rule_id          TEXT REFERENCES rules(id) ON DELETE SET NULL,
  status           TEXT NOT NULL CHECK (status IN ('PENDING','VALIDATING','IN_PROGRESS','VERIFYING',
                                                   'COMPLETED','FAILED','ROLLBACK_REQUIRED','ROLLED_BACK')),
  reversible       INTEGER NOT NULL DEFAULT 1 CHECK (reversible IN (0,1)),
  reversed_by      TEXT REFERENCES operations(id),
  error_code       TEXT,
  error_message    TEXT,
  started_at       INTEGER NOT NULL,
  completed_at     INTEGER
);
CREATE INDEX idx_ops_status    ON operations(status);
CREATE INDEX idx_ops_batch     ON operations(batch_id);
CREATE INDEX idx_ops_started   ON operations(started_at DESC);
CREATE INDEX idx_ops_rule      ON operations(rule_id);
CREATE INDEX idx_ops_source    ON operations(source_path);
CREATE INDEX idx_ops_dest      ON operations(destination_path);

CREATE TABLE review_queue (                -- "ask" conflicts and "mark for review"
  id           TEXT PRIMARY KEY,
  path         TEXT NOT NULL,
  reason       TEXT NOT NULL,
  rule_id      TEXT REFERENCES rules(id) ON DELETE SET NULL,
  proposed_json TEXT,
  created_at   INTEGER NOT NULL,
  resolved_at  INTEGER
);

CREATE TABLE schedules (
  id              TEXT PRIMARY KEY,
  rule_id         TEXT REFERENCES rules(id) ON DELETE CASCADE,
  folder_id       TEXT REFERENCES monitored_folders(id) ON DELETE CASCADE,
  schedule_type   TEXT NOT NULL CHECK (schedule_type IN ('realtime','interval','daily','weekly','on_launch','manual')),
  schedule_config TEXT NOT NULL,          -- JSON: interval_secs, time, weekdays, quiet_hours, catch_up
  enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  last_run_at     INTEGER,
  next_run_at     INTEGER,
  CHECK (rule_id IS NOT NULL OR folder_id IS NOT NULL)
);
CREATE INDEX idx_sched_next ON schedules(enabled, next_run_at);

CREATE TABLE exclusions (
  id             TEXT PRIMARY KEY,
  folder_id      TEXT REFERENCES monitored_folders(id) ON DELETE CASCADE, -- NULL = global
  path           TEXT NOT NULL,
  exclusion_type TEXT NOT NULL CHECK (exclusion_type IN ('path','glob','extension')),
  created_at     INTEGER NOT NULL
);

CREATE TABLE duplicate_groups (
  id           TEXT PRIMARY KEY,
  content_hash TEXT NOT NULL UNIQUE,
  file_size    INTEGER NOT NULL,
  ignored      INTEGER NOT NULL DEFAULT 0 CHECK (ignored IN (0,1)),
  created_at   INTEGER NOT NULL
);
CREATE TABLE duplicate_members (
  group_id       TEXT NOT NULL REFERENCES duplicate_groups(id) ON DELETE CASCADE,
  file_record_id TEXT NOT NULL REFERENCES file_records(id) ON DELETE CASCADE,
  is_keeper      INTEGER NOT NULL DEFAULT 0 CHECK (is_keeper IN (0,1)),
  PRIMARY KEY (group_id, file_record_id)
);

CREATE TABLE watcher_errors (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  folder_id  TEXT REFERENCES monitored_folders(id) ON DELETE CASCADE,
  code       TEXT NOT NULL,
  message    TEXT NOT NULL,
  occurred_at INTEGER NOT NULL
);
CREATE INDEX idx_werr_folder ON watcher_errors(folder_id, occurred_at DESC);

CREATE TABLE license_state (               -- metadata only; key lives in Keychain
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  status        TEXT NOT NULL,
  licensee      TEXT,
  activated_at  INTEGER,
  last_validated_at INTEGER
);
```

## Retention

- `operations`: kept until user clears history (configurable auto-prune > N days, default off).
- `watcher_errors`: keep last 500 per folder.
- `file_records`: rows removed when reconciliation confirms the file is gone.
