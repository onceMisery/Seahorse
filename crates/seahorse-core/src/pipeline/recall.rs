use std::collections::HashSet;
use std::fmt;
use std::time::Instant;

use crate::embedding::{EmbeddingError, EmbeddingProvider};
use crate::index::{IndexError, SearchRequest, VectorIndex};
use crate::storage::{RecallChunkRecord, SqliteRepository, StorageError};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecallFilters {
    pub file_id: Option<i64>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallRequest {
    pub namespace: String,
    pub query: String,
    pub top_k: usize,
    pub filters: RecallFilters,
    pub timeout_ms: Option<u64>,
}

impl RecallRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            namespace: "default".to_owned(),
            query: query.into(),
            top_k: 10,
            filters: RecallFilters::default(),
            timeout_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecallResultItem {
    pub chunk_id: i64,
    pub chunk_text: String,
    pub source_file: String,
    pub tags: Vec<String>,
    pub score: f32,
    pub source_type: String,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallResponseMetadata {
    pub top_k: usize,
    pub latency_ms: u64,
    pub degraded: bool,
    pub degraded_reason: Option<String>,
    pub result_count: usize,
    pub index_state: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecallResult {
    pub results: Vec<RecallResultItem>,
    pub metadata: RecallResponseMetadata,
}

#[derive(Debug)]
pub enum RecallError {
    InvalidInput { message: String },
    Embedding(EmbeddingError),
    Storage(StorageError),
    Index(IndexError),
}

impl fmt::Display for RecallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { message } => write!(f, "invalid recall input: {message}"),
            Self::Embedding(source) => write!(f, "recall embedding failed: {source}"),
            Self::Storage(source) => write!(f, "recall storage failed: {source}"),
            Self::Index(source) => write!(f, "recall index failed: {source}"),
        }
    }
}

impl std::error::Error for RecallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Embedding(source) => Some(source),
            Self::Storage(source) => Some(source),
            Self::Index(source) => Some(source),
            Self::InvalidInput { .. } => None,
        }
    }
}

impl From<EmbeddingError> for RecallError {
    fn from(value: EmbeddingError) -> Self {
        Self::Embedding(value)
    }
}

impl From<StorageError> for RecallError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<IndexError> for RecallError {
    fn from(value: IndexError) -> Self {
        Self::Index(value)
    }
}

pub struct RecallPipeline<'a, P, I>
where
    P: EmbeddingProvider + ?Sized,
    I: VectorIndex + ?Sized,
{
    repository: &'a SqliteRepository,
    embedding_provider: &'a P,
    vector_index: &'a I,
}

impl<'a, P, I> RecallPipeline<'a, P, I>
where
    P: EmbeddingProvider + ?Sized,
    I: VectorIndex + ?Sized,
{
    pub fn new(
        repository: &'a SqliteRepository,
        embedding_provider: &'a P,
        vector_index: &'a I,
    ) -> Self {
        Self {
            repository,
            embedding_provider,
            vector_index,
        }
    }

    pub fn recall(&self, request: RecallRequest) -> Result<RecallResult, RecallError> {
        validate_request(&request)?;

        let started_at = Instant::now();
        let query_text = request.query.trim();
        let query_embedding = self.embedding_provider.embed(query_text)?;
        if query_embedding.len() != self.embedding_provider.dimension() {
            return Err(RecallError::Embedding(EmbeddingError::DimensionMismatch {
                expected: self.embedding_provider.dimension(),
                actual: query_embedding.len(),
            }));
        }

        let hits = self.vector_index.search(&SearchRequest::new(
            request.namespace.clone(),
            query_embedding,
            request.top_k,
        ))?;

        let mut seen = HashSet::new();
        let normalized_filter_tags = normalize_filter_tags(&request.filters.tags);
        let mut results = Vec::new();

        for hit in hits {
            let Some(record) = self.repository.get_chunk_record(hit.chunk_id)? else {
                continue;
            };

            if hit.namespace != request.namespace || record.namespace != request.namespace {
                continue;
            }

            if !matches_filters(&record, &request.filters, &normalized_filter_tags) {
                continue;
            }

            if !seen.insert(hit.chunk_id) {
                continue;
            }

            results.push(RecallResultItem {
                chunk_id: record.chunk_id,
                chunk_text: record.chunk_text,
                source_file: record.source_file,
                tags: record.tags,
                score: hit.score,
                source_type: "Vector".to_owned(),
                metadata_json: record.metadata_json,
            });

            if results.len() >= request.top_k {
                break;
            }
        }

        Ok(RecallResult {
            metadata: RecallResponseMetadata {
                top_k: request.top_k,
                latency_ms: started_at.elapsed().as_millis() as u64,
                degraded: false,
                degraded_reason: None,
                result_count: results.len(),
                index_state: "ready".to_owned(),
            },
            results,
        })
    }
}

fn validate_request(request: &RecallRequest) -> Result<(), RecallError> {
    if request.namespace != "default" {
        return Err(RecallError::InvalidInput {
            message: "only namespace=default is supported in MVP".to_owned(),
        });
    }

    if request.query.trim().is_empty() {
        return Err(RecallError::InvalidInput {
            message: "query must not be empty".to_owned(),
        });
    }

    if request.top_k == 0 || request.top_k > 20 {
        return Err(RecallError::InvalidInput {
            message: "top_k must be between 1 and 20".to_owned(),
        });
    }

    Ok(())
}

fn normalize_filter_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| tag.trim().to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect()
}

fn matches_filters(
    record: &RecallChunkRecord,
    filters: &RecallFilters,
    normalized_filter_tags: &[String],
) -> bool {
    if let Some(file_id) = filters.file_id {
        if record.file_id != file_id {
            return false;
        }
    }

    if normalized_filter_tags.is_empty() {
        return true;
    }

    let record_tags = record
        .tags
        .iter()
        .map(|tag| tag.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    normalized_filter_tags
        .iter()
        .all(|tag| record_tags.contains(tag))
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{RecallFilters, RecallPipeline, RecallRequest};
    use crate::embedding::StubEmbeddingProvider;
    use crate::index::{IndexEntry, IndexResult, SearchHit, SearchRequest, VectorIndex};
    use crate::pipeline::{IngestPipeline, IngestRequest};
    use crate::storage::SqliteRepository;

    const MIGRATION: &str = include_str!("../../../../migrations/0001_init.sql");

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        connection.execute_batch(MIGRATION).expect("apply migration");
        SqliteRepository::new(connection).expect("repository")
    }

    #[test]
    fn loads_chunk_metadata_tags_and_source_type() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let mut ingest_request = IngestRequest::new("alpha project recall");
        ingest_request.filename = "alpha.txt".to_owned();
        ingest_request.source_type = Some("note".to_owned());
        ingest_request.tags = vec!["Project".to_owned(), "Rust".to_owned()];
        ingest_request.metadata_json = Some("{\"kind\":\"alpha\"}".to_owned());

        let ingest_result = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(ingest_request)
            .expect("ingest");
        let recall_index = StaticHitIndex::new(
            4,
            vec![SearchHit {
                chunk_id: ingest_result.chunk_ids[0],
                namespace: "default".to_owned(),
                score: 0.9,
            }],
        );

        let result = RecallPipeline::new(&repository, &provider, &recall_index)
            .recall(RecallRequest::new("alpha project"))
            .expect("recall");

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].source_file, "alpha.txt");
        assert_eq!(result.results[0].source_type, "Vector");
        assert_eq!(result.results[0].tags, vec!["project".to_owned(), "rust".to_owned()]);
        assert_eq!(
            result.results[0].metadata_json.as_deref(),
            Some("{\"kind\":\"alpha\"}")
        );
    }

    #[test]
    fn applies_file_and_tag_filters() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let mut first_request = IngestRequest::new("alpha one");
        first_request.filename = "first.txt".to_owned();
        first_request.tags = vec!["Project".to_owned(), "Rust".to_owned()];
        let first = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(first_request)
            .expect("first ingest");

        let mut second_request = IngestRequest::new("beta two");
        second_request.filename = "second.txt".to_owned();
        second_request.tags = vec!["Rust".to_owned()];
        let second = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(second_request)
            .expect("second ingest");

        let recall_index = StaticHitIndex::new(
            4,
            vec![
                SearchHit {
                    chunk_id: first.chunk_ids[0],
                    namespace: "default".to_owned(),
                    score: 0.7,
                },
                SearchHit {
                    chunk_id: second.chunk_ids[0],
                    namespace: "default".to_owned(),
                    score: 0.6,
                },
            ],
        );

        let mut request = RecallRequest::new("rust");
        request.filters = RecallFilters {
            file_id: Some(first.file_id),
            tags: vec!["project".to_owned(), "rust".to_owned()],
        };

        let result = RecallPipeline::new(&repository, &provider, &recall_index)
            .recall(request)
            .expect("recall");

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].chunk_id, first.chunk_ids[0]);
    }

    #[test]
    fn deduplicates_duplicate_hits() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let ingest_result = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(IngestRequest::new("duplicate chunk id"))
            .expect("ingest");

        let recall_index = StaticHitIndex::new(
            4,
            vec![
                SearchHit {
                    chunk_id: ingest_result.chunk_ids[0],
                    namespace: "default".to_owned(),
                    score: 0.9,
                },
                SearchHit {
                    chunk_id: ingest_result.chunk_ids[0],
                    namespace: "default".to_owned(),
                    score: 0.8,
                },
            ],
        );

        let result = RecallPipeline::new(&repository, &provider, &recall_index)
            .recall(RecallRequest::new("duplicate chunk id"))
            .expect("recall");

        assert_eq!(result.results.len(), 1);
    }

    #[test]
    fn validates_basic_request_bounds() {
        let repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let recall_index = StaticHitIndex::new(4, Vec::new());

        let mut request = RecallRequest::new(" ");
        request.top_k = 0;

        let error = RecallPipeline::new(&repository, &provider, &recall_index)
            .recall(request)
            .expect_err("invalid request should fail");

        assert!(error.to_string().contains("query must not be empty"));
    }

    #[derive(Debug)]
    struct StaticHitIndex {
        dimension: usize,
        hits: Vec<SearchHit>,
    }

    impl StaticHitIndex {
        fn new(dimension: usize, hits: Vec<SearchHit>) -> Self {
            Self { dimension, hits }
        }
    }

    impl VectorIndex for StaticHitIndex {
        fn dimension(&self) -> usize {
            self.dimension
        }

        fn insert(&mut self, _entries: &[IndexEntry]) -> IndexResult<()> {
            Ok(())
        }

        fn search(&self, _request: &SearchRequest) -> IndexResult<Vec<SearchHit>> {
            Ok(self.hits.clone())
        }

        fn mark_deleted(&mut self, _namespace: &str, _chunk_ids: &[i64]) -> IndexResult<usize> {
            Ok(0)
        }

        fn rebuild(&mut self, _entries: &[IndexEntry]) -> IndexResult<()> {
            Ok(())
        }
    }
}
