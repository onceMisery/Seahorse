use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::models::{
    ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, PersistedChunk, PersistedFile,
    PersistedIngest, RecallChunkRecord, TagWrite,
};
use super::schema::{validate_schema_meta, SchemaExpectation, SchemaMetaSnapshot};
use super::{StorageError, StorageResult};

#[derive(Debug)]
pub struct SqliteRepository {
    connection: Connection,
}

impl SqliteRepository {
    pub fn new(connection: Connection) -> StorageResult<Self> {
        enable_foreign_keys(&connection)?;
        Ok(Self { connection })
    }

    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let connection = Connection::open(path)?;
        Self::new(connection)
    }

    pub fn open_in_memory() -> StorageResult<Self> {
        let connection = Connection::open_in_memory()?;
        Self::new(connection)
    }

    pub fn validate_schema(
        &self,
        expected: &SchemaExpectation,
    ) -> StorageResult<SchemaMetaSnapshot> {
        validate_schema_meta(&self.connection, expected)
    }

    pub fn find_file_by_hash(
        &self,
        namespace: &str,
        file_hash: &str,
    ) -> StorageResult<Option<PersistedFile>> {
        let file = self
            .connection
            .query_row(
                "SELECT id, namespace, filename, file_hash, ingest_status
                 FROM files
                 WHERE namespace = ?1 AND file_hash = ?2",
                params![namespace, file_hash],
                |row| {
                    Ok(PersistedFile {
                        id: row.get(0)?,
                        namespace: row.get(1)?,
                        filename: row.get(2)?,
                        file_hash: row.get(3)?,
                        ingest_status: row.get(4)?,
                    })
                },
            )
            .optional()?;

        Ok(file)
    }

    pub fn list_chunks_by_file_id(&self, file_id: i64) -> StorageResult<Vec<PersistedChunk>> {
        let mut statement = self.connection.prepare(
            "SELECT id, file_id, chunk_index, content_hash, index_status
             FROM chunks
             WHERE file_id = ?1
             ORDER BY chunk_index ASC",
        )?;
        let rows = statement.query_map([file_id], |row| {
            Ok(PersistedChunk {
                id: row.get(0)?,
                file_id: row.get(1)?,
                chunk_index: row.get(2)?,
                content_hash: row.get(3)?,
                index_status: row.get(4)?,
            })
        })?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(row?);
        }
        Ok(chunks)
    }

    pub fn get_chunk_record(&self, chunk_id: i64) -> StorageResult<Option<RecallChunkRecord>> {
        let record = self
            .connection
            .query_row(
                "SELECT
                    c.id,
                    c.file_id,
                    c.namespace,
                    c.chunk_text,
                    f.filename,
                    f.source_type,
                    COALESCE(c.metadata_json, f.metadata_json)
                 FROM chunks c
                 JOIN files f ON f.id = c.file_id
                 WHERE c.id = ?1
                   AND c.is_deleted = 0
                   AND c.index_status != 'deleted'
                   AND f.ingest_status != 'deleted'",
                [chunk_id],
                |row| {
                    Ok(RecallChunkRecord {
                        chunk_id: row.get(0)?,
                        file_id: row.get(1)?,
                        namespace: row.get(2)?,
                        chunk_text: row.get(3)?,
                        source_file: row.get(4)?,
                        source_type: row.get(5)?,
                        metadata_json: row.get(6)?,
                        tags: Vec::new(),
                    })
                },
            )
            .optional()?;

        match record {
            Some(mut record) => {
                record.tags = self.list_tags_by_chunk_id(record.chunk_id)?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    pub fn with_transaction<T, F>(&mut self, operation: F) -> StorageResult<T>
    where
        F: FnOnce(&Transaction<'_>) -> StorageResult<T>,
    {
        let transaction = self.connection.transaction()?;
        let output = operation(&transaction)?;
        transaction.commit()?;
        Ok(output)
    }

    pub fn write_ingest_batch(&mut self, batch: &IngestWriteBatch) -> StorageResult<PersistedIngest> {
        self.with_transaction(|transaction| write_ingest_batch(transaction, batch))
    }

    pub fn update_indexing_result(
        &mut self,
        file_id: i64,
        chunk_ids: &[i64],
        file_status: &str,
        chunk_status: &str,
    ) -> StorageResult<()> {
        self.with_transaction(|transaction| {
            transaction.execute(
                "UPDATE files SET ingest_status = ?1 WHERE id = ?2",
                params![file_status, file_id],
            )?;

            for chunk_id in chunk_ids {
                transaction.execute(
                    "UPDATE chunks SET index_status = ?1 WHERE id = ?2",
                    params![chunk_status, chunk_id],
                )?;
            }

            Ok(())
        })
    }

    pub fn enqueue_repair_task(
        &mut self,
        namespace: &str,
        task_type: &str,
        target_type: &str,
        target_id: Option<i64>,
        payload_json: Option<&str>,
    ) -> StorageResult<i64> {
        self.connection.execute(
            "INSERT INTO repair_queue (
                namespace,
                task_type,
                target_type,
                target_id,
                payload_json,
                status
            ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            params![namespace, task_type, target_type, target_id, payload_json],
        )?;

        Ok(self.connection.last_insert_rowid())
    }

    fn list_tags_by_chunk_id(&self, chunk_id: i64) -> StorageResult<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT t.normalized_name
             FROM chunk_tags ct
             JOIN tags t ON t.id = ct.tag_id
             WHERE ct.chunk_id = ?1
             ORDER BY t.normalized_name ASC",
        )?;
        let rows = statement.query_map([chunk_id], |row| row.get::<_, String>(0))?;

        let mut tags = Vec::new();
        for row in rows {
            tags.push(row?);
        }
        Ok(tags)
    }
}

fn enable_foreign_keys(connection: &Connection) -> StorageResult<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn write_ingest_batch(
    transaction: &Transaction<'_>,
    batch: &IngestWriteBatch,
) -> StorageResult<PersistedIngest> {
    let file = insert_file(transaction, &batch.file)?;

    let mut chunks = Vec::with_capacity(batch.chunks.len());
    let mut chunk_ids = BTreeMap::new();
    for chunk in &batch.chunks {
        let persisted = insert_chunk(transaction, file.id, chunk)?;
        chunk_ids.insert(persisted.chunk_index, persisted.id);
        chunks.push(persisted);
    }

    let mut tag_ids = BTreeMap::new();
    for tag in &batch.tags {
        let tag_id = ensure_tag(transaction, tag)?;
        tag_ids.insert(tag.normalized_name.clone(), tag_id);
    }

    for link in &batch.chunk_tags {
        let chunk_id = chunk_ids
            .get(&link.chunk_index)
            .copied()
            .ok_or_else(|| StorageError::InvalidBatchReference {
                message: format!("chunk_index={} not found in batch", link.chunk_index),
            })?;
        let tag_id = tag_ids
            .get(&link.tag_normalized_name)
            .copied()
            .ok_or_else(|| StorageError::InvalidBatchReference {
                message: format!(
                    "tag_normalized_name={} not found in batch",
                    link.tag_normalized_name
                ),
            })?;
        insert_chunk_tag(transaction, chunk_id, tag_id, link)?;
    }

    Ok(PersistedIngest {
        file,
        chunks,
        tag_ids,
    })
}

fn insert_file(transaction: &Transaction<'_>, file: &FileWrite) -> StorageResult<PersistedFile> {
    transaction.execute(
        "INSERT INTO files (
            namespace,
            filename,
            source_type,
            source_uri,
            file_hash,
            metadata_json,
            ingest_status
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &file.namespace,
            &file.filename,
            &file.source_type,
            &file.source_uri,
            &file.file_hash,
            &file.metadata_json,
            &file.ingest_status,
        ],
    )?;

    Ok(PersistedFile {
        id: transaction.last_insert_rowid(),
        namespace: file.namespace.clone(),
        filename: file.filename.clone(),
        file_hash: file.file_hash.clone(),
        ingest_status: file.ingest_status.clone(),
    })
}

fn insert_chunk(
    transaction: &Transaction<'_>,
    file_id: i64,
    chunk: &ChunkWrite,
) -> StorageResult<PersistedChunk> {
    transaction.execute(
        "INSERT INTO chunks (
            namespace,
            file_id,
            chunk_index,
            chunk_text,
            content_hash,
            token_count,
            model_id,
            dimension,
            metadata_json,
            index_status
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            &chunk.namespace,
            file_id,
            chunk.chunk_index,
            &chunk.chunk_text,
            &chunk.content_hash,
            &chunk.token_count,
            &chunk.model_id,
            chunk.dimension,
            &chunk.metadata_json,
            &chunk.index_status,
        ],
    )?;

    Ok(PersistedChunk {
        id: transaction.last_insert_rowid(),
        file_id,
        chunk_index: chunk.chunk_index,
        content_hash: chunk.content_hash.clone(),
        index_status: chunk.index_status.clone(),
    })
}

fn ensure_tag(transaction: &Transaction<'_>, tag: &TagWrite) -> StorageResult<i64> {
    transaction.execute(
        "INSERT OR IGNORE INTO tags (
            namespace,
            name,
            normalized_name,
            category
        ) VALUES (?1, ?2, ?3, ?4)",
        params![
            &tag.namespace,
            &tag.name,
            &tag.normalized_name,
            &tag.category
        ],
    )?;

    let tag_id = transaction.query_row(
        "SELECT id FROM tags WHERE namespace = ?1 AND normalized_name = ?2",
        params![&tag.namespace, &tag.normalized_name],
        |row| row.get::<_, i64>(0),
    )?;

    Ok(tag_id)
}

fn insert_chunk_tag(
    transaction: &Transaction<'_>,
    chunk_id: i64,
    tag_id: i64,
    link: &ChunkTagInsert,
) -> StorageResult<()> {
    transaction.execute(
        "INSERT INTO chunk_tags (
            chunk_id,
            tag_id,
            confidence,
            source
        ) VALUES (?1, ?2, ?3, ?4)",
        params![chunk_id, tag_id, link.confidence, &link.source],
    )?;

    transaction.execute(
        "UPDATE tags SET usage_count = usage_count + 1 WHERE id = ?1",
        [tag_id],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::SqliteRepository;
    use crate::storage::models::{ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, TagWrite};
    use crate::storage::{SchemaExpectation, StorageError};

    const MIGRATION: &str = include_str!("../../../../migrations/0001_init.sql");

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        connection.execute_batch(MIGRATION).expect("apply migration");
        SqliteRepository::new(connection).expect("repository")
    }

    #[test]
    fn validates_schema_through_repository() {
        let repository = repository_with_schema();
        let snapshot = repository
            .validate_schema(&SchemaExpectation::new("1", "1", "unknown", 0))
            .expect("schema valid");

        assert_eq!(snapshot.schema_version, "1");
    }

    #[test]
    fn writes_file_chunks_tags_and_relations_in_one_transaction() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("demo.txt", "file-hash-1"),
            chunks: vec![
                ChunkWrite::new(0, "alpha", "chunk-hash-1", "test-model", 3),
                ChunkWrite::new(1, "beta", "chunk-hash-2", "test-model", 3),
            ],
            tags: vec![
                TagWrite::new("Project", "project"),
                TagWrite::new("Rust", "rust"),
            ],
            chunk_tags: vec![
                ChunkTagInsert::new(0, "project"),
                ChunkTagInsert::new(0, "rust"),
                ChunkTagInsert::new(1, "rust"),
            ],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");

        assert_eq!(persisted.file.filename, "demo.txt");
        assert_eq!(persisted.chunks.len(), 2);
        assert_eq!(persisted.tag_ids.len(), 2);

        let file_count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .expect("count files");
        let chunk_count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .expect("count chunks");
        let tag_count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .expect("count tags");
        let relation_count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM chunk_tags", [], |row| row.get(0))
            .expect("count chunk_tags");

        assert_eq!(file_count, 1);
        assert_eq!(chunk_count, 2);
        assert_eq!(tag_count, 2);
        assert_eq!(relation_count, 3);
    }

    #[test]
    fn rolls_back_transaction_when_batch_references_are_invalid() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("demo.txt", "file-hash-rollback"),
            chunks: vec![ChunkWrite::new(0, "alpha", "chunk-hash-rollback", "test-model", 3)],
            tags: vec![TagWrite::new("Project", "project")],
            chunk_tags: vec![ChunkTagInsert::new(0, "missing-tag")],
        };

        let error = repository
            .write_ingest_batch(&batch)
            .expect_err("batch should fail");

        match error {
            StorageError::InvalidBatchReference { .. } => {}
            other => panic!("unexpected error: {other}"),
        }

        let file_count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .expect("count files after rollback");
        let chunk_count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .expect("count chunks after rollback");

        assert_eq!(file_count, 0);
        assert_eq!(chunk_count, 0);
    }

    #[test]
    fn finds_file_by_hash_and_lists_chunks() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("demo.txt", "hash-find-me"),
            chunks: vec![ChunkWrite::new(0, "alpha", "chunk-find-me", "test-model", 3)],
            tags: vec![],
            chunk_tags: vec![],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");

        let file = repository
            .find_file_by_hash("default", "hash-find-me")
            .expect("find file")
            .expect("file should exist");
        let chunks = repository
            .list_chunks_by_file_id(file.id)
            .expect("list chunks");

        assert_eq!(file.id, persisted.file.id);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, persisted.chunks[0].id);
    }

    #[test]
    fn updates_indexing_result_and_enqueues_repair() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("demo.txt", "hash-status"),
            chunks: vec![ChunkWrite::new(0, "alpha", "chunk-status", "test-model", 3)],
            tags: vec![],
            chunk_tags: vec![],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");
        let chunk_ids = persisted.chunks.iter().map(|chunk| chunk.id).collect::<Vec<_>>();

        repository
            .update_indexing_result(persisted.file.id, &chunk_ids, "partial", "failed")
            .expect("update indexing result");
        let repair_id = repository
            .enqueue_repair_task(
                "default",
                "index_insert",
                "file",
                Some(persisted.file.id),
                Some("{\"test\":true}"),
            )
            .expect("enqueue repair");

        assert!(repair_id > 0);

        let file = repository
            .find_file_by_hash("default", "hash-status")
            .expect("find file")
            .expect("file exists");
        let chunks = repository
            .list_chunks_by_file_id(file.id)
            .expect("list chunks");
        let repair_count: i64 = repository
            .connection
            .query_row("SELECT COUNT(*) FROM repair_queue", [], |row| row.get(0))
            .expect("count repair_queue");

        assert_eq!(file.ingest_status, "partial");
        assert_eq!(chunks[0].index_status, "failed");
        assert_eq!(repair_count, 1);
    }

    #[test]
    fn loads_chunk_record_with_joined_tags() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("doc.txt", "hash-record"),
            chunks: vec![ChunkWrite::new(0, "alpha", "chunk-record", "test-model", 3)],
            tags: vec![TagWrite::new("Project", "project"), TagWrite::new("Rust", "rust")],
            chunk_tags: vec![
                ChunkTagInsert::new(0, "project"),
                ChunkTagInsert::new(0, "rust"),
            ],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");

        let record = repository
            .get_chunk_record(persisted.chunks[0].id)
            .expect("load record")
            .expect("record exists");

        assert_eq!(record.chunk_id, persisted.chunks[0].id);
        assert_eq!(record.source_file, "doc.txt");
        assert_eq!(record.tags, vec!["project".to_owned(), "rust".to_owned()]);
    }
}
