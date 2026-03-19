-- Seahorse MVP SQLite schema
-- Source: docs/mvp-design-and-roadmap.md

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS files (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  namespace TEXT NOT NULL DEFAULT 'default',
  filename TEXT NOT NULL,
  source_type TEXT,
  source_uri TEXT,
  file_hash TEXT NOT NULL,
  metadata_json TEXT,
  ingest_status TEXT NOT NULL CHECK(ingest_status IN ('pending_index','ready','partial','deleted')) DEFAULT 'pending_index',
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS chunks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  namespace TEXT NOT NULL DEFAULT 'default',
  file_id INTEGER NOT NULL REFERENCES files(id),
  chunk_index INTEGER NOT NULL,
  chunk_text TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  token_count INTEGER,
  model_id TEXT NOT NULL,
  dimension INTEGER NOT NULL,
  metadata_json TEXT,
  index_status TEXT NOT NULL CHECK(index_status IN ('pending','ready','failed','deleted')) DEFAULT 'pending',
  is_deleted INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0,1)),
  deleted_at DATETIME,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(file_id, chunk_index)
);

CREATE TABLE IF NOT EXISTS tags (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  namespace TEXT NOT NULL DEFAULT 'default',
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL,
  category TEXT,
  usage_count INTEGER NOT NULL DEFAULT 0,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(namespace, normalized_name)
);

CREATE TABLE IF NOT EXISTS chunk_tags (
  chunk_id INTEGER NOT NULL REFERENCES chunks(id),
  tag_id INTEGER NOT NULL REFERENCES tags(id),
  confidence REAL NOT NULL DEFAULT 1.0,
  source TEXT NOT NULL DEFAULT 'auto',
  PRIMARY KEY(chunk_id, tag_id)
);

CREATE TABLE IF NOT EXISTS repair_queue (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  namespace TEXT NOT NULL DEFAULT 'default',
  task_type TEXT NOT NULL,
  target_type TEXT NOT NULL,
  target_id INTEGER,
  payload_json TEXT,
  status TEXT NOT NULL CHECK(status IN ('pending','running','succeeded','failed','deadletter')) DEFAULT 'pending',
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS maintenance_jobs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  job_type TEXT NOT NULL,
  namespace TEXT NOT NULL DEFAULT 'default',
  payload_json TEXT,
  status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','cancelled')) DEFAULT 'queued',
  progress TEXT,
  result_summary TEXT,
  error_message TEXT,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  started_at DATETIME,
  finished_at DATETIME
);

CREATE TABLE IF NOT EXISTS schema_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_chunks_namespace ON chunks(namespace);
CREATE INDEX IF NOT EXISTS idx_chunks_deleted ON chunks(namespace, is_deleted);
CREATE INDEX IF NOT EXISTS idx_chunks_index_status ON chunks(namespace, index_status);
CREATE INDEX IF NOT EXISTS idx_repair_queue_status ON repair_queue(namespace, status);
CREATE INDEX IF NOT EXISTS idx_jobs_namespace_status ON maintenance_jobs(namespace, status);
CREATE INDEX IF NOT EXISTS idx_files_namespace_file ON files(namespace, file_hash);

INSERT OR IGNORE INTO schema_meta(key, value) VALUES
  ('schema_version', '2'),
  ('index_version', '1'),
  ('embedding_model_id', 'unknown'),
  ('embedding_dimension', '0');

CREATE TRIGGER IF NOT EXISTS trg_files_updated_at
AFTER UPDATE ON files
FOR EACH ROW
BEGIN
  UPDATE files SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_chunks_updated_at
AFTER UPDATE ON chunks
FOR EACH ROW
BEGIN
  UPDATE chunks SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_tags_updated_at
AFTER UPDATE ON tags
FOR EACH ROW
BEGIN
  UPDATE tags SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_repair_queue_updated_at
AFTER UPDATE ON repair_queue
FOR EACH ROW
BEGIN
  UPDATE repair_queue SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_schema_meta_updated_at
AFTER UPDATE ON schema_meta
FOR EACH ROW
BEGIN
  UPDATE schema_meta SET updated_at = CURRENT_TIMESTAMP WHERE key = NEW.key;
END;

COMMIT;
