use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, Transaction};

use super::models::{
    ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, PersistedChunk, PersistedFile,
    PersistedIngest, TagWrite,
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
}
