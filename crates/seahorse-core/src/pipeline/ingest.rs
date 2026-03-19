use std::collections::BTreeMap;
use std::fmt;

use crate::embedding::{EmbeddingError, EmbeddingProvider};
use crate::index::{IndexEntry, IndexError, VectorIndex};
use crate::storage::{
    ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, PersistedFile,
    PersistedReplacement, SqliteRepository, StorageError, TagWrite,
};

use super::chunker::{chunk_text, ChunkerConfig};
use super::hashing::stable_content_hash;
use super::preprocessor::normalize_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupMode {
    Reject,
    Upsert,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestOptions {
    pub dedup_mode: DedupMode,
    pub chunk_size: usize,
    pub auto_tag: bool,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            dedup_mode: DedupMode::Reject,
            chunk_size: 512,
            auto_tag: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestRequest {
    pub namespace: String,
    pub content: String,
    pub filename: String,
    pub source_type: Option<String>,
    pub source_uri: Option<String>,
    pub tags: Vec<String>,
    pub metadata_json: Option<String>,
    pub options: IngestOptions,
}

impl IngestRequest {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            namespace: "default".to_owned(),
            content: content.into(),
            filename: "inline.txt".to_owned(),
            source_type: None,
            source_uri: None,
            tags: Vec::new(),
            metadata_json: None,
            options: IngestOptions::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestResult {
    pub file_id: i64,
    pub chunk_ids: Vec<i64>,
    pub ingest_status: String,
    pub index_status: String,
    pub file_hash: String,
    pub duplicate: bool,
    pub repair_task_id: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum IngestError {
    InvalidInput { message: String },
    UnsupportedDedupMode {
        mode: DedupMode,
        reason: &'static str,
    },
    Embedding(EmbeddingError),
    Storage(StorageError),
    Index(IndexError),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { message } => write!(f, "invalid ingest input: {message}"),
            Self::UnsupportedDedupMode { mode, reason } => {
                write!(f, "unsupported dedup mode {}: {reason}", mode.as_str())
            }
            Self::Embedding(source) => write!(f, "embedding failed: {source}"),
            Self::Storage(source) => write!(f, "storage failed: {source}"),
            Self::Index(source) => write!(f, "index failed: {source}"),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Embedding(source) => Some(source),
            Self::Storage(source) => Some(source),
            Self::Index(source) => Some(source),
            Self::InvalidInput { .. } | Self::UnsupportedDedupMode { .. } => None,
        }
    }
}

impl From<EmbeddingError> for IngestError {
    fn from(value: EmbeddingError) -> Self {
        Self::Embedding(value)
    }
}

impl From<StorageError> for IngestError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<IndexError> for IngestError {
    fn from(value: IndexError) -> Self {
        Self::Index(value)
    }
}

impl DedupMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Upsert => "upsert",
            Self::Allow => "allow",
        }
    }
}

pub struct IngestPipeline<'a, P, I>
where
    P: EmbeddingProvider + ?Sized,
    I: VectorIndex + ?Sized,
{
    repository: &'a mut SqliteRepository,
    embedding_provider: &'a P,
    vector_index: &'a mut I,
}

impl<'a, P, I> IngestPipeline<'a, P, I>
where
    P: EmbeddingProvider + ?Sized,
    I: VectorIndex + ?Sized,
{
    pub fn new(
        repository: &'a mut SqliteRepository,
        embedding_provider: &'a P,
        vector_index: &'a mut I,
    ) -> Self {
        Self {
            repository,
            embedding_provider,
            vector_index,
        }
    }

    pub fn ingest(&mut self, request: IngestRequest) -> Result<IngestResult, IngestError> {
        validate_request(&request)?;

        let normalized_content = normalize_text(&request.content);
        let trimmed_content = normalized_content.trim();
        if trimmed_content.is_empty() {
            return Err(IngestError::InvalidInput {
                message: "content is empty after normalization".to_owned(),
            });
        }

        let file_hash = stable_content_hash(trimmed_content);
        let existing_files = self
            .repository
            .list_active_files_by_hash(&request.namespace, &file_hash)?;
        if let DedupMode::Reject = request.options.dedup_mode {
            if let Some(existing) = existing_files.first() {
                return self.reuse_duplicate(existing.clone(), file_hash);
            }
        }

        let chunk_config = ChunkerConfig {
            max_chars: request.options.chunk_size,
        };
        let chunks = chunk_text(trimmed_content, chunk_config);
        if chunks.is_empty() {
            return Err(IngestError::InvalidInput {
                message: "chunker produced no chunks".to_owned(),
            });
        }

        let texts = chunks
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let embeddings = embed_texts(self.embedding_provider, &texts)?;
        if embeddings.len() != chunks.len() {
            return Err(IngestError::Embedding(EmbeddingError::ProviderFailure {
                provider: "pipeline",
                message: format!(
                    "embedding batch size mismatch: expected={}, actual={}",
                    chunks.len(),
                    embeddings.len()
                ),
            }));
        }
        for embedding in &embeddings {
            if embedding.len() != self.embedding_provider.dimension() {
                return Err(IngestError::Embedding(EmbeddingError::DimensionMismatch {
                    expected: self.embedding_provider.dimension(),
                    actual: embedding.len(),
                }));
            }
        }

        let tags = normalize_tags(&request.namespace, &request.tags);
        let batch = build_write_batch(&request, &file_hash, &chunks, &tags, self.embedding_provider);
        let mut warnings = build_ingest_warnings(&request.options);
        if matches!(request.options.dedup_mode, DedupMode::Allow) && !existing_files.is_empty() {
            warnings.push(format!(
                "duplicate content was allowed; {} active file(s) already used this file_hash",
                existing_files.len()
            ));
        }

        let PersistedReplacement {
            ingest: persisted,
            deleted_chunk_ids,
        } = if matches!(request.options.dedup_mode, DedupMode::Upsert) && !existing_files.is_empty() {
            warnings.push(format!(
                "upsert replaced {} active file(s) sharing the same file_hash",
                existing_files.len()
            ));
            self.repository.replace_ingest_batch(
                &request.namespace,
                &existing_files.iter().map(|file| file.id).collect::<Vec<_>>(),
                &batch,
            )?
        } else {
            PersistedReplacement {
                ingest: self.repository.write_ingest_batch(&batch)?,
                deleted_chunk_ids: Vec::new(),
            }
        };

        let cleanup_repair_task_id = if deleted_chunk_ids.is_empty() {
            None
        } else {
            self.cleanup_replaced_index_entries(
                &request.namespace,
                &file_hash,
                &existing_files,
                &deleted_chunk_ids,
                &mut warnings,
            )?
        };

        let chunk_ids = persisted
            .chunks
            .iter()
            .map(|chunk| chunk.id)
            .collect::<Vec<_>>();
        let index_entries = persisted
            .chunks
            .iter()
            .zip(embeddings.iter())
            .map(|(chunk, embedding)| {
                IndexEntry::new(chunk.id, request.namespace.clone(), embedding.clone())
            })
            .collect::<Vec<_>>();

        match self.vector_index.insert(&index_entries) {
            Ok(()) => {
                self.repository
                    .update_indexing_result(persisted.file.id, &chunk_ids, "ready", "ready")?;

                Ok(IngestResult {
                    file_id: persisted.file.id,
                    chunk_ids,
                    ingest_status: "ready".to_owned(),
                    index_status: "ready".to_owned(),
                    file_hash,
                    duplicate: false,
                    repair_task_id: cleanup_repair_task_id,
                    warnings,
                })
            }
            Err(index_error) => {
                self.repository.update_indexing_result(
                    persisted.file.id,
                    &chunk_ids,
                    "partial",
                    "failed",
                )?;

                let repair_payload = build_repair_payload(
                    persisted.file.id,
                    &chunk_ids,
                    self.embedding_provider.model_id(),
                    self.embedding_provider.dimension(),
                    &index_error,
                );
                let repair_task_id = self.repository.enqueue_repair_task(
                    &request.namespace,
                    "index_insert",
                    "file",
                    Some(persisted.file.id),
                    Some(&repair_payload),
                )?;

                warnings.push(format!("index update failed and was queued for repair: {index_error}"));
                if let Some(cleanup_repair_task_id) = cleanup_repair_task_id {
                    warnings.push(format!(
                        "previous version index cleanup is also queued for repair: repair_task_id={cleanup_repair_task_id}"
                    ));
                }

                Ok(IngestResult {
                    file_id: persisted.file.id,
                    chunk_ids,
                    ingest_status: "partial".to_owned(),
                    index_status: "pending_repair".to_owned(),
                    file_hash,
                    duplicate: false,
                    repair_task_id: Some(repair_task_id),
                    warnings,
                })
            }
        }
    }

    fn reuse_duplicate(
        &self,
        file: PersistedFile,
        file_hash: String,
    ) -> Result<IngestResult, IngestError> {
        let chunks = self.repository.list_chunks_by_file_id(file.id)?;
        Ok(IngestResult {
            file_id: file.id,
            chunk_ids: chunks.iter().map(|chunk| chunk.id).collect(),
            ingest_status: file.ingest_status,
            index_status: summarize_index_status(&chunks).to_owned(),
            file_hash,
            duplicate: true,
            repair_task_id: None,
            warnings: vec!["duplicate content rejected; reused latest active file".to_owned()],
        })
    }

    fn cleanup_replaced_index_entries(
        &mut self,
        namespace: &str,
        file_hash: &str,
        existing_files: &[PersistedFile],
        deleted_chunk_ids: &[i64],
        warnings: &mut Vec<String>,
    ) -> Result<Option<i64>, IngestError> {
        match self.vector_index.mark_deleted(namespace, deleted_chunk_ids) {
            Ok(affected) => {
                if affected < deleted_chunk_ids.len() {
                    warnings.push(format!(
                        "upsert removed {} chunk(s) from the index; {} chunk(s) were already absent",
                        affected,
                        deleted_chunk_ids.len() - affected
                    ));
                }
                Ok(None)
            }
            Err(index_error) => {
                let repair_payload = build_delete_repair_payload(
                    file_hash,
                    &existing_files.iter().map(|file| file.id).collect::<Vec<_>>(),
                    deleted_chunk_ids,
                    &index_error,
                );
                let repair_task_id = self.repository.enqueue_repair_task(
                    namespace,
                    "index_delete",
                    "file",
                    existing_files.first().map(|file| file.id),
                    Some(&repair_payload),
                )?;
                warnings.push(format!(
                    "previous version index cleanup failed and was queued for repair: {index_error}"
                ));
                Ok(Some(repair_task_id))
            }
        }
    }
}

fn validate_request(request: &IngestRequest) -> Result<(), IngestError> {
    if request.namespace != "default" {
        return Err(IngestError::InvalidInput {
            message: "only namespace=default is supported in MVP".to_owned(),
        });
    }

    if request.filename.trim().is_empty() {
        return Err(IngestError::InvalidInput {
            message: "filename must not be empty".to_owned(),
        });
    }

    Ok(())
}

fn build_write_batch<P: EmbeddingProvider + ?Sized>(
    request: &IngestRequest,
    file_hash: &str,
    chunks: &[super::chunker::Chunk],
    tags: &[TagWrite],
    embedding_provider: &P,
) -> IngestWriteBatch {
    let mut file = FileWrite::new(&request.filename, file_hash);
    file.namespace = request.namespace.clone();
    file.source_type = request.source_type.clone();
    file.source_uri = request.source_uri.clone();
    file.metadata_json = request.metadata_json.clone();

    let chunk_writes = chunks
        .iter()
        .map(|chunk| {
            let mut write = ChunkWrite::new(
                chunk.index as i64,
                &chunk.text,
                &chunk.content_hash,
                embedding_provider.model_id(),
                embedding_provider.dimension() as i64,
            );
            write.namespace = request.namespace.clone();
            write.token_count = Some(count_tokens(&chunk.text));
            write
        })
        .collect::<Vec<_>>();

    let chunk_tags = chunks
        .iter()
        .flat_map(|chunk| {
            tags.iter().map(move |tag| {
                let mut link = ChunkTagInsert::new(chunk.index as i64, &tag.normalized_name);
                link.source = "explicit".to_owned();
                link
            })
        })
        .collect::<Vec<_>>();

    IngestWriteBatch {
        file,
        chunks: chunk_writes,
        tags: tags.to_vec(),
        chunk_tags,
    }
}

fn normalize_tags(namespace: &str, tags: &[String]) -> Vec<TagWrite> {
    let mut normalized = BTreeMap::<String, String>::new();

    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }

        let key = trimmed.to_ascii_lowercase();
        normalized.entry(key).or_insert_with(|| trimmed.to_owned());
    }

    normalized
        .into_iter()
        .map(|(normalized_name, name)| {
            let mut tag = TagWrite::new(name, normalized_name);
            tag.namespace = namespace.to_owned();
            tag
        })
        .collect()
}

fn summarize_index_status(chunks: &[crate::storage::PersistedChunk]) -> &'static str {
    if !chunks.is_empty() && chunks.iter().all(|chunk| chunk.index_status == "ready") {
        "ready"
    } else {
        "pending_repair"
    }
}

fn build_ingest_warnings(options: &IngestOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    if options.auto_tag {
        warnings.push("auto_tag is not implemented yet; only explicit tags were used".to_owned());
    }
    warnings
}

fn build_repair_payload(
    file_id: i64,
    chunk_ids: &[i64],
    model_id: &str,
    dimension: usize,
    index_error: &IndexError,
) -> String {
    let chunk_ids_json = chunk_ids
        .iter()
        .map(|chunk_id| chunk_id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"file_id\":{file_id},\"chunk_ids\":[{chunk_ids_json}],\"model_id\":\"{}\",\"dimension\":{dimension},\"error\":\"{}\"}}",
        escape_json(model_id),
        escape_json(&index_error.to_string()),
    )
}

fn build_delete_repair_payload(
    file_hash: &str,
    file_ids: &[i64],
    chunk_ids: &[i64],
    index_error: &IndexError,
) -> String {
    let file_ids_json = file_ids
        .iter()
        .map(|file_id| file_id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let chunk_ids_json = chunk_ids
        .iter()
        .map(|chunk_id| chunk_id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"file_hash\":\"{}\",\"file_ids\":[{file_ids_json}],\"chunk_ids\":[{chunk_ids_json}],\"error\":\"{}\"}}",
        escape_json(file_hash),
        escape_json(&index_error.to_string()),
    )
}

fn count_tokens(text: &str) -> i64 {
    text.split_whitespace().count() as i64
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn embed_texts<P: EmbeddingProvider + ?Sized>(
    embedding_provider: &P,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let batch_size = embedding_provider.max_batch_size().max(1);
    let mut embeddings = Vec::with_capacity(texts.len());

    for chunk in texts.chunks(batch_size) {
        let batch_embeddings = embedding_provider.embed_batch(chunk)?;
        embeddings.extend(batch_embeddings);
    }

    Ok(embeddings)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{DedupMode, IngestPipeline, IngestRequest};
    use crate::embedding::{EmbeddingProvider, StubEmbeddingProvider};
    use crate::index::{IndexEntry, IndexError, IndexResult, InMemoryVectorIndex, SearchRequest, VectorIndex};
    use crate::storage::{apply_sqlite_migrations, SqliteRepository};

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        apply_sqlite_migrations(&connection).expect("apply migration");
        SqliteRepository::new(connection).expect("repository")
    }

    #[test]
    fn ingests_content_and_updates_index_statuses() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut index = InMemoryVectorIndex::new(4);
        let mut pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);

        let mut request = IngestRequest::new("alpha beta gamma delta");
        request.tags = vec!["Project".to_owned(), "Rust".to_owned()];

        let result = pipeline.ingest(request).expect("ingest");
        assert_eq!(result.ingest_status, "ready");
        assert_eq!(result.index_status, "ready");
        assert!(!result.chunk_ids.is_empty());

        let hits = index
            .search(&SearchRequest::new("default", provider.embed("alpha beta").expect("embedding"), 5))
            .expect("search");
        assert!(!hits.is_empty());
    }

    #[test]
    fn rejects_duplicates_by_default() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut index = InMemoryVectorIndex::new(4);

        let mut first_pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);
        let first = first_pipeline
            .ingest(IngestRequest::new("duplicate body"))
            .expect("first ingest");

        let mut second_pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);
        let second = second_pipeline
            .ingest(IngestRequest::new("duplicate body"))
            .expect("second ingest");

        assert!(second.duplicate);
        assert_eq!(second.file_id, first.file_id);
        assert_eq!(second.chunk_ids, first.chunk_ids);
    }

    #[test]
    fn returns_partial_result_when_index_insert_fails() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut index = FailingIndex::new(3);
        let mut pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);

        let result = pipeline
            .ingest(IngestRequest::new("content that will hit a failing index"))
            .expect("ingest should degrade");

        assert_eq!(result.ingest_status, "partial");
        assert_eq!(result.index_status, "pending_repair");
        assert!(result.repair_task_id.is_some());

        let file = repository
            .find_file_by_hash("default", &result.file_hash)
            .expect("find file")
            .expect("file exists");
        let chunks = repository
            .list_chunks_by_file_id(file.id)
            .expect("list chunks");

        assert_eq!(file.ingest_status, "partial");
        assert!(chunks.iter().all(|chunk| chunk.index_status == "failed"));
    }

    #[test]
    fn allows_duplicate_content_when_configured() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut index = InMemoryVectorIndex::new(4);

        let mut first_pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);
        let first = first_pipeline
            .ingest(IngestRequest::new("duplicate body"))
            .expect("first ingest");

        let mut request = IngestRequest::new("duplicate body");
        request.options.dedup_mode = DedupMode::Allow;

        let mut second_pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);
        let second = second_pipeline
            .ingest(request)
            .expect("allow should accept duplicate content");

        let active_files = repository
            .list_active_files_by_hash("default", &first.file_hash)
            .expect("list active files");

        assert_eq!(active_files.len(), 2);
        assert_ne!(first.file_id, second.file_id);
        assert_eq!(first.file_hash, second.file_hash);
        assert!(!second.duplicate);
    }

    #[test]
    fn upsert_soft_deletes_previous_file_and_replaces_index_entries() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut index = InMemoryVectorIndex::new(4);

        let mut first_pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);
        let first = first_pipeline
            .ingest(IngestRequest::new("duplicate body"))
            .expect("first ingest");
        let old_chunk_ids = first.chunk_ids.clone();

        let mut request = IngestRequest::new("duplicate body");
        request.filename = "replacement.txt".to_owned();
        request.options.dedup_mode = DedupMode::Upsert;

        let mut second_pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);
        let second = second_pipeline.ingest(request).expect("upsert should replace duplicate");

        let active_files = repository
            .list_active_files_by_hash("default", &first.file_hash)
            .expect("list active files");
        let previous_chunks = repository
            .list_chunks_by_file_id(first.file_id)
            .expect("load previous chunks");
        let previous_chunk = repository
            .get_chunk_record(old_chunk_ids[0])
            .expect("get deleted chunk record");
        let hits = index
            .search(&SearchRequest::new(
                "default",
                provider.embed("duplicate body").expect("embedding"),
                10,
            ))
            .expect("search");

        assert_eq!(active_files.len(), 1);
        assert_eq!(active_files[0].id, second.file_id);
        assert_eq!(previous_chunks[0].index_status, "deleted");
        assert!(previous_chunk.is_none());
        assert_eq!(hits.len(), second.chunk_ids.len());
        assert_eq!(hits[0].chunk_id, second.chunk_ids[0]);
    }

    #[derive(Debug)]
    struct FailingIndex {
        dimension: usize,
    }

    impl FailingIndex {
        fn new(dimension: usize) -> Self {
            Self { dimension }
        }
    }

    impl VectorIndex for FailingIndex {
        fn dimension(&self) -> usize {
            self.dimension
        }

        fn insert(&mut self, entries: &[IndexEntry]) -> IndexResult<()> {
            let actual = entries.first().map(|entry| entry.vector.len()).unwrap_or(0);
            Err(IndexError::DimensionMismatch {
                expected: self.dimension,
                actual,
            })
        }

        fn search(&self, _request: &SearchRequest) -> IndexResult<Vec<crate::index::SearchHit>> {
            Ok(Vec::new())
        }

        fn mark_deleted(&mut self, _namespace: &str, _chunk_ids: &[i64]) -> IndexResult<usize> {
            Ok(0)
        }

        fn rebuild(&mut self, _entries: &[IndexEntry]) -> IndexResult<()> {
            Ok(())
        }
    }
}
