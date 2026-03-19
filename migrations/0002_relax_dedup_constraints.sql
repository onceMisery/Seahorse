-- Relax file_hash/content_hash uniqueness so dedup_mode upsert/allow can coexist.

PRAGMA foreign_keys = OFF;

BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS files_v2 (
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

INSERT INTO files_v2 (
  id,
  namespace,
  filename,
  source_type,
  source_uri,
  file_hash,
  metadata_json,
  ingest_status,
  created_at,
  updated_at
)
SELECT
  id,
  namespace,
  filename,
  source_type,
  source_uri,
  file_hash,
  metadata_json,
  ingest_status,
  created_at,
  updated_at
FROM files;

CREATE TABLE IF NOT EXISTS chunks_v2 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  namespace TEXT NOT NULL DEFAULT 'default',
  file_id INTEGER NOT NULL REFERENCES files_v2(id),
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

INSERT INTO chunks_v2 (
  id,
  namespace,
  file_id,
  chunk_index,
  chunk_text,
  content_hash,
  token_count,
  model_id,
  dimension,
  metadata_json,
  index_status,
  is_deleted,
  deleted_at,
  created_at,
  updated_at
)
SELECT
  id,
  namespace,
  file_id,
  chunk_index,
  chunk_text,
  content_hash,
  token_count,
  model_id,
  dimension,
  metadata_json,
  index_status,
  is_deleted,
  deleted_at,
  created_at,
  updated_at
FROM chunks;

CREATE TABLE IF NOT EXISTS chunk_tags_v2 (
  chunk_id INTEGER NOT NULL REFERENCES chunks_v2(id),
  tag_id INTEGER NOT NULL REFERENCES tags(id),
  confidence REAL NOT NULL DEFAULT 1.0,
  source TEXT NOT NULL DEFAULT 'auto',
  PRIMARY KEY(chunk_id, tag_id)
);

INSERT INTO chunk_tags_v2 (chunk_id, tag_id, confidence, source)
SELECT chunk_id, tag_id, confidence, source
FROM chunk_tags;

DROP TABLE chunk_tags;
DROP TABLE chunks;
DROP TABLE files;

ALTER TABLE files_v2 RENAME TO files;
ALTER TABLE chunks_v2 RENAME TO chunks;
ALTER TABLE chunk_tags_v2 RENAME TO chunk_tags;

CREATE INDEX IF NOT EXISTS idx_chunks_namespace ON chunks(namespace);
CREATE INDEX IF NOT EXISTS idx_chunks_deleted ON chunks(namespace, is_deleted);
CREATE INDEX IF NOT EXISTS idx_chunks_index_status ON chunks(namespace, index_status);
CREATE INDEX IF NOT EXISTS idx_repair_queue_status ON repair_queue(namespace, status);
CREATE INDEX IF NOT EXISTS idx_jobs_namespace_status ON maintenance_jobs(namespace, status);
CREATE INDEX IF NOT EXISTS idx_files_namespace_file ON files(namespace, file_hash);

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

UPDATE schema_meta
SET value = '2'
WHERE key = 'schema_version';

COMMIT;

PRAGMA foreign_keys = ON;
