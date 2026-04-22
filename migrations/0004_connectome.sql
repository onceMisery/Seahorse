-- Add connectome co-occurrence graph for synapse foundation.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS connectome (
  namespace TEXT NOT NULL DEFAULT 'default',
  tag_i INTEGER NOT NULL REFERENCES tags(id),
  tag_j INTEGER NOT NULL REFERENCES tags(id),
  weight REAL NOT NULL DEFAULT 1.0,
  cooccur_count INTEGER NOT NULL DEFAULT 1,
  last_updated DATETIME DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY(namespace, tag_i, tag_j)
);

CREATE INDEX IF NOT EXISTS idx_connectome_tag_i
ON connectome(namespace, tag_i);

CREATE INDEX IF NOT EXISTS idx_connectome_tag_j
ON connectome(namespace, tag_j);

UPDATE schema_meta
SET value = '4'
WHERE key = 'schema_version';

COMMIT;
