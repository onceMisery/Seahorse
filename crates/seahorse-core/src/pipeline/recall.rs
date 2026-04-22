use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Instant;

use crate::embedding::{EmbeddingError, EmbeddingProvider};
use crate::index::{IndexError, SearchRequest, VectorIndex};
use crate::pipeline::hashing::stable_content_hash;
use crate::pipeline::tagging::resolve_tags;
use crate::storage::{RecallChunkRecord, RetrievalLogWrite, SqliteRepository, StorageError};
use crate::synapse::{Synapse, SynapseConfig};
use crate::thalamus::{ThalamicAnalysis, Thalamus, ThalamusConfig};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecallFilters {
    pub file_id: Option<i64>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallMode {
    Basic,
    TagMemo,
}

impl RecallMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::TagMemo => "tagmemo",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallRequest {
    pub namespace: String,
    pub query: String,
    pub top_k: usize,
    pub mode: RecallMode,
    pub filters: RecallFilters,
    pub timeout_ms: Option<u64>,
}

impl RecallRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            namespace: "default".to_owned(),
            query: query.into(),
            top_k: 10,
            mode: RecallMode::Basic,
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

#[derive(Debug, Clone, PartialEq)]
pub struct RecallResponseMetadata {
    pub top_k: usize,
    pub latency_ms: u64,
    pub degraded: bool,
    pub degraded_reason: Option<String>,
    pub result_count: usize,
    pub index_state: String,
    pub worldview: Option<String>,
    pub entropy: Option<f32>,
    pub focus_terms: Vec<String>,
    pub weak_signal_allowed: bool,
    pub weak_signal_reason: String,
    pub association_allowed: Option<bool>,
    pub association_reason: Option<String>,
    pub vector_result_count: usize,
    pub association_result_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecallResult {
    pub results: Vec<RecallResultItem>,
    pub metadata: RecallResponseMetadata,
}

#[derive(Debug)]
pub enum RecallError {
    InvalidInput { message: String },
    Timeout { timeout_ms: u64, elapsed_ms: u64 },
    Embedding(EmbeddingError),
    Storage(StorageError),
    Index(IndexError),
}

impl fmt::Display for RecallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { message } => write!(f, "invalid recall input: {message}"),
            Self::Timeout {
                timeout_ms,
                elapsed_ms,
            } => write!(
                f,
                "recall timed out: timeout_ms={timeout_ms}, elapsed_ms={elapsed_ms}"
            ),
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
            Self::InvalidInput { .. } | Self::Timeout { .. } => None,
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
    repository: &'a mut SqliteRepository,
    embedding_provider: &'a P,
    vector_index: &'a I,
}

impl<'a, P, I> RecallPipeline<'a, P, I>
where
    P: EmbeddingProvider + ?Sized,
    I: VectorIndex + ?Sized,
{
    pub fn new(
        repository: &'a mut SqliteRepository,
        embedding_provider: &'a P,
        vector_index: &'a I,
    ) -> Self {
        Self {
            repository,
            embedding_provider,
            vector_index,
        }
    }

    pub fn recall(&mut self, request: RecallRequest) -> Result<RecallResult, RecallError> {
        validate_request(&request)?;

        let started_at = Instant::now();
        ensure_not_timed_out(started_at, request.timeout_ms)?;
        let query_text = request.query.trim();
        let thalamic_analysis = analyze_query_with_thalamus(query_text, request.top_k);
        let query_embedding = self.embedding_provider.embed(query_text)?;
        ensure_not_timed_out(started_at, request.timeout_ms)?;
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
        ensure_not_timed_out(started_at, request.timeout_ms)?;

        let mut seen = HashSet::new();
        let normalized_filter_tags = normalize_filter_tags(&request.filters.tags);
        let mut results = collect_vector_results(
            self.repository,
            &request,
            &normalized_filter_tags,
            hits,
            &mut seen,
            started_at,
        )?;
        let vector_result_count = results.len();
        let mut association_result_count = 0;
        let mut association_attempted = false;

        if request.mode == RecallMode::TagMemo
            && results.len() < request.top_k
            && thalamic_analysis.route.allow_association
        {
            association_attempted = true;
            let remaining = request.top_k - results.len();
            let associated = collect_tagmemo_results(
                self.repository,
                &request,
                &normalized_filter_tags,
                &mut seen,
                remaining,
                started_at,
            )?;
            association_result_count = associated.len();
            results.extend(associated);
        }

        let recall_duration = started_at.elapsed();
        let latency_ms = recall_duration.as_millis().min(u128::from(u64::MAX)) as u64;
        let total_time_us = recall_duration.as_micros().min(i64::MAX as u128) as i64;
        let query_hash = stable_content_hash(query_text);
        let mut retrieval_log =
            RetrievalLogWrite::new(query_text, query_hash, request.mode.as_str())
                .with_worldview(thalamic_analysis.worldview.clone())
                .with_entropy(f64::from(thalamic_analysis.entropy))
                .with_result_count(results.len() as i64)
                .with_total_time_us(total_time_us)
                .with_params_snapshot(build_retrieval_log_params_snapshot(
                    &request,
                    &thalamic_analysis,
                    vector_result_count,
                    association_result_count,
                ));
        if request.mode == RecallMode::TagMemo {
            retrieval_log = retrieval_log
                .with_spike_depth(if association_attempted { 1 } else { 0 })
                .with_emergent_count(association_result_count as i64);
        }
        self.repository
            .append_retrieval_log(&request.namespace, &retrieval_log)?;

        let association_allowed = if request.mode == RecallMode::TagMemo {
            Some(thalamic_analysis.route.allow_association)
        } else {
            None
        };
        let association_reason = if request.mode == RecallMode::TagMemo {
            Some(thalamic_analysis.route.association_reason.clone())
        } else {
            None
        };

        Ok(RecallResult {
            metadata: RecallResponseMetadata {
                top_k: request.top_k,
                latency_ms,
                degraded: false,
                degraded_reason: None,
                result_count: results.len(),
                index_state: "ready".to_owned(),
                worldview: Some(thalamic_analysis.worldview),
                entropy: Some(thalamic_analysis.entropy),
                focus_terms: thalamic_analysis.focus_terms.clone(),
                weak_signal_allowed: thalamic_analysis.route.allow_weak_signal,
                weak_signal_reason: thalamic_analysis.route.weak_signal_reason.clone(),
                association_allowed,
                association_reason,
                vector_result_count,
                association_result_count,
            },
            results,
        })
    }
}

fn analyze_query_with_thalamus(query_text: &str, top_k: usize) -> ThalamicAnalysis {
    Thalamus::new(ThalamusConfig::default()).analyze(query_text, top_k)
}

fn build_spike_association_metadata_json(
    base_metadata_json: Option<&str>,
    seed_tags: &[String],
    matched_tags: &[String],
    score: f32,
) -> Option<String> {
    let mut metadata = match base_metadata_json {
        Some(json) => match serde_json::from_str::<Value>(json) {
            Ok(Value::Object(object)) => object,
            _ => Map::new(),
        },
        None => Map::new(),
    };

    metadata.insert(
        "seahorse_association".to_owned(),
        Value::Object(Map::from_iter([
            ("mode".to_owned(), Value::String("tagmemo".to_owned())),
            (
                "seed_tags".to_owned(),
                Value::Array(seed_tags.iter().cloned().map(Value::String).collect()),
            ),
            (
                "matched_tags".to_owned(),
                Value::Array(matched_tags.iter().cloned().map(Value::String).collect()),
            ),
            ("score".to_owned(), Value::from(f64::from(score))),
        ])),
    );

    serde_json::to_string(&Value::Object(metadata)).ok()
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

fn build_retrieval_log_params_snapshot(
    request: &RecallRequest,
    thalamic_analysis: &ThalamicAnalysis,
    vector_result_count: usize,
    association_result_count: usize,
) -> String {
    json!({
        "mode": request.mode.as_str(),
        "top_k": request.top_k,
        "filter_file_id": request.filters.file_id,
        "filter_tag_count": request.filters.tags.len(),
        "timeout_ms": request.timeout_ms,
        "worldview": thalamic_analysis.worldview,
        "entropy": thalamic_analysis.entropy,
        "focus_terms": thalamic_analysis.focus_terms,
        "association_allowed": thalamic_analysis.route.allow_association,
        "association_reason": thalamic_analysis.route.association_reason,
        "weak_signal_allowed": thalamic_analysis.route.allow_weak_signal,
        "weak_signal_reason": thalamic_analysis.route.weak_signal_reason,
        "vector_result_count": vector_result_count,
        "association_result_count": association_result_count,
    })
    .to_string()
}

fn collect_vector_results(
    repository: &mut SqliteRepository,
    request: &RecallRequest,
    normalized_filter_tags: &[String],
    hits: Vec<crate::index::SearchHit>,
    seen: &mut HashSet<i64>,
    started_at: Instant,
) -> Result<Vec<RecallResultItem>, RecallError> {
    let mut results = Vec::new();

    for hit in hits {
        ensure_not_timed_out(started_at, request.timeout_ms)?;
        let Some(record) = repository.get_chunk_record(hit.chunk_id)? else {
            continue;
        };

        if hit.namespace != request.namespace || record.namespace != request.namespace {
            continue;
        }

        if !matches_filters(&record, &request.filters, normalized_filter_tags) {
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

    Ok(results)
}

fn collect_tagmemo_results(
    repository: &mut SqliteRepository,
    request: &RecallRequest,
    normalized_filter_tags: &[String],
    seen: &mut HashSet<i64>,
    limit: usize,
    started_at: Instant,
) -> Result<Vec<RecallResultItem>, RecallError> {
    ensure_not_timed_out(started_at, request.timeout_ms)?;
    let mut seed_tags = resolve_tags(&[], request.query.trim(), None, true)
        .into_iter()
        .map(|tag| tag.normalized_name)
        .collect::<Vec<_>>();
    ensure_not_timed_out(started_at, request.timeout_ms)?;
    seed_tags.extend(normalized_filter_tags.iter().cloned());
    seed_tags.sort();
    seed_tags.dedup();

    if seed_tags.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut synapse = Synapse::new(SynapseConfig {
        max_signals: limit.saturating_mul(4).max(seed_tags.len()),
        neighbor_limit: limit.saturating_mul(2).max(4),
    });
    for seed_tag in &seed_tags {
        ensure_not_timed_out(started_at, request.timeout_ms)?;
        synapse.activate_connectome_neighbors(repository, &request.namespace, seed_tag, 1.0)?;
    }

    let mut signal_strengths = HashMap::<String, f32>::new();
    for signal in synapse.signals() {
        signal_strengths
            .entry(signal.tag.clone())
            .and_modify(|value| *value = value.max(signal.potential))
            .or_insert(signal.potential);
    }

    let candidate_tags = signal_strengths.keys().cloned().collect::<Vec<_>>();
    ensure_not_timed_out(started_at, request.timeout_ms)?;
    let records = repository.list_chunk_records_by_any_tags(&request.namespace, &candidate_tags)?;

    let mut results = Vec::new();
    for record in records {
        ensure_not_timed_out(started_at, request.timeout_ms)?;
        if !matches_filters(&record, &request.filters, normalized_filter_tags) {
            continue;
        }

        if !seen.insert(record.chunk_id) {
            continue;
        }

        let score = record
            .tags
            .iter()
            .filter_map(|tag| signal_strengths.get(tag))
            .copied()
            .sum::<f32>();
        let matched_tags = record
            .tags
            .iter()
            .filter(|tag| signal_strengths.contains_key(*tag))
            .cloned()
            .collect::<Vec<_>>();

        results.push(RecallResultItem {
            chunk_id: record.chunk_id,
            chunk_text: record.chunk_text,
            source_file: record.source_file,
            tags: record.tags,
            score,
            source_type: "SpikeAssociation".to_owned(),
            metadata_json: build_spike_association_metadata_json(
                record.metadata_json.as_deref(),
                &seed_tags,
                &matched_tags,
                score,
            ),
        });
    }

    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    results.truncate(limit);

    Ok(results)
}

fn ensure_not_timed_out(started_at: Instant, timeout_ms: Option<u64>) -> Result<(), RecallError> {
    let Some(timeout_ms) = timeout_ms else {
        return Ok(());
    };

    let elapsed_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    if elapsed_ms >= timeout_ms {
        return Err(RecallError::Timeout {
            timeout_ms,
            elapsed_ms,
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
    use serde_json::Value;

    use super::{RecallFilters, RecallMode, RecallPipeline, RecallRequest};
    use crate::embedding::{EmbeddingProvider, EmbeddingResult, StubEmbeddingProvider};
    use crate::index::{IndexEntry, IndexResult, SearchHit, SearchRequest, VectorIndex};
    use crate::pipeline::{IngestPipeline, IngestRequest};
    use crate::storage::{apply_sqlite_migrations, SqliteRepository};

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        apply_sqlite_migrations(&connection).expect("apply migration");
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

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(RecallRequest::new("alpha project"))
            .expect("recall");

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].source_file, "alpha.txt");
        assert_eq!(result.results[0].source_type, "Vector");
        assert_eq!(
            result.results[0].tags,
            vec!["project".to_owned(), "rust".to_owned()]
        );
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

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
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

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(RecallRequest::new("duplicate chunk id"))
            .expect("recall");

        assert_eq!(result.results.len(), 1);
    }

    #[test]
    fn validates_basic_request_bounds() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let recall_index = StaticHitIndex::new(4, Vec::new());

        let mut request = RecallRequest::new(" ");
        request.top_k = 0;

        let error = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(request)
            .expect_err("invalid request should fail");

        assert!(error.to_string().contains("query must not be empty"));
    }

    #[test]
    fn times_out_when_timeout_budget_is_exhausted() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let recall_index = StaticHitIndex::new(4, Vec::new());

        let mut request = RecallRequest::new("alpha");
        request.timeout_ms = Some(0);

        let error = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(request)
            .expect_err("timeout budget should fail recall");

        match error {
            super::RecallError::Timeout { timeout_ms, .. } => assert_eq!(timeout_ms, 0),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn persists_retrieval_log_for_successful_recall() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let ingest_result = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(IngestRequest::new("alpha recall log"))
            .expect("ingest");
        let recall_index = StaticHitIndex::new(
            4,
            vec![SearchHit {
                chunk_id: ingest_result.chunk_ids[0],
                namespace: "default".to_owned(),
                score: 0.95,
            }],
        );

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(RecallRequest::new("alpha recall log"))
            .expect("recall");
        let logs = repository
            .list_retrieval_logs("default", 10)
            .expect("list retrieval logs");

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.metadata.worldview.as_deref(), Some("default"));
        assert!(result.metadata.entropy.is_some());
        assert_eq!(
            result.metadata.focus_terms,
            vec!["recall".to_owned(), "alpha".to_owned(), "log".to_owned()]
        );
        assert!(!result.metadata.weak_signal_allowed);
        assert_eq!(result.metadata.weak_signal_reason, "tide_not_implemented");
        assert_eq!(result.metadata.association_allowed, None);
        assert_eq!(result.metadata.association_reason, None);
        assert_eq!(result.metadata.vector_result_count, 1);
        assert_eq!(result.metadata.association_result_count, 0);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].mode, "basic");
        assert_eq!(logs[0].query_text, "alpha recall log");
        assert_eq!(logs[0].worldview.as_deref(), Some("default"));
        assert!(logs[0].entropy.is_some());
        assert_eq!(logs[0].result_count, 1);
        assert!(logs[0].total_time_us.is_some());
        assert!(logs[0].params_snapshot.is_some());
    }

    #[test]
    fn writes_technical_thalamus_analysis_into_recall_metadata() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let ingest_result = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(IngestRequest::new("rust vector recall architecture"))
            .expect("ingest");
        let recall_index = StaticHitIndex::new(
            4,
            vec![SearchHit {
                chunk_id: ingest_result.chunk_ids[0],
                namespace: "default".to_owned(),
                score: 0.95,
            }],
        );

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(RecallRequest::new("rust vector index recall"))
            .expect("recall");
        let logs = repository
            .list_retrieval_logs("default", 10)
            .expect("list retrieval logs");
        let params_snapshot = serde_json::from_str::<Value>(
            logs[0]
                .params_snapshot
                .as_deref()
                .expect("params snapshot should exist"),
        )
        .expect("params snapshot should be valid json");

        assert_eq!(result.metadata.worldview.as_deref(), Some("technical"));
        assert!(result.metadata.entropy.is_some());
        assert_eq!(
            result.metadata.focus_terms,
            vec!["recall".to_owned(), "vector".to_owned(), "index".to_owned()]
        );
        assert!(!result.metadata.weak_signal_allowed);
        assert_eq!(result.metadata.weak_signal_reason, "tide_not_implemented");
        assert_eq!(result.metadata.association_allowed, None);
        assert_eq!(result.metadata.association_reason, None);
        assert_eq!(logs[0].worldview.as_deref(), Some("technical"));
        assert!(logs[0].entropy.is_some());
        assert_eq!(
            params_snapshot["focus_terms"],
            Value::Array(vec![
                Value::String("recall".to_owned()),
                Value::String("vector".to_owned()),
                Value::String("index".to_owned()),
            ])
        );
    }

    #[test]
    fn persists_retrieval_log_for_empty_result_recall() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let recall_index = StaticHitIndex::new(4, Vec::new());

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(RecallRequest::new("missing recall log"))
            .expect("recall");
        let logs = repository
            .list_retrieval_logs("default", 10)
            .expect("list retrieval logs");

        assert!(result.results.is_empty());
        assert_eq!(result.metadata.worldview.as_deref(), Some("default"));
        assert!(result.metadata.entropy.is_some());
        assert_eq!(
            result.metadata.focus_terms,
            vec!["missing".to_owned(), "recall".to_owned(), "log".to_owned()]
        );
        assert!(!result.metadata.weak_signal_allowed);
        assert_eq!(result.metadata.weak_signal_reason, "tide_not_implemented");
        assert_eq!(result.metadata.association_allowed, None);
        assert_eq!(result.metadata.association_reason, None);
        assert_eq!(result.metadata.vector_result_count, 0);
        assert_eq!(result.metadata.association_result_count, 0);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].query_text, "missing recall log");
        assert_eq!(logs[0].worldview.as_deref(), Some("default"));
        assert!(logs[0].entropy.is_some());
        assert_eq!(logs[0].result_count, 0);
    }

    #[test]
    fn recalls_associated_chunks_in_tagmemo_mode_when_vector_hits_are_empty() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let mut connectome_request = IngestRequest::new("project rust anchor");
        connectome_request.filename = "connectome.txt".to_owned();
        connectome_request.tags = vec!["project".to_owned(), "rust".to_owned()];
        IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(connectome_request)
            .expect("seed connectome");

        let mut associated_request = IngestRequest::new("rust compiler deep dive");
        associated_request.filename = "rust.txt".to_owned();
        associated_request.tags = vec!["rust".to_owned()];
        let associated_ingest = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(associated_request)
            .expect("seed associated chunk");

        let recall_index = StaticHitIndex::new(4, Vec::new());
        let mut request = RecallRequest::new("project");
        request.mode = RecallMode::TagMemo;

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(request)
            .expect("tagmemo recall");
        let logs = repository
            .list_retrieval_logs("default", 10)
            .expect("list retrieval logs");

        assert!(
            result
                .results
                .iter()
                .any(|item| item.chunk_id == associated_ingest.chunk_ids[0]
                    && item.source_type == "SpikeAssociation"),
            "tagmemo should recover the associated rust chunk through connectome"
        );
        assert_eq!(result.metadata.association_allowed, Some(true));
        assert_eq!(
            result.metadata.association_reason.as_deref(),
            Some("tagmemo_allowed")
        );
        assert_eq!(result.metadata.vector_result_count, 0);
        assert_eq!(result.metadata.association_result_count, 2);
        assert_eq!(logs[0].spike_depth, Some(1));
        assert_eq!(logs[0].emergent_count, Some(2));
    }

    #[test]
    fn appends_spike_association_metadata_for_tagmemo_results() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let mut connectome_request = IngestRequest::new("project rust anchor");
        connectome_request.filename = "connectome.txt".to_owned();
        connectome_request.tags = vec!["project".to_owned(), "rust".to_owned()];
        IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(connectome_request)
            .expect("seed connectome");

        let mut associated_request = IngestRequest::new("rust compiler deep dive");
        associated_request.filename = "rust.txt".to_owned();
        associated_request.tags = vec!["rust".to_owned()];
        associated_request.metadata_json = Some("{\"kind\":\"rust-note\"}".to_owned());
        let associated_ingest = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(associated_request)
            .expect("seed associated chunk");

        let recall_index = StaticHitIndex::new(4, Vec::new());
        let mut request = RecallRequest::new("project");
        request.mode = RecallMode::TagMemo;

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(request)
            .expect("tagmemo recall");
        let associated = result
            .results
            .iter()
            .find(|item| item.chunk_id == associated_ingest.chunk_ids[0])
            .expect("associated result should exist");
        let metadata = serde_json::from_str::<Value>(
            associated
                .metadata_json
                .as_deref()
                .expect("association metadata should exist"),
        )
        .expect("association metadata should be valid json");

        assert_eq!(metadata["kind"], Value::String("rust-note".to_owned()));
        assert_eq!(
            metadata["seahorse_association"]["mode"],
            Value::String("tagmemo".to_owned())
        );
        assert_eq!(
            metadata["seahorse_association"]["seed_tags"],
            Value::Array(vec![Value::String("project".to_owned())])
        );
        assert_eq!(
            metadata["seahorse_association"]["matched_tags"],
            Value::Array(vec![Value::String("rust".to_owned())])
        );
        assert!(metadata["seahorse_association"]["score"].is_number());
    }

    #[test]
    fn skips_tagmemo_association_when_thalamus_blocks_route() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        let mut connectome_request = IngestRequest::new("care rust anchor");
        connectome_request.filename = "connectome.txt".to_owned();
        connectome_request.tags = vec!["care".to_owned(), "rust".to_owned()];
        IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(connectome_request)
            .expect("seed connectome");

        let mut associated_request = IngestRequest::new("rust compiler deep dive");
        associated_request.filename = "rust.txt".to_owned();
        associated_request.tags = vec!["rust".to_owned()];
        IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(associated_request)
            .expect("seed associated chunk");

        let recall_index = StaticHitIndex::new(4, Vec::new());
        let mut request = RecallRequest::new("care feel grief");
        request.mode = RecallMode::TagMemo;

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(request)
            .expect("tagmemo recall");
        let logs = repository
            .list_retrieval_logs("default", 10)
            .expect("list retrieval logs");
        let params_snapshot = serde_json::from_str::<Value>(
            logs[0]
                .params_snapshot
                .as_deref()
                .expect("params snapshot should exist"),
        )
        .expect("params snapshot should be valid json");

        assert!(result.results.is_empty());
        assert_eq!(result.metadata.association_allowed, Some(false));
        assert_eq!(
            result.metadata.association_reason.as_deref(),
            Some("worldview_emotional_blocks_association")
        );
        assert_eq!(
            result.metadata.focus_terms,
            vec!["grief".to_owned(), "care".to_owned(), "feel".to_owned()]
        );
        assert!(!result.metadata.weak_signal_allowed);
        assert_eq!(result.metadata.weak_signal_reason, "tide_not_implemented");
        assert_eq!(result.metadata.vector_result_count, 0);
        assert_eq!(result.metadata.association_result_count, 0);
        assert_eq!(logs[0].spike_depth, Some(0));
        assert_eq!(logs[0].emergent_count, Some(0));
        assert_eq!(params_snapshot["association_allowed"], Value::Bool(false));
        assert_eq!(
            params_snapshot["association_reason"],
            Value::String("worldview_emotional_blocks_association".to_owned())
        );
        assert_eq!(
            params_snapshot["focus_terms"],
            Value::Array(vec![
                Value::String("grief".to_owned()),
                Value::String("care".to_owned()),
                Value::String("feel".to_owned()),
            ])
        );
    }

    #[test]
    fn times_out_during_tagmemo_association_expansion() {
        let mut repository = repository_with_schema();
        let provider = FixedEmbeddingProvider::new(4);
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        for occurrence in 0..8 {
            let mut connectome_request = IngestRequest::new(format!("alpha bridge {occurrence}"));
            connectome_request.filename = format!("connectome-{occurrence}.txt");
            connectome_request.tags = vec!["alpha".to_owned(), "bridge".to_owned()];
            IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
                .ingest(connectome_request)
                .expect("seed connectome");
        }

        let mut candidate_request = IngestRequest::new("alpha candidate");
        candidate_request.filename = "candidate.txt".to_owned();
        candidate_request.tags = vec!["alpha".to_owned(), "candidate".to_owned()];
        IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(candidate_request)
            .expect("seed candidate");

        let recall_index = StaticHitIndex::new(4, Vec::new());
        let mut request = RecallRequest::new("alpha ".repeat(200_000));
        request.mode = RecallMode::TagMemo;
        request.timeout_ms = Some(1);

        let error = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(request)
            .expect_err("tagmemo expansion should respect timeout budget");

        assert!(
            matches!(error, super::RecallError::Timeout { .. }),
            "expected timeout during tagmemo association, got {error:?}"
        );
    }

    #[test]
    fn ranks_tagmemo_candidates_by_final_signal_score_before_truncation() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = StaticHitIndex::new(4, Vec::new());

        for occurrence in 0..3 {
            let mut request = IngestRequest::new(format!("alpha zeta bridge {occurrence}"));
            request.filename = format!("alpha-zeta-{occurrence}.txt");
            request.tags = vec!["alpha".to_owned(), "zeta".to_owned()];
            IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
                .ingest(request)
                .expect("seed alpha-zeta connectome");
        }

        for (filename, related_tag) in [("alpha-gamma.txt", "gamma"), ("alpha-delta.txt", "delta")]
        {
            let mut request = IngestRequest::new(format!("alpha {related_tag} bridge"));
            request.filename = filename.to_owned();
            request.tags = vec!["alpha".to_owned(), related_tag.to_owned()];
            IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
                .ingest(request)
                .expect("seed weaker connectome edge");
        }

        let mut strong_request = IngestRequest::new("alpha strongest candidate");
        strong_request.filename = "strong.txt".to_owned();
        strong_request.tags = vec!["alpha".to_owned(), "candidate".to_owned()];
        let strong = IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(strong_request)
            .expect("seed strong candidate");

        let mut weak_request = IngestRequest::new("gamma delta candidate");
        weak_request.filename = "weak.txt".to_owned();
        weak_request.tags = vec![
            "gamma".to_owned(),
            "delta".to_owned(),
            "candidate".to_owned(),
        ];
        IngestPipeline::new(&mut repository, &provider, &mut ingest_index)
            .ingest(weak_request)
            .expect("seed weak candidate");

        let recall_index = StaticHitIndex::new(4, Vec::new());
        let mut request = RecallRequest::new("alpha");
        request.mode = RecallMode::TagMemo;
        request.top_k = 1;
        request.filters = RecallFilters {
            file_id: None,
            tags: vec!["candidate".to_owned()],
        };

        let result = RecallPipeline::new(&mut repository, &provider, &recall_index)
            .recall(request)
            .expect("tagmemo recall");

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].chunk_id, strong.chunk_ids[0]);
        assert_eq!(result.results[0].source_type, "SpikeAssociation");
        assert!(result.results[0].score > 0.9);
    }

    #[derive(Debug)]
    struct FixedEmbeddingProvider {
        dimension: usize,
    }

    impl FixedEmbeddingProvider {
        fn new(dimension: usize) -> Self {
            Self { dimension }
        }
    }

    impl EmbeddingProvider for FixedEmbeddingProvider {
        fn embed(&self, _text: &str) -> EmbeddingResult<Vec<f32>> {
            Ok(vec![0.0; self.dimension])
        }

        fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.0; self.dimension]).collect())
        }

        fn model_id(&self) -> &str {
            "fixed-test"
        }

        fn dimension(&self) -> usize {
            self.dimension
        }

        fn max_batch_size(&self) -> usize {
            32
        }
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
