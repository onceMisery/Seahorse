use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::models::{
    CachedEmbedding, ChunkTagInsert, ChunkWrite, ConnectomeEdgeRecord, ConnectomeHealthSnapshot,
    FileWrite, IngestWriteBatch, MaintenanceJob, PersistedChunk, PersistedDeletion, PersistedFile,
    PersistedIngest, PersistedReplacement, RebuildChunkRecord, RecallChunkRecord, RepairTask,
    RetrievalLogRecord, RetrievalLogWrite, StorageStatsSnapshot, TagWrite,
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
        Ok(self
            .list_active_files_by_hash(namespace, file_hash)?
            .into_iter()
            .next())
    }

    pub fn list_active_files_by_hash(
        &self,
        namespace: &str,
        file_hash: &str,
    ) -> StorageResult<Vec<PersistedFile>> {
        let mut statement = self.connection.prepare(
            "SELECT id, namespace, filename, file_hash, ingest_status
             FROM files
             WHERE namespace = ?1
               AND file_hash = ?2
               AND ingest_status != 'deleted'
             ORDER BY id DESC",
        )?;
        let rows = statement.query_map(params![namespace, file_hash], |row| {
            Ok(PersistedFile {
                id: row.get(0)?,
                namespace: row.get(1)?,
                filename: row.get(2)?,
                file_hash: row.get(3)?,
                ingest_status: row.get(4)?,
            })
        })?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
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

    pub fn get_cached_embedding(
        &self,
        namespace: &str,
        content_hash: &str,
        model_id: &str,
    ) -> StorageResult<Option<CachedEmbedding>> {
        self.connection
            .query_row(
                "SELECT
                    id,
                    namespace,
                    content_hash,
                    model_id,
                    dimension,
                    vector_blob,
                    created_at
                 FROM embedding_cache
                 WHERE namespace = ?1
                   AND content_hash = ?2
                   AND model_id = ?3",
                params![namespace, content_hash, model_id],
                map_cached_embedding,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn put_cached_embedding(
        &mut self,
        namespace: &str,
        content_hash: &str,
        model_id: &str,
        vector: &[f32],
    ) -> StorageResult<CachedEmbedding> {
        let dimension =
            i64::try_from(vector.len()).map_err(|_| StorageError::InvalidBatchReference {
                message: "embedding vector length exceeds i64".to_owned(),
            })?;
        let vector_blob = serialize_embedding(vector);

        self.connection.execute(
            "INSERT INTO embedding_cache (
                namespace,
                content_hash,
                model_id,
                dimension,
                vector_blob
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(namespace, content_hash, model_id)
            DO UPDATE SET
                dimension = excluded.dimension,
                vector_blob = excluded.vector_blob",
            params![namespace, content_hash, model_id, dimension, vector_blob],
        )?;

        self.get_cached_embedding(namespace, content_hash, model_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows.into())
    }

    pub fn append_retrieval_log(
        &mut self,
        namespace: &str,
        log: &RetrievalLogWrite,
    ) -> StorageResult<RetrievalLogRecord> {
        self.connection.execute(
            "INSERT INTO retrieval_log (
                namespace,
                query_text,
                query_hash,
                mode,
                worldview,
                entropy,
                result_count,
                total_time_us,
                spike_depth,
                emergent_count,
                params_snapshot
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                namespace,
                log.query_text,
                log.query_hash,
                log.mode,
                log.worldview,
                log.entropy,
                log.result_count,
                log.total_time_us,
                log.spike_depth,
                log.emergent_count,
                log.params_snapshot,
            ],
        )?;

        let log_id = self.connection.last_insert_rowid();
        self.connection
            .query_row(
                "SELECT
                    id,
                    namespace,
                    query_text,
                    query_hash,
                    mode,
                    worldview,
                    entropy,
                    result_count,
                    total_time_us,
                    spike_depth,
                    emergent_count,
                    params_snapshot,
                    created_at
                 FROM retrieval_log
                 WHERE id = ?1",
                [log_id],
                map_retrieval_log_record,
            )
            .map_err(Into::into)
    }

    pub fn list_retrieval_logs(
        &self,
        namespace: &str,
        limit: usize,
    ) -> StorageResult<Vec<RetrievalLogRecord>> {
        let limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut statement = self.connection.prepare(
            "SELECT
                id,
                namespace,
                query_text,
                query_hash,
                mode,
                worldview,
                entropy,
                result_count,
                total_time_us,
                spike_depth,
                emergent_count,
                params_snapshot,
                created_at
             FROM retrieval_log
             WHERE namespace = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![namespace, limit], map_retrieval_log_record)?;

        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }

    pub fn list_connectome_neighbors(
        &self,
        namespace: &str,
        tag_normalized_name: &str,
        limit: usize,
    ) -> StorageResult<Vec<ConnectomeEdgeRecord>> {
        let limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut statement = self.connection.prepare(
            "SELECT
                namespace,
                source_tag,
                target_tag,
                weight,
                cooccur_count,
                last_updated
             FROM (
                SELECT
                    c.namespace AS namespace,
                    src.normalized_name AS source_tag,
                    dst.normalized_name AS target_tag,
                    c.weight AS weight,
                    c.cooccur_count AS cooccur_count,
                    c.last_updated AS last_updated
                FROM connectome c
                JOIN tags src ON src.id = c.tag_i
                JOIN tags dst ON dst.id = c.tag_j
                WHERE c.namespace = ?1
                  AND src.normalized_name = ?2
                UNION ALL
                SELECT
                    c.namespace AS namespace,
                    src.normalized_name AS source_tag,
                    dst.normalized_name AS target_tag,
                    c.weight AS weight,
                    c.cooccur_count AS cooccur_count,
                    c.last_updated AS last_updated
                FROM connectome c
                JOIN tags src ON src.id = c.tag_j
                JOIN tags dst ON dst.id = c.tag_i
                WHERE c.namespace = ?1
                  AND src.normalized_name = ?2
             )
             ORDER BY cooccur_count DESC, weight DESC, target_tag ASC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![namespace, tag_normalized_name.to_ascii_lowercase(), limit],
            map_connectome_edge_record,
        )?;

        let mut edges = Vec::new();
        for row in rows {
            edges.push(row?);
        }
        Ok(edges)
    }

    pub fn load_connectome_health(
        &self,
        namespace: &str,
    ) -> StorageResult<ConnectomeHealthSnapshot> {
        const ACTIVE_CONNECTOME_PAIRS_CTE: &str = "
            WITH active_pairs AS (
                SELECT
                    CASE WHEN ct1.tag_id < ct2.tag_id THEN ct1.tag_id ELSE ct2.tag_id END AS tag_i,
                    CASE WHEN ct1.tag_id < ct2.tag_id THEN ct2.tag_id ELSE ct1.tag_id END AS tag_j,
                    COUNT(*) AS expected_count
                FROM chunks c
                JOIN files f ON f.id = c.file_id
                JOIN chunk_tags ct1 ON ct1.chunk_id = c.id
                JOIN chunk_tags ct2 ON ct2.chunk_id = c.id AND ct1.tag_id < ct2.tag_id
                JOIN tags t1 ON t1.id = ct1.tag_id
                JOIN tags t2 ON t2.id = ct2.tag_id
                WHERE c.namespace = ?1
                  AND f.namespace = ?1
                  AND t1.namespace = ?1
                  AND t2.namespace = ?1
                  AND c.is_deleted = 0
                  AND c.index_status != 'deleted'
                  AND f.ingest_status != 'deleted'
                GROUP BY tag_i, tag_j
            ),
            actual_pairs AS (
                SELECT tag_i, tag_j, cooccur_count, weight
                FROM connectome
                WHERE namespace = ?1
            )";

        let expected_edge_count: i64 = self.connection.query_row(
            &format!(
                "{ACTIVE_CONNECTOME_PAIRS_CTE}
                 SELECT COUNT(*) FROM active_pairs"
            ),
            [namespace],
            |row| row.get(0),
        )?;
        let actual_edge_count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM connectome WHERE namespace = ?1",
            [namespace],
            |row| row.get(0),
        )?;
        let missing_edge_count: i64 = self.connection.query_row(
            &format!(
                "{ACTIVE_CONNECTOME_PAIRS_CTE}
                 SELECT COUNT(*)
                 FROM active_pairs e
                 LEFT JOIN actual_pairs a
                   ON a.tag_i = e.tag_i AND a.tag_j = e.tag_j
                 WHERE a.tag_i IS NULL"
            ),
            [namespace],
            |row| row.get(0),
        )?;
        let stale_edge_count: i64 = self.connection.query_row(
            &format!(
                "{ACTIVE_CONNECTOME_PAIRS_CTE}
                 SELECT COUNT(*)
                 FROM actual_pairs a
                 LEFT JOIN active_pairs e
                   ON e.tag_i = a.tag_i AND e.tag_j = a.tag_j
                 WHERE e.tag_i IS NULL"
            ),
            [namespace],
            |row| row.get(0),
        )?;
        let cooccur_mismatch_count: i64 = self.connection.query_row(
            &format!(
                "{ACTIVE_CONNECTOME_PAIRS_CTE}
                 SELECT COUNT(*)
                 FROM active_pairs e
                 JOIN actual_pairs a
                   ON a.tag_i = e.tag_i AND a.tag_j = e.tag_j
                 WHERE a.cooccur_count != e.expected_count"
            ),
            [namespace],
            |row| row.get(0),
        )?;
        let weight_mismatch_count: i64 = self.connection.query_row(
            &format!(
                "{ACTIVE_CONNECTOME_PAIRS_CTE}
                 SELECT COUNT(*)
                 FROM active_pairs e
                 JOIN actual_pairs a
                   ON a.tag_i = e.tag_i AND a.tag_j = e.tag_j
                 WHERE ABS(a.weight - CAST(e.expected_count AS REAL)) > 0.0001"
            ),
            [namespace],
            |row| row.get(0),
        )?;
        let expected_cooccur_total: i64 = self.connection.query_row(
            &format!(
                "{ACTIVE_CONNECTOME_PAIRS_CTE}
                 SELECT COALESCE(SUM(expected_count), 0) FROM active_pairs"
            ),
            [namespace],
            |row| row.get(0),
        )?;
        let actual_cooccur_total: i64 = self.connection.query_row(
            "SELECT COALESCE(SUM(cooccur_count), 0)
             FROM connectome
             WHERE namespace = ?1",
            [namespace],
            |row| row.get(0),
        )?;

        Ok(ConnectomeHealthSnapshot {
            expected_edge_count: expected_edge_count.max(0) as usize,
            actual_edge_count: actual_edge_count.max(0) as usize,
            missing_edge_count: missing_edge_count.max(0) as usize,
            stale_edge_count: stale_edge_count.max(0) as usize,
            cooccur_mismatch_count: cooccur_mismatch_count.max(0) as usize,
            weight_mismatch_count: weight_mismatch_count.max(0) as usize,
            expected_cooccur_total: expected_cooccur_total.max(0) as usize,
            actual_cooccur_total: actual_cooccur_total.max(0) as usize,
        })
    }

    pub fn connectome_requires_repair(&self, namespace: &str) -> StorageResult<bool> {
        Ok(self.load_connectome_health(namespace)?.requires_repair())
    }

    pub fn list_chunk_records_by_any_tags(
        &self,
        namespace: &str,
        tags: &[String],
    ) -> StorageResult<Vec<RecallChunkRecord>> {
        let unique_tags = tags
            .iter()
            .map(|tag| tag.trim().to_ascii_lowercase())
            .filter(|tag| !tag.is_empty())
            .collect::<BTreeSet<_>>();
        if unique_tags.is_empty() {
            return Ok(Vec::new());
        }

        let mut match_counts = BTreeMap::<i64, usize>::new();
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT c.id
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             JOIN chunk_tags ct ON ct.chunk_id = c.id
             JOIN tags t ON t.id = ct.tag_id
             WHERE c.namespace = ?1
               AND f.namespace = ?1
               AND t.namespace = ?1
               AND t.normalized_name = ?2
               AND c.is_deleted = 0
               AND c.index_status != 'deleted'
               AND f.ingest_status != 'deleted'",
        )?;

        for tag in unique_tags {
            let rows = statement.query_map(params![namespace, tag], |row| row.get::<_, i64>(0))?;
            for row in rows {
                let chunk_id = row?;
                *match_counts.entry(chunk_id).or_insert(0) += 1;
            }
        }

        let mut ranked_chunk_ids = match_counts.into_iter().collect::<Vec<_>>();
        ranked_chunk_ids
            .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

        let mut records = Vec::new();
        for (chunk_id, _) in ranked_chunk_ids {
            if let Some(record) = self.get_chunk_record(chunk_id)? {
                records.push(record);
            }
        }

        Ok(records)
    }

    pub fn rebuild_connectome(&mut self, namespace: &str) -> StorageResult<()> {
        self.with_transaction(|transaction| {
            rebuild_connectome_in_transaction(transaction, namespace)
        })
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

    pub fn write_ingest_batch(
        &mut self,
        batch: &IngestWriteBatch,
    ) -> StorageResult<PersistedIngest> {
        self.with_transaction(|transaction| write_ingest_batch(transaction, batch))
    }

    pub fn replace_ingest_batch(
        &mut self,
        namespace: &str,
        replaced_file_ids: &[i64],
        batch: &IngestWriteBatch,
    ) -> StorageResult<PersistedReplacement> {
        self.with_transaction(|transaction| {
            let deleted_chunk_ids =
                soft_delete_files_in_transaction(transaction, namespace, replaced_file_ids)?;
            let ingest = write_ingest_batch(transaction, batch)?;

            Ok(PersistedReplacement {
                ingest,
                deleted_chunk_ids,
            })
        })
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

        self.set_schema_meta_value("index_state", "degraded")?;

        Ok(self.connection.last_insert_rowid())
    }

    pub fn find_active_repair_task(
        &self,
        namespace: &str,
        task_type: &str,
        target_type: &str,
    ) -> StorageResult<Option<RepairTask>> {
        self.connection
            .query_row(
                "SELECT
                    id,
                    namespace,
                    task_type,
                    target_type,
                    target_id,
                    payload_json,
                    status,
                    retry_count,
                    last_error,
                    created_at,
                    updated_at
                 FROM repair_queue
                 WHERE namespace = ?1
                   AND task_type = ?2
                   AND target_type = ?3
                   AND status IN ('pending', 'running')
                 ORDER BY id ASC
                 LIMIT 1",
                params![namespace, task_type, target_type],
                map_repair_task,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn claim_next_repair_task(
        &mut self,
        namespace: &str,
        max_retry_count: u32,
    ) -> StorageResult<Option<RepairTask>> {
        const MAX_CLAIM_ATTEMPTS: usize = 8;

        for _ in 0..MAX_CLAIM_ATTEMPTS {
            match self.with_transaction(|transaction| {
                let candidate_id = transaction
                    .query_row(
                        "SELECT id
                         FROM repair_queue
                         WHERE namespace = ?1
                           AND status IN ('pending', 'failed')
                           AND retry_count < ?2
                         ORDER BY CASE status WHEN 'pending' THEN 0 ELSE 1 END ASC, id ASC
                         LIMIT 1",
                        params![namespace, i64::from(max_retry_count)],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?;

                let Some(task_id) = candidate_id else {
                    return Ok(ClaimRepairTaskResult::Empty);
                };

                let updated = transaction.execute(
                    "UPDATE repair_queue
                     SET status = 'running'
                     WHERE id = ?1
                       AND status IN ('pending', 'failed')",
                    [task_id],
                )?;

                if updated == 0 {
                    return Ok(ClaimRepairTaskResult::Contended);
                }

                let task = transaction
                    .query_row(
                        "SELECT
                            id,
                            namespace,
                            task_type,
                            target_type,
                            target_id,
                            payload_json,
                            status,
                            retry_count,
                            last_error,
                            created_at,
                            updated_at
                         FROM repair_queue
                         WHERE id = ?1",
                        [task_id],
                        map_repair_task,
                    )
                    .optional()?
                    .ok_or(rusqlite::Error::QueryReturnedNoRows)?;

                Ok(ClaimRepairTaskResult::Claimed(task))
            })? {
                ClaimRepairTaskResult::Claimed(task) => return Ok(Some(task)),
                ClaimRepairTaskResult::Empty => return Ok(None),
                ClaimRepairTaskResult::Contended => continue,
            }
        }

        Ok(None)
    }

    pub fn get_repair_task(&self, task_id: i64) -> StorageResult<Option<RepairTask>> {
        self.connection
            .query_row(
                "SELECT
                    id,
                    namespace,
                    task_type,
                    target_type,
                    target_id,
                    payload_json,
                    status,
                    retry_count,
                    last_error,
                    created_at,
                    updated_at
                 FROM repair_queue
                 WHERE id = ?1",
                [task_id],
                map_repair_task,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn succeed_repair_task(&mut self, task_id: i64) -> StorageResult<()> {
        self.connection.execute(
            "UPDATE repair_queue
             SET status = 'succeeded',
                 last_error = NULL
             WHERE id = ?1",
            [task_id],
        )?;

        Ok(())
    }

    pub fn fail_repair_task(
        &mut self,
        task_id: i64,
        last_error: &str,
        deadletter: bool,
    ) -> StorageResult<()> {
        let status = if deadletter { "deadletter" } else { "failed" };
        self.connection.execute(
            "UPDATE repair_queue
             SET status = ?2,
                 retry_count = retry_count + 1,
                 last_error = ?3
             WHERE id = ?1",
            params![task_id, status, last_error],
        )?;

        Ok(())
    }

    pub fn recover_running_repair_tasks(
        &mut self,
        namespace: &str,
        max_retry_count: u32,
        last_error: &str,
    ) -> StorageResult<usize> {
        let updated = self.connection.execute(
            "UPDATE repair_queue
             SET status = CASE
                    WHEN retry_count + 1 >= ?2 THEN 'deadletter'
                    ELSE 'failed'
                 END,
                 retry_count = retry_count + 1,
                 last_error = ?3
             WHERE namespace = ?1
               AND status = 'running'",
            params![namespace, i64::from(max_retry_count), last_error],
        )?;

        Ok(updated)
    }

    pub fn has_repair_backlog(&self, namespace: &str) -> StorageResult<bool> {
        let backlog_count = self.connection.query_row(
            "SELECT COUNT(*)
             FROM repair_queue
             WHERE namespace = ?1
               AND status IN ('pending', 'running', 'failed', 'deadletter')",
            [namespace],
            |row| row.get::<_, i64>(0),
        )?;

        Ok(backlog_count > 0)
    }

    pub fn has_repair_backlog_excluding(
        &self,
        namespace: &str,
        excluded_task_id: i64,
    ) -> StorageResult<bool> {
        let backlog_count = self.connection.query_row(
            "SELECT COUNT(*)
             FROM repair_queue
             WHERE namespace = ?1
               AND status IN ('pending', 'running', 'failed', 'deadletter')
               AND id != ?2",
            params![namespace, excluded_task_id],
            |row| row.get::<_, i64>(0),
        )?;

        Ok(backlog_count > 0)
    }

    pub fn soft_delete_files(
        &mut self,
        namespace: &str,
        file_ids: &[i64],
    ) -> StorageResult<PersistedDeletion> {
        self.with_transaction(|transaction| {
            Ok(PersistedDeletion {
                deleted_chunk_ids: soft_delete_files_in_transaction(
                    transaction,
                    namespace,
                    file_ids,
                )?,
            })
        })
    }

    pub fn soft_delete_chunks(
        &mut self,
        namespace: &str,
        chunk_ids: &[i64],
    ) -> StorageResult<PersistedDeletion> {
        self.with_transaction(|transaction| {
            Ok(PersistedDeletion {
                deleted_chunk_ids: soft_delete_chunks_in_transaction(
                    transaction,
                    namespace,
                    chunk_ids,
                )?,
            })
        })
    }

    pub fn create_maintenance_job(
        &mut self,
        job_type: &str,
        namespace: &str,
        payload_json: Option<&str>,
    ) -> StorageResult<MaintenanceJob> {
        self.connection.execute(
            "INSERT INTO maintenance_jobs (
                job_type,
                namespace,
                payload_json,
                status
            ) VALUES (?1, ?2, ?3, 'queued')",
            params![job_type, namespace, payload_json],
        )?;

        let job_id = self.connection.last_insert_rowid();
        self.get_maintenance_job(job_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows.into())
    }

    pub fn find_active_maintenance_job(
        &self,
        job_type: &str,
        namespace: &str,
    ) -> StorageResult<Option<MaintenanceJob>> {
        self.connection
            .query_row(
                "SELECT
                    id,
                    job_type,
                    namespace,
                    payload_json,
                    status,
                    progress,
                    result_summary,
                    error_message,
                    created_at,
                    started_at,
                    finished_at
                 FROM maintenance_jobs
                 WHERE job_type = ?1
                   AND namespace = ?2
                   AND status IN ('queued', 'running')
                 ORDER BY id DESC
                 LIMIT 1",
                params![job_type, namespace],
                map_maintenance_job,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_active_maintenance_jobs(
        &self,
        job_type: &str,
        namespace: &str,
    ) -> StorageResult<Vec<MaintenanceJob>> {
        let mut statement = self.connection.prepare(
            "SELECT
                id,
                job_type,
                namespace,
                payload_json,
                status,
                progress,
                result_summary,
                error_message,
                created_at,
                started_at,
                finished_at
             FROM maintenance_jobs
             WHERE job_type = ?1
               AND namespace = ?2
               AND status IN ('queued', 'running')
             ORDER BY id DESC",
        )?;
        let rows = statement.query_map(params![job_type, namespace], map_maintenance_job)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    pub fn cancel_active_maintenance_jobs(
        &mut self,
        job_type: &str,
        namespace: &str,
        reason: &str,
    ) -> StorageResult<usize> {
        let updated = self.connection.execute(
            "UPDATE maintenance_jobs
             SET status = 'cancelled',
                 error_message = ?3,
                 finished_at = CURRENT_TIMESTAMP
             WHERE job_type = ?1
               AND namespace = ?2
               AND status IN ('queued', 'running')",
            params![job_type, namespace, reason],
        )?;

        Ok(updated)
    }

    pub fn cancel_maintenance_job(&mut self, job_id: i64, reason: &str) -> StorageResult<()> {
        self.connection.execute(
            "UPDATE maintenance_jobs
             SET status = 'cancelled',
                 error_message = ?2,
                 finished_at = CURRENT_TIMESTAMP
             WHERE id = ?1
               AND status IN ('queued', 'running')",
            params![job_id, reason],
        )?;

        Ok(())
    }

    pub fn mark_maintenance_job_running(
        &mut self,
        job_id: i64,
        progress: Option<&str>,
    ) -> StorageResult<()> {
        self.connection.execute(
            "UPDATE maintenance_jobs
             SET status = 'running',
                 progress = ?2,
                 started_at = COALESCE(started_at, CURRENT_TIMESTAMP)
             WHERE id = ?1",
            params![job_id, progress],
        )?;

        Ok(())
    }

    pub fn finish_maintenance_job(
        &mut self,
        job_id: i64,
        status: &str,
        progress: Option<&str>,
        result_summary: Option<&str>,
        error_message: Option<&str>,
    ) -> StorageResult<()> {
        self.connection.execute(
            "UPDATE maintenance_jobs
             SET status = ?2,
                 progress = ?3,
                 result_summary = ?4,
                 error_message = ?5,
                 finished_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![job_id, status, progress, result_summary, error_message],
        )?;

        Ok(())
    }

    pub fn get_maintenance_job(&self, job_id: i64) -> StorageResult<Option<MaintenanceJob>> {
        self.connection
            .query_row(
                "SELECT
                    id,
                    job_type,
                    namespace,
                    payload_json,
                    status,
                    progress,
                    result_summary,
                    error_message,
                    created_at,
                    started_at,
                    finished_at
                 FROM maintenance_jobs
                 WHERE id = ?1",
                [job_id],
                map_maintenance_job,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_rebuild_chunks(&self, namespace: &str) -> StorageResult<Vec<RebuildChunkRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT
                c.id,
                c.file_id,
                c.namespace,
                c.chunk_text,
                c.index_status
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE c.namespace = ?1
               AND f.namespace = ?1
               AND c.is_deleted = 0
               AND c.index_status != 'deleted'
               AND f.ingest_status != 'deleted'
             ORDER BY c.id ASC",
        )?;
        let rows = statement.query_map([namespace], |row| {
            Ok(RebuildChunkRecord {
                chunk_id: row.get(0)?,
                file_id: row.get(1)?,
                namespace: row.get(2)?,
                chunk_text: row.get(3)?,
                index_status: row.get(4)?,
            })
        })?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(row?);
        }
        Ok(chunks)
    }

    pub fn list_missing_index_chunks(
        &self,
        namespace: &str,
    ) -> StorageResult<Vec<RebuildChunkRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT
                c.id,
                c.file_id,
                c.namespace,
                c.chunk_text,
                c.index_status
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE c.namespace = ?1
               AND f.namespace = ?1
               AND c.is_deleted = 0
               AND c.index_status IN ('pending', 'failed')
               AND f.ingest_status != 'deleted'
             ORDER BY c.id ASC",
        )?;
        let rows = statement.query_map([namespace], |row| {
            Ok(RebuildChunkRecord {
                chunk_id: row.get(0)?,
                file_id: row.get(1)?,
                namespace: row.get(2)?,
                chunk_text: row.get(3)?,
                index_status: row.get(4)?,
            })
        })?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(row?);
        }
        Ok(chunks)
    }

    pub fn mark_chunks_ready(&mut self, namespace: &str, chunk_ids: &[i64]) -> StorageResult<()> {
        for chunk_id in chunk_ids {
            self.connection.execute(
                "UPDATE chunks
                 SET index_status = 'ready'
                 WHERE namespace = ?1
                   AND id = ?2
                   AND is_deleted = 0
                   AND index_status != 'deleted'",
                params![namespace, chunk_id],
            )?;
        }

        Ok(())
    }

    pub fn refresh_file_statuses(&mut self, namespace: &str) -> StorageResult<()> {
        self.connection.execute(
            "UPDATE files
             SET ingest_status = CASE
                 WHEN EXISTS (
                     SELECT 1
                     FROM chunks c
                     WHERE c.file_id = files.id
                       AND c.is_deleted = 0
                       AND c.index_status != 'ready'
                 ) THEN 'partial'
                 ELSE 'ready'
             END
             WHERE namespace = ?1
               AND ingest_status != 'deleted'",
            [namespace],
        )?;

        Ok(())
    }

    pub fn set_schema_meta_value(&mut self, key: &str, value: &str) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO schema_meta(key, value)
             VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;

        Ok(())
    }

    pub fn get_schema_meta_value(&self, key: &str) -> StorageResult<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = ?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn load_stats(&self, namespace: &str) -> StorageResult<StorageStatsSnapshot> {
        let chunk_count = self.connection.query_row(
            "SELECT COUNT(*)
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE c.namespace = ?1
               AND f.namespace = ?1
               AND c.is_deleted = 0
               AND c.index_status != 'deleted'
               AND f.ingest_status != 'deleted'",
            [namespace],
            |row| row.get::<_, i64>(0),
        )?;
        let tag_count = self.connection.query_row(
            "SELECT COUNT(*)
             FROM tags
             WHERE namespace = ?1",
            [namespace],
            |row| row.get::<_, i64>(0),
        )?;
        let active_tag_count = self.connection.query_row(
            "SELECT COUNT(DISTINCT t.id)
             FROM tags t
             JOIN chunk_tags ct ON ct.tag_id = t.id
             JOIN chunks c ON c.id = ct.chunk_id
             JOIN files f ON f.id = c.file_id
             WHERE t.namespace = ?1
               AND c.namespace = ?1
               AND f.namespace = ?1
               AND c.is_deleted = 0
               AND c.index_status != 'deleted'
               AND f.ingest_status != 'deleted'",
            [namespace],
            |row| row.get::<_, i64>(0),
        )?;
        let deleted_chunk_count = self.connection.query_row(
            "SELECT COUNT(*)
             FROM chunks
             WHERE namespace = ?1
               AND (is_deleted = 1 OR index_status = 'deleted')",
            [namespace],
            |row| row.get::<_, i64>(0),
        )?;
        let connectome_edge_count = self.connection.query_row(
            "SELECT COUNT(*)
             FROM connectome
             WHERE namespace = ?1",
            [namespace],
            |row| row.get::<_, i64>(0),
        )?;
        let repair_queue_size = self.connection.query_row(
            "SELECT COUNT(*)
             FROM repair_queue
             WHERE namespace = ?1
               AND status IN ('pending', 'running', 'failed', 'deadletter')",
            [namespace],
            |row| row.get::<_, i64>(0),
        )?;
        let index_status = self
            .get_schema_meta_value("index_state")?
            .unwrap_or_else(|| "ready".to_owned());

        Ok(StorageStatsSnapshot {
            chunk_count: chunk_count.max(0) as usize,
            tag_count: tag_count.max(0) as usize,
            active_tag_count: active_tag_count.max(0) as usize,
            deleted_chunk_count: deleted_chunk_count.max(0) as usize,
            connectome_edge_count: connectome_edge_count.max(0) as usize,
            repair_queue_size: repair_queue_size.max(0) as usize,
            index_status,
        })
    }

    pub fn load_repair_queue_status_counts(
        &self,
        namespace: &str,
    ) -> StorageResult<Vec<crate::storage::models::StatusCount>> {
        let mut statement = self.connection.prepare(
            "SELECT status, COUNT(*)
             FROM repair_queue
             WHERE namespace = ?1
             GROUP BY status
             ORDER BY status ASC",
        )?;
        let rows = statement.query_map([namespace], |row| {
            Ok(crate::storage::models::StatusCount {
                status: row.get(0)?,
                count: row.get::<_, i64>(1)?.max(0) as usize,
            })
        })?;

        let mut counts = Vec::new();
        for row in rows {
            counts.push(row?);
        }

        Ok(counts)
    }

    pub fn load_maintenance_job_status_counts(
        &self,
        job_type: &str,
        namespace: &str,
    ) -> StorageResult<Vec<crate::storage::models::StatusCount>> {
        let mut statement = self.connection.prepare(
            "SELECT status, COUNT(*)
             FROM maintenance_jobs
             WHERE job_type = ?1
               AND namespace = ?2
             GROUP BY status
             ORDER BY status ASC",
        )?;
        let rows = statement.query_map(params![job_type, namespace], |row| {
            Ok(crate::storage::models::StatusCount {
                status: row.get(0)?,
                count: row.get::<_, i64>(1)?.max(0) as usize,
            })
        })?;

        let mut counts = Vec::new();
        for row in rows {
            counts.push(row?);
        }

        Ok(counts)
    }

    pub fn load_oldest_repair_task_age_seconds(
        &self,
        namespace: &str,
    ) -> StorageResult<Option<f64>> {
        let age_seconds = self.connection.query_row(
            "SELECT MAX((julianday('now') - julianday(created_at)) * 86400.0)
             FROM repair_queue
             WHERE namespace = ?1
               AND status IN ('pending', 'running', 'failed', 'deadletter')",
            [namespace],
            |row| row.get::<_, Option<f64>>(0),
        )?;

        Ok(age_seconds.filter(|value| value.is_finite() && *value >= 0.0))
    }

    pub fn load_oldest_active_maintenance_job_age_seconds(
        &self,
        job_type: &str,
        namespace: &str,
    ) -> StorageResult<Option<f64>> {
        let age_seconds = self.connection.query_row(
            "SELECT MAX((julianday('now') - julianday(COALESCE(started_at, created_at))) * 86400.0)
             FROM maintenance_jobs
             WHERE job_type = ?1
               AND namespace = ?2
               AND status IN ('queued', 'running')",
            params![job_type, namespace],
            |row| row.get::<_, Option<f64>>(0),
        )?;

        Ok(age_seconds.filter(|value| value.is_finite() && *value >= 0.0))
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

fn map_maintenance_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<MaintenanceJob> {
    Ok(MaintenanceJob {
        id: row.get(0)?,
        job_type: row.get(1)?,
        namespace: row.get(2)?,
        payload_json: row.get(3)?,
        status: row.get(4)?,
        progress: row.get(5)?,
        result_summary: row.get(6)?,
        error_message: row.get(7)?,
        created_at: row.get(8)?,
        started_at: row.get(9)?,
        finished_at: row.get(10)?,
    })
}

fn map_repair_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepairTask> {
    Ok(RepairTask {
        id: row.get(0)?,
        namespace: row.get(1)?,
        task_type: row.get(2)?,
        target_type: row.get(3)?,
        target_id: row.get(4)?,
        payload_json: row.get(5)?,
        status: row.get(6)?,
        retry_count: row.get(7)?,
        last_error: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn map_cached_embedding(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedEmbedding> {
    let vector_blob: Vec<u8> = row.get(5)?;
    let vector = deserialize_embedding(&vector_blob).map_err(|message| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Blob,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )),
        )
    })?;

    Ok(CachedEmbedding {
        id: row.get(0)?,
        namespace: row.get(1)?,
        content_hash: row.get(2)?,
        model_id: row.get(3)?,
        dimension: row.get(4)?,
        vector,
        created_at: row.get(6)?,
    })
}

fn map_retrieval_log_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<RetrievalLogRecord> {
    Ok(RetrievalLogRecord {
        id: row.get(0)?,
        namespace: row.get(1)?,
        query_text: row.get(2)?,
        query_hash: row.get(3)?,
        mode: row.get(4)?,
        worldview: row.get(5)?,
        entropy: row.get(6)?,
        result_count: row.get(7)?,
        total_time_us: row.get(8)?,
        spike_depth: row.get(9)?,
        emergent_count: row.get(10)?,
        params_snapshot: row.get(11)?,
        created_at: row.get(12)?,
    })
}

fn map_connectome_edge_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectomeEdgeRecord> {
    Ok(ConnectomeEdgeRecord {
        namespace: row.get(0)?,
        source_tag: row.get(1)?,
        target_tag: row.get(2)?,
        weight: row.get(3)?,
        cooccur_count: row.get(4)?,
        last_updated: row.get(5)?,
    })
}

fn serialize_embedding(vector: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(vector.len() * std::mem::size_of::<f32>());
    for value in vector {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

fn deserialize_embedding(blob: &[u8]) -> Result<Vec<f32>, String> {
    if blob.len() % std::mem::size_of::<f32>() != 0 {
        return Err(format!(
            "embedding blob size {} is not divisible by {}",
            blob.len(),
            std::mem::size_of::<f32>()
        ));
    }

    let mut vector = Vec::with_capacity(blob.len() / std::mem::size_of::<f32>());
    for chunk in blob.chunks_exact(std::mem::size_of::<f32>()) {
        let mut bytes = [0_u8; std::mem::size_of::<f32>()];
        bytes.copy_from_slice(chunk);
        vector.push(f32::from_le_bytes(bytes));
    }
    Ok(vector)
}

enum ClaimRepairTaskResult {
    Claimed(RepairTask),
    Empty,
    Contended,
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

    let mut chunk_tag_pairs = BTreeMap::<i64, Vec<i64>>::new();
    for link in &batch.chunk_tags {
        let chunk_id = chunk_ids.get(&link.chunk_index).copied().ok_or_else(|| {
            StorageError::InvalidBatchReference {
                message: format!("chunk_index={} not found in batch", link.chunk_index),
            }
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
        chunk_tag_pairs
            .entry(link.chunk_index)
            .or_default()
            .push(tag_id);
    }

    for tag_ids in chunk_tag_pairs.values() {
        update_connectome_for_chunk_tags(transaction, &batch.file.namespace, tag_ids)?;
    }

    Ok(PersistedIngest {
        file,
        chunks,
        tag_ids,
    })
}

fn soft_delete_files_in_transaction(
    transaction: &Transaction<'_>,
    namespace: &str,
    file_ids: &[i64],
) -> StorageResult<Vec<i64>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut deleted_chunk_ids = Vec::new();
    let unique_file_ids = file_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut select_chunks = transaction.prepare(
        "SELECT c.id
         FROM chunks c
         JOIN files f ON f.id = c.file_id
         WHERE f.namespace = ?1
           AND f.id = ?2
           AND f.ingest_status != 'deleted'
           AND c.is_deleted = 0
         ORDER BY c.chunk_index ASC",
    )?;

    for file_id in &unique_file_ids {
        let rows =
            select_chunks.query_map(params![namespace, file_id], |row| row.get::<_, i64>(0))?;
        for row in rows {
            deleted_chunk_ids.push(row?);
        }
    }

    for chunk_id in &deleted_chunk_ids {
        transaction.execute(
            "UPDATE chunks
             SET is_deleted = 1,
                 deleted_at = CURRENT_TIMESTAMP,
                 index_status = 'deleted'
             WHERE id = ?1",
            [chunk_id],
        )?;
    }

    for file_id in &unique_file_ids {
        transaction.execute(
            "UPDATE files
             SET ingest_status = 'deleted'
             WHERE namespace = ?1 AND id = ?2",
            params![namespace, file_id],
        )?;
    }

    Ok(deleted_chunk_ids)
}

fn soft_delete_chunks_in_transaction(
    transaction: &Transaction<'_>,
    namespace: &str,
    chunk_ids: &[i64],
) -> StorageResult<Vec<i64>> {
    if chunk_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut affected_chunk_ids = Vec::new();
    let mut touched_file_ids = BTreeMap::<i64, ()>::new();
    let unique_chunk_ids = chunk_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut select_chunk = transaction.prepare(
        "SELECT c.id, c.file_id
         FROM chunks c
         JOIN files f ON f.id = c.file_id
         WHERE c.id = ?1
           AND c.namespace = ?2
           AND f.namespace = ?2
           AND f.ingest_status != 'deleted'
           AND c.is_deleted = 0",
    )?;

    for chunk_id in &unique_chunk_ids {
        let record = select_chunk
            .query_row(params![chunk_id, namespace], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()?;

        if let Some((chunk_id, file_id)) = record {
            affected_chunk_ids.push(chunk_id);
            touched_file_ids.insert(file_id, ());
        }
    }

    for chunk_id in &affected_chunk_ids {
        transaction.execute(
            "UPDATE chunks
             SET is_deleted = 1,
                 deleted_at = CURRENT_TIMESTAMP,
                 index_status = 'deleted'
             WHERE id = ?1",
            [chunk_id],
        )?;
    }

    for file_id in touched_file_ids.keys() {
        let remaining_active_chunks: i64 = transaction.query_row(
            "SELECT COUNT(*)
             FROM chunks
             WHERE file_id = ?1
               AND is_deleted = 0
               AND index_status != 'deleted'",
            [file_id],
            |row| row.get(0),
        )?;

        if remaining_active_chunks == 0 {
            transaction.execute(
                "UPDATE files
                 SET ingest_status = 'deleted'
                 WHERE namespace = ?1 AND id = ?2",
                params![namespace, file_id],
            )?;
        }
    }

    Ok(affected_chunk_ids)
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

fn update_connectome_for_chunk_tags(
    transaction: &Transaction<'_>,
    namespace: &str,
    tag_ids: &[i64],
) -> StorageResult<()> {
    let unique_tag_ids = tag_ids.iter().copied().collect::<BTreeSet<_>>();
    let ordered_tag_ids = unique_tag_ids.into_iter().collect::<Vec<_>>();

    for left_index in 0..ordered_tag_ids.len() {
        for right_index in (left_index + 1)..ordered_tag_ids.len() {
            let tag_i = ordered_tag_ids[left_index];
            let tag_j = ordered_tag_ids[right_index];
            upsert_connectome_edge(transaction, namespace, tag_i, tag_j)?;
        }
    }

    Ok(())
}

fn upsert_connectome_edge(
    transaction: &Transaction<'_>,
    namespace: &str,
    tag_i: i64,
    tag_j: i64,
) -> StorageResult<()> {
    let (tag_i, tag_j) = if tag_i <= tag_j {
        (tag_i, tag_j)
    } else {
        (tag_j, tag_i)
    };

    transaction.execute(
        "INSERT INTO connectome (
            namespace,
            tag_i,
            tag_j,
            weight,
            cooccur_count
        ) VALUES (?1, ?2, ?3, 1.0, 1)
        ON CONFLICT(namespace, tag_i, tag_j)
        DO UPDATE SET
            cooccur_count = connectome.cooccur_count + 1,
            weight = CAST(connectome.cooccur_count + 1 AS REAL),
            last_updated = CURRENT_TIMESTAMP",
        params![namespace, tag_i, tag_j],
    )?;

    Ok(())
}

fn rebuild_connectome_in_transaction(
    transaction: &Transaction<'_>,
    namespace: &str,
) -> StorageResult<()> {
    transaction.execute("DELETE FROM connectome WHERE namespace = ?1", [namespace])?;

    let mut statement = transaction.prepare(
        "SELECT
            ct.chunk_id,
            ct.tag_id
         FROM chunk_tags ct
         JOIN chunks c ON c.id = ct.chunk_id
         JOIN files f ON f.id = c.file_id
         JOIN tags t ON t.id = ct.tag_id
         WHERE c.namespace = ?1
           AND f.namespace = ?1
           AND t.namespace = ?1
           AND c.is_deleted = 0
           AND c.index_status != 'deleted'
           AND f.ingest_status != 'deleted'
         ORDER BY ct.chunk_id ASC, ct.tag_id ASC",
    )?;
    let rows = statement.query_map([namespace], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;

    let mut tag_ids_by_chunk = BTreeMap::<i64, Vec<i64>>::new();
    for row in rows {
        let (chunk_id, tag_id) = row?;
        tag_ids_by_chunk.entry(chunk_id).or_default().push(tag_id);
    }

    for tag_ids in tag_ids_by_chunk.into_values() {
        update_connectome_for_chunk_tags(transaction, namespace, &tag_ids)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::SqliteRepository;
    use crate::storage::models::{
        ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, RetrievalLogWrite, TagWrite,
    };
    use crate::storage::{
        apply_sqlite_migrations, SchemaExpectation, StorageError, LATEST_SCHEMA_VERSION,
    };

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        apply_sqlite_migrations(&connection).expect("apply migration");
        SqliteRepository::new(connection).expect("repository")
    }

    #[test]
    fn validates_schema_through_repository() {
        let repository = repository_with_schema();
        let snapshot = repository
            .validate_schema(&SchemaExpectation::new(
                LATEST_SCHEMA_VERSION,
                "1",
                "unknown",
                0,
            ))
            .expect("schema valid");

        assert_eq!(snapshot.schema_version, LATEST_SCHEMA_VERSION);
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
            chunks: vec![ChunkWrite::new(
                0,
                "alpha",
                "chunk-hash-rollback",
                "test-model",
                3,
            )],
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
            chunks: vec![ChunkWrite::new(
                0,
                "alpha",
                "chunk-find-me",
                "test-model",
                3,
            )],
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
        let chunk_ids = persisted
            .chunks
            .iter()
            .map(|chunk| chunk.id)
            .collect::<Vec<_>>();

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
            tags: vec![
                TagWrite::new("Project", "project"),
                TagWrite::new("Rust", "rust"),
            ],
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

    #[test]
    fn soft_delete_hides_files_from_active_hash_lookup() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("doc.txt", "hash-soft-delete"),
            chunks: vec![ChunkWrite::new(
                0,
                "alpha",
                "chunk-soft-delete",
                "test-model",
                3,
            )],
            tags: vec![],
            chunk_tags: vec![],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");

        let deleted = repository
            .soft_delete_files("default", &[persisted.file.id])
            .expect("soft delete file");
        let active_files = repository
            .list_active_files_by_hash("default", "hash-soft-delete")
            .expect("list active files");
        let chunk = repository
            .get_chunk_record(persisted.chunks[0].id)
            .expect("load chunk record after delete");

        assert_eq!(deleted.deleted_chunk_ids, vec![persisted.chunks[0].id]);
        assert!(active_files.is_empty());
        assert!(chunk.is_none());
    }

    #[test]
    fn soft_delete_chunks_keeps_file_until_last_chunk_is_deleted() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("doc.txt", "hash-partial-delete"),
            chunks: vec![
                ChunkWrite::new(0, "alpha", "chunk-delete-1", "test-model", 3),
                ChunkWrite::new(1, "beta", "chunk-delete-2", "test-model", 3),
            ],
            tags: vec![],
            chunk_tags: vec![],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");

        let partial = repository
            .soft_delete_chunks("default", &[persisted.chunks[0].id])
            .expect("soft delete one chunk");
        let file_after_partial = repository
            .find_file_by_hash("default", "hash-partial-delete")
            .expect("find file after partial")
            .expect("file should remain active");
        let final_delete = repository
            .soft_delete_chunks("default", &[persisted.chunks[1].id])
            .expect("soft delete final chunk");
        let file_after_final = repository
            .find_file_by_hash("default", "hash-partial-delete")
            .expect("find file after final delete");

        assert_eq!(partial.deleted_chunk_ids, vec![persisted.chunks[0].id]);
        assert_eq!(file_after_partial.id, persisted.file.id);
        assert_eq!(final_delete.deleted_chunk_ids, vec![persisted.chunks[1].id]);
        assert!(file_after_final.is_none());
    }

    #[test]
    fn persists_and_transitions_maintenance_jobs() {
        let mut repository = repository_with_schema();

        let job = repository
            .create_maintenance_job("rebuild", "default", Some("{\"scope\":\"all\"}"))
            .expect("create maintenance job");
        let newer_job = repository
            .create_maintenance_job("rebuild", "default", Some("{\"scope\":\"missing_index\"}"))
            .expect("create newer maintenance job");
        assert_eq!(job.status, "queued");
        assert_eq!(newer_job.status, "queued");

        let active = repository
            .find_active_maintenance_job("rebuild", "default")
            .expect("find active maintenance job")
            .expect("active job exists");
        assert_eq!(active.id, newer_job.id);

        let active_jobs = repository
            .list_active_maintenance_jobs("rebuild", "default")
            .expect("list active maintenance jobs");
        assert_eq!(active_jobs.len(), 2);
        assert_eq!(active_jobs[0].id, newer_job.id);
        assert_eq!(active_jobs[1].id, job.id);

        repository
            .cancel_maintenance_job(job.id, "superseded")
            .expect("cancel older job");

        repository
            .mark_maintenance_job_running(newer_job.id, Some("running"))
            .expect("mark running");
        repository
            .finish_maintenance_job(newer_job.id, "succeeded", Some("1/1"), Some("done"), None)
            .expect("finish job");

        let cancelled = repository
            .get_maintenance_job(job.id)
            .expect("load cancelled maintenance job")
            .expect("cancelled job should exist");
        assert_eq!(cancelled.status, "cancelled");
        assert_eq!(cancelled.error_message.as_deref(), Some("superseded"));

        let persisted = repository
            .get_maintenance_job(newer_job.id)
            .expect("load maintenance job")
            .expect("job should exist");
        assert_eq!(persisted.status, "succeeded");
        assert_eq!(persisted.progress.as_deref(), Some("1/1"));
        assert_eq!(persisted.result_summary.as_deref(), Some("done"));
    }

    #[test]
    fn lists_rebuild_targets_and_refreshes_file_status() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("rebuild.txt", "hash-rebuild"),
            chunks: vec![ChunkWrite::new(
                0,
                "alpha",
                "chunk-rebuild",
                "test-model",
                3,
            )],
            tags: vec![],
            chunk_tags: vec![],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");
        repository
            .update_indexing_result(
                persisted.file.id,
                &[persisted.chunks[0].id],
                "partial",
                "failed",
            )
            .expect("mark chunk failed");

        let all_chunks = repository
            .list_rebuild_chunks("default")
            .expect("list rebuild chunks");
        let missing_chunks = repository
            .list_missing_index_chunks("default")
            .expect("list missing index chunks");

        assert_eq!(all_chunks.len(), 1);
        assert_eq!(missing_chunks.len(), 1);
        assert_eq!(missing_chunks[0].chunk_id, persisted.chunks[0].id);

        repository
            .mark_chunks_ready("default", &[persisted.chunks[0].id])
            .expect("mark chunk ready");
        repository
            .refresh_file_statuses("default")
            .expect("refresh file statuses");
        repository
            .set_schema_meta_value("index_state", "ready")
            .expect("set index_state");

        let file = repository
            .find_file_by_hash("default", "hash-rebuild")
            .expect("find file")
            .expect("file exists");
        let index_state = repository
            .get_schema_meta_value("index_state")
            .expect("get index_state");

        assert_eq!(file.ingest_status, "ready");
        assert_eq!(index_state.as_deref(), Some("ready"));
    }

    #[test]
    fn loads_stats_for_active_deleted_and_pending_repairs() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("stats.txt", "hash-stats"),
            chunks: vec![
                ChunkWrite::new(0, "alpha", "chunk-stats-1", "test-model", 3),
                ChunkWrite::new(1, "beta", "chunk-stats-2", "test-model", 3),
            ],
            tags: vec![TagWrite::new("Project", "project")],
            chunk_tags: vec![
                ChunkTagInsert::new(0, "project"),
                ChunkTagInsert::new(1, "project"),
            ],
        };

        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");
        let chunk_ids = persisted
            .chunks
            .iter()
            .map(|chunk| chunk.id)
            .collect::<Vec<_>>();
        repository
            .update_indexing_result(persisted.file.id, &chunk_ids, "ready", "ready")
            .expect("mark chunks ready");
        repository
            .soft_delete_chunks("default", &[persisted.chunks[1].id])
            .expect("soft delete second chunk");
        repository
            .enqueue_repair_task(
                "default",
                "index_insert",
                "chunk",
                Some(persisted.chunks[0].id),
                None,
            )
            .expect("enqueue repair");
        repository
            .set_schema_meta_value("index_state", "degraded")
            .expect("set index_state");

        let stats = repository.load_stats("default").expect("load stats");

        assert_eq!(stats.chunk_count, 1);
        assert_eq!(stats.tag_count, 1);
        assert_eq!(stats.active_tag_count, 1);
        assert_eq!(stats.deleted_chunk_count, 1);
        assert_eq!(stats.connectome_edge_count, 0);
        assert_eq!(stats.repair_queue_size, 1);
        assert_eq!(stats.index_status, "degraded");
    }

    #[test]
    fn loads_oldest_repair_task_and_active_job_ages() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("ops.txt", "ops-hash"),
            chunks: vec![ChunkWrite::new(
                0,
                "ops visibility content",
                "ops-chunk-hash",
                "test-model",
                3,
            )],
            tags: vec![],
            chunk_tags: vec![],
        };
        let persisted = repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");

        repository
            .enqueue_repair_task(
                "default",
                "index_insert",
                "chunk",
                Some(persisted.chunks[0].id),
                None,
            )
            .expect("enqueue repair task");
        repository
            .create_maintenance_job("rebuild", "default", Some("{\"scope\":\"all\"}"))
            .expect("create maintenance job");

        repository
            .connection
            .execute(
                "UPDATE repair_queue
                 SET created_at = datetime('now', '-120 seconds')
                 WHERE namespace = 'default'",
                [],
            )
            .expect("age repair queue task");
        repository
            .connection
            .execute(
                "UPDATE maintenance_jobs
                 SET created_at = datetime('now', '-180 seconds')
                 WHERE job_type = 'rebuild'
                   AND namespace = 'default'",
                [],
            )
            .expect("age maintenance job");

        let repair_age = repository
            .load_oldest_repair_task_age_seconds("default")
            .expect("load oldest repair task age")
            .expect("repair age should exist");
        let rebuild_age = repository
            .load_oldest_active_maintenance_job_age_seconds("rebuild", "default")
            .expect("load oldest rebuild age")
            .expect("rebuild age should exist");

        assert!(
            repair_age >= 120.0,
            "expected repair age >= 120s, got {repair_age}"
        );
        assert!(
            rebuild_age >= 180.0,
            "expected rebuild age >= 180s, got {rebuild_age}"
        );
    }

    #[test]
    fn persists_and_reads_embedding_cache_entries() {
        let mut repository = repository_with_schema();
        let embedding = vec![0.125, -0.5, 0.875];

        repository
            .put_cached_embedding("default", "chunk-cache-hash", "test-model", &embedding)
            .expect("cache embedding");

        let cached = repository
            .get_cached_embedding("default", "chunk-cache-hash", "test-model")
            .expect("load cached embedding")
            .expect("cached embedding should exist");

        assert_eq!(cached.namespace, "default");
        assert_eq!(cached.content_hash, "chunk-cache-hash");
        assert_eq!(cached.model_id, "test-model");
        assert_eq!(cached.dimension, 3);
        assert_eq!(cached.vector, embedding);
    }

    #[test]
    fn persists_retrieval_log_entries_in_time_order() {
        let mut repository = repository_with_schema();

        repository
            .append_retrieval_log(
                "default",
                &RetrievalLogWrite::new("query alpha", "hash-alpha", "basic")
                    .with_result_count(2)
                    .with_total_time_us(1_200)
                    .with_params_snapshot("{\"top_k\":5}"),
            )
            .expect("append first retrieval log");
        repository
            .append_retrieval_log(
                "default",
                &RetrievalLogWrite::new("query beta", "hash-beta", "basic")
                    .with_result_count(0)
                    .with_total_time_us(900)
                    .with_params_snapshot("{\"top_k\":3}"),
            )
            .expect("append second retrieval log");

        let logs = repository
            .list_retrieval_logs("default", 10)
            .expect("list retrieval logs");

        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].query_hash, "hash-beta");
        assert_eq!(logs[0].result_count, 0);
        assert_eq!(logs[0].total_time_us, Some(900));
        assert_eq!(logs[1].query_hash, "hash-alpha");
        assert_eq!(logs[1].result_count, 2);
        assert_eq!(logs[1].params_snapshot.as_deref(), Some("{\"top_k\":5}"));
    }

    #[test]
    fn writes_connectome_edges_from_chunk_tag_cooccurrence() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("connectome.txt", "hash-connectome"),
            chunks: vec![ChunkWrite::new(
                0,
                "project rust memory",
                "chunk-connectome",
                "test-model",
                3,
            )],
            tags: vec![
                TagWrite::new("Project", "project"),
                TagWrite::new("Rust", "rust"),
                TagWrite::new("Memory", "memory"),
            ],
            chunk_tags: vec![
                ChunkTagInsert::new(0, "project"),
                ChunkTagInsert::new(0, "rust"),
                ChunkTagInsert::new(0, "memory"),
            ],
        };

        repository
            .write_ingest_batch(&batch)
            .expect("write connectome batch");

        let neighbors = repository
            .list_connectome_neighbors("default", "project", 10)
            .expect("list connectome neighbors");

        assert_eq!(neighbors.len(), 2);
        assert_eq!(neighbors[0].source_tag, "project");
        assert_eq!(neighbors[0].cooccur_count, 1);
        assert!(neighbors.iter().any(|edge| edge.target_tag == "memory"));
        assert!(neighbors.iter().any(|edge| edge.target_tag == "rust"));
    }

    #[test]
    fn increments_connectome_cooccurrence_across_batches() {
        let mut repository = repository_with_schema();
        let first_batch = IngestWriteBatch {
            file: FileWrite::new("first.txt", "hash-connectome-first"),
            chunks: vec![ChunkWrite::new(
                0,
                "project rust",
                "chunk-connectome-first",
                "test-model",
                3,
            )],
            tags: vec![
                TagWrite::new("Project", "project"),
                TagWrite::new("Rust", "rust"),
            ],
            chunk_tags: vec![
                ChunkTagInsert::new(0, "project"),
                ChunkTagInsert::new(0, "rust"),
            ],
        };
        let second_batch = IngestWriteBatch {
            file: FileWrite::new("second.txt", "hash-connectome-second"),
            chunks: vec![ChunkWrite::new(
                0,
                "project rust again",
                "chunk-connectome-second",
                "test-model",
                3,
            )],
            tags: vec![
                TagWrite::new("Project", "project"),
                TagWrite::new("Rust", "rust"),
            ],
            chunk_tags: vec![
                ChunkTagInsert::new(0, "project"),
                ChunkTagInsert::new(0, "rust"),
            ],
        };

        repository
            .write_ingest_batch(&first_batch)
            .expect("write first connectome batch");
        repository
            .write_ingest_batch(&second_batch)
            .expect("write second connectome batch");

        let neighbors = repository
            .list_connectome_neighbors("default", "project", 10)
            .expect("list connectome neighbors");

        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].target_tag, "rust");
        assert_eq!(neighbors[0].cooccur_count, 2);
        assert_eq!(neighbors[0].weight, 2.0);
    }

    #[test]
    fn rebuild_connectome_drops_edges_from_deleted_files() {
        let mut repository = repository_with_schema();
        let first = repository
            .write_ingest_batch(&IngestWriteBatch {
                file: FileWrite::new("first.txt", "hash-connectome-live-first"),
                chunks: vec![ChunkWrite::new(
                    0,
                    "project rust live",
                    "chunk-connectome-live-first",
                    "test-model",
                    3,
                )],
                tags: vec![
                    TagWrite::new("Project", "project"),
                    TagWrite::new("Rust", "rust"),
                ],
                chunk_tags: vec![
                    ChunkTagInsert::new(0, "project"),
                    ChunkTagInsert::new(0, "rust"),
                ],
            })
            .expect("write first connectome batch");
        repository
            .write_ingest_batch(&IngestWriteBatch {
                file: FileWrite::new("second.txt", "hash-connectome-live-second"),
                chunks: vec![ChunkWrite::new(
                    0,
                    "project memory live",
                    "chunk-connectome-live-second",
                    "test-model",
                    3,
                )],
                tags: vec![
                    TagWrite::new("Project", "project"),
                    TagWrite::new("Memory", "memory"),
                ],
                chunk_tags: vec![
                    ChunkTagInsert::new(0, "project"),
                    ChunkTagInsert::new(0, "memory"),
                ],
            })
            .expect("write second connectome batch");

        repository
            .soft_delete_files("default", &[first.file.id])
            .expect("soft delete first file");

        let stale_neighbors = repository
            .list_connectome_neighbors("default", "project", 10)
            .expect("list stale neighbors");
        assert!(
            stale_neighbors.iter().any(|edge| edge.target_tag == "rust"),
            "soft delete should leave stale connectome edge before rebuild"
        );

        repository
            .rebuild_connectome("default")
            .expect("rebuild connectome");

        let rebuilt_neighbors = repository
            .list_connectome_neighbors("default", "project", 10)
            .expect("list rebuilt neighbors");
        assert!(
            rebuilt_neighbors
                .iter()
                .all(|edge| edge.target_tag != "rust"),
            "deleted file edge should be removed after connectome rebuild"
        );
        assert!(
            rebuilt_neighbors
                .iter()
                .any(|edge| edge.target_tag == "memory"),
            "active file edge should remain after connectome rebuild"
        );
    }

    #[test]
    fn detects_when_connectome_requires_repair() {
        let mut repository = repository_with_schema();
        repository
            .write_ingest_batch(&IngestWriteBatch {
                file: FileWrite::new("connectome.txt", "hash-connectome-repair-needed"),
                chunks: vec![ChunkWrite::new(
                    0,
                    "project rust repair seed",
                    "chunk-hash-connectome-repair-needed",
                    "test-model",
                    4,
                )],
                tags: vec![
                    TagWrite::new("Project", "project"),
                    TagWrite::new("Rust", "rust"),
                ],
                chunk_tags: vec![
                    ChunkTagInsert::new(0, "project"),
                    ChunkTagInsert::new(0, "rust"),
                ],
            })
            .expect("write connectome repair batch");
        let healthy = repository
            .load_connectome_health("default")
            .expect("load healthy connectome snapshot");
        assert_eq!(healthy.expected_edge_count, 1);
        assert_eq!(healthy.actual_edge_count, 1);
        assert!(!healthy.requires_repair());

        repository
            .connection
            .execute("DELETE FROM connectome WHERE namespace = 'default'", [])
            .expect("delete connectome rows");

        let degraded = repository
            .load_connectome_health("default")
            .expect("load degraded connectome snapshot");
        assert_eq!(degraded.expected_edge_count, 1);
        assert_eq!(degraded.actual_edge_count, 0);
        assert_eq!(degraded.missing_edge_count, 1);
        assert!(degraded.requires_repair());
    }

    #[test]
    fn detects_connectome_cooccur_drift_as_repair_needed() {
        let mut repository = repository_with_schema();
        repository
            .write_ingest_batch(&IngestWriteBatch {
                file: FileWrite::new("connectome-drift.txt", "hash-connectome-drift"),
                chunks: vec![
                    ChunkWrite::new(0, "project rust first", "chunk-drift-1", "test-model", 4),
                    ChunkWrite::new(1, "project rust second", "chunk-drift-2", "test-model", 4),
                ],
                tags: vec![
                    TagWrite::new("Project", "project"),
                    TagWrite::new("Rust", "rust"),
                ],
                chunk_tags: vec![
                    ChunkTagInsert::new(0, "project"),
                    ChunkTagInsert::new(0, "rust"),
                    ChunkTagInsert::new(1, "project"),
                    ChunkTagInsert::new(1, "rust"),
                ],
            })
            .expect("write connectome drift batch");

        repository
            .connection
            .execute(
                "UPDATE connectome
                 SET cooccur_count = 1,
                     weight = 1.0
                 WHERE namespace = 'default'",
                [],
            )
            .expect("mutate connectome cooccur count");

        let snapshot = repository
            .load_connectome_health("default")
            .expect("load drifted connectome snapshot");

        assert_eq!(snapshot.expected_edge_count, 1);
        assert_eq!(snapshot.actual_edge_count, 1);
        assert_eq!(snapshot.expected_cooccur_total, 2);
        assert_eq!(snapshot.actual_cooccur_total, 1);
        assert_eq!(snapshot.cooccur_mismatch_count, 1);
        assert_eq!(snapshot.weight_mismatch_count, 1);
        assert!(snapshot.requires_repair());
    }

    #[test]
    fn finds_active_repair_task_by_type() {
        let mut repository = repository_with_schema();
        repository
            .enqueue_repair_task(
                "default",
                "connectome_rebuild",
                "namespace",
                None,
                Some("{\"deleted_chunk_ids\":[1],\"reason\":\"forget_soft_delete\"}"),
            )
            .expect("enqueue active repair task");
        let completed_id = repository
            .enqueue_repair_task(
                "default",
                "connectome_rebuild",
                "namespace",
                None,
                Some("{\"deleted_chunk_ids\":[2],\"reason\":\"forget_soft_delete\"}"),
            )
            .expect("enqueue second repair task");
        repository
            .succeed_repair_task(completed_id)
            .expect("mark second repair task succeeded");

        let active = repository
            .find_active_repair_task("default", "connectome_rebuild", "namespace")
            .expect("find active repair task")
            .expect("active repair task should exist");

        assert_eq!(active.task_type, "connectome_rebuild");
        assert_eq!(active.target_type, "namespace");
        assert_eq!(active.status, "pending");
    }
}
