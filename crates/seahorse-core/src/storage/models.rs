use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWrite {
    pub namespace: String,
    pub filename: String,
    pub source_type: Option<String>,
    pub source_uri: Option<String>,
    pub file_hash: String,
    pub metadata_json: Option<String>,
    pub ingest_status: String,
}

impl FileWrite {
    pub fn new(filename: impl Into<String>, file_hash: impl Into<String>) -> Self {
        Self {
            namespace: "default".to_owned(),
            filename: filename.into(),
            source_type: None,
            source_uri: None,
            file_hash: file_hash.into(),
            metadata_json: None,
            ingest_status: "pending_index".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkWrite {
    pub namespace: String,
    pub chunk_index: i64,
    pub chunk_text: String,
    pub content_hash: String,
    pub token_count: Option<i64>,
    pub model_id: String,
    pub dimension: i64,
    pub metadata_json: Option<String>,
    pub index_status: String,
}

impl ChunkWrite {
    pub fn new(
        chunk_index: i64,
        chunk_text: impl Into<String>,
        content_hash: impl Into<String>,
        model_id: impl Into<String>,
        dimension: i64,
    ) -> Self {
        Self {
            namespace: "default".to_owned(),
            chunk_index,
            chunk_text: chunk_text.into(),
            content_hash: content_hash.into(),
            token_count: None,
            model_id: model_id.into(),
            dimension,
            metadata_json: None,
            index_status: "pending".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagWrite {
    pub namespace: String,
    pub name: String,
    pub normalized_name: String,
    pub category: Option<String>,
}

impl TagWrite {
    pub fn new(name: impl Into<String>, normalized_name: impl Into<String>) -> Self {
        Self {
            namespace: "default".to_owned(),
            name: name.into(),
            normalized_name: normalized_name.into(),
            category: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkTagInsert {
    pub chunk_index: i64,
    pub tag_normalized_name: String,
    pub confidence: f64,
    pub source: String,
}

impl ChunkTagInsert {
    pub fn new(chunk_index: i64, tag_normalized_name: impl Into<String>) -> Self {
        Self {
            chunk_index,
            tag_normalized_name: tag_normalized_name.into(),
            confidence: 1.0,
            source: "auto".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngestWriteBatch {
    pub file: FileWrite,
    pub chunks: Vec<ChunkWrite>,
    pub tags: Vec<TagWrite>,
    pub chunk_tags: Vec<ChunkTagInsert>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedFile {
    pub id: i64,
    pub namespace: String,
    pub filename: String,
    pub file_hash: String,
    pub ingest_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedChunk {
    pub id: i64,
    pub file_id: i64,
    pub chunk_index: i64,
    pub content_hash: String,
    pub index_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedIngest {
    pub file: PersistedFile,
    pub chunks: Vec<PersistedChunk>,
    pub tag_ids: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedReplacement {
    pub ingest: PersistedIngest,
    pub deleted_chunk_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedDeletion {
    pub deleted_chunk_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallChunkRecord {
    pub chunk_id: i64,
    pub file_id: i64,
    pub namespace: String,
    pub chunk_text: String,
    pub source_file: String,
    pub source_type: Option<String>,
    pub metadata_json: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildChunkRecord {
    pub chunk_id: i64,
    pub file_id: i64,
    pub namespace: String,
    pub chunk_text: String,
    pub index_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceJob {
    pub id: i64,
    pub job_type: String,
    pub namespace: String,
    pub payload_json: Option<String>,
    pub status: String,
    pub progress: Option<String>,
    pub result_summary: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairTask {
    pub id: i64,
    pub namespace: String,
    pub task_type: String,
    pub target_type: String,
    pub target_id: Option<i64>,
    pub payload_json: Option<String>,
    pub status: String,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectomeEdgeRecord {
    pub namespace: String,
    pub source_tag: String,
    pub target_tag: String,
    pub weight: f64,
    pub cooccur_count: i64,
    pub last_updated: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CachedEmbedding {
    pub id: i64,
    pub namespace: String,
    pub content_hash: String,
    pub model_id: String,
    pub dimension: i64,
    pub vector: Vec<f32>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalLogWrite {
    pub query_text: String,
    pub query_hash: String,
    pub mode: String,
    pub worldview: Option<String>,
    pub entropy: Option<f64>,
    pub result_count: i64,
    pub total_time_us: Option<i64>,
    pub spike_depth: Option<i64>,
    pub emergent_count: Option<i64>,
    pub params_snapshot: Option<String>,
}

impl RetrievalLogWrite {
    pub fn new(
        query_text: impl Into<String>,
        query_hash: impl Into<String>,
        mode: impl Into<String>,
    ) -> Self {
        Self {
            query_text: query_text.into(),
            query_hash: query_hash.into(),
            mode: mode.into(),
            worldview: None,
            entropy: None,
            result_count: 0,
            total_time_us: None,
            spike_depth: None,
            emergent_count: None,
            params_snapshot: None,
        }
    }

    pub fn with_worldview(mut self, worldview: impl Into<String>) -> Self {
        self.worldview = Some(worldview.into());
        self
    }

    pub fn with_entropy(mut self, entropy: f64) -> Self {
        self.entropy = Some(entropy);
        self
    }

    pub fn with_result_count(mut self, result_count: i64) -> Self {
        self.result_count = result_count;
        self
    }

    pub fn with_total_time_us(mut self, total_time_us: i64) -> Self {
        self.total_time_us = Some(total_time_us);
        self
    }

    pub fn with_spike_depth(mut self, spike_depth: i64) -> Self {
        self.spike_depth = Some(spike_depth);
        self
    }

    pub fn with_emergent_count(mut self, emergent_count: i64) -> Self {
        self.emergent_count = Some(emergent_count);
        self
    }

    pub fn with_params_snapshot(mut self, params_snapshot: impl Into<String>) -> Self {
        self.params_snapshot = Some(params_snapshot.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalLogRecord {
    pub id: i64,
    pub namespace: String,
    pub query_text: String,
    pub query_hash: String,
    pub mode: String,
    pub worldview: Option<String>,
    pub entropy: Option<f64>,
    pub result_count: i64,
    pub total_time_us: Option<i64>,
    pub spike_depth: Option<i64>,
    pub emergent_count: Option<i64>,
    pub params_snapshot: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageStatsSnapshot {
    pub chunk_count: usize,
    pub tag_count: usize,
    pub deleted_chunk_count: usize,
    pub repair_queue_size: usize,
    pub index_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusCount {
    pub status: String,
    pub count: usize,
}
