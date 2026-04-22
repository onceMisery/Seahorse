-- Add design-all phase1 persistence primitives.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS embedding_cache (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  namespace TEXT NOT NULL DEFAULT 'default',
  content_hash TEXT NOT NULL,
  model_id TEXT NOT NULL,
  dimension INTEGER NOT NULL,
  vector_blob BLOB NOT NULL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(namespace, content_hash, model_id)
);

CREATE TABLE IF NOT EXISTS retrieval_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  namespace TEXT NOT NULL DEFAULT 'default',
  query_text TEXT,
  query_hash TEXT,
  mode TEXT NOT NULL,
  worldview TEXT,
  entropy REAL,
  result_count INTEGER NOT NULL DEFAULT 0,
  total_time_us INTEGER,
  spike_depth INTEGER,
  emergent_count INTEGER,
  params_snapshot TEXT,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_embedding_cache_lookup
ON embedding_cache(namespace, content_hash, model_id);

CREATE INDEX IF NOT EXISTS idx_retrieval_log_time
ON retrieval_log(namespace, created_at, id);

UPDATE schema_meta
SET value = '3'
WHERE key = 'schema_version';

COMMIT;
