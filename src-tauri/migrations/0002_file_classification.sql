-- 0002_file_classification.sql — classification results stored per file (Phase 2).
-- Source of truth: docs/DATABASE.md. Never edit after release.

ALTER TABLE file_records ADD COLUMN mime TEXT;
ALTER TABLE file_records ADD COLUMN confidence TEXT NOT NULL DEFAULT 'high' CHECK (confidence IN ('high','low'));
ALTER TABLE file_records ADD COLUMN is_package INTEGER NOT NULL DEFAULT 0 CHECK (is_package IN (0,1));
CREATE INDEX idx_files_scanned ON file_records(folder_id, last_scanned_at);
