use std::fmt;

use crate::index::{IndexError, VectorIndex};
use crate::storage::{PersistedDeletion, SqliteRepository, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgetMode {
    Soft,
    Hard,
}

impl ForgetMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Hard => "hard",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgetRequest {
    pub namespace: String,
    pub chunk_ids: Vec<i64>,
    pub file_id: Option<i64>,
    pub mode: ForgetMode,
}

impl Default for ForgetRequest {
    fn default() -> Self {
        Self {
            namespace: "default".to_owned(),
            chunk_ids: Vec::new(),
            file_id: None,
            mode: ForgetMode::Soft,
        }
    }
}

impl ForgetRequest {
    pub fn for_file(file_id: i64) -> Self {
        Self {
            file_id: Some(file_id),
            ..Self::default()
        }
    }

    pub fn for_chunks(chunk_ids: Vec<i64>) -> Self {
        Self {
            chunk_ids,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgetResult {
    pub affected_chunks: usize,
    pub index_cleanup_status: String,
    pub repair_task_id: Option<i64>,
}

#[derive(Debug)]
pub enum ForgetError {
    InvalidInput { message: String },
    Storage(StorageError),
    Index(IndexError),
}

impl fmt::Display for ForgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { message } => write!(f, "invalid forget input: {message}"),
            Self::Storage(source) => write!(f, "forget storage failed: {source}"),
            Self::Index(source) => write!(f, "forget index failed: {source}"),
        }
    }
}

impl std::error::Error for ForgetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(source) => Some(source),
            Self::Index(source) => Some(source),
            Self::InvalidInput { .. } => None,
        }
    }
}

impl From<StorageError> for ForgetError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<IndexError> for ForgetError {
    fn from(value: IndexError) -> Self {
        Self::Index(value)
    }
}

pub struct ForgetPipeline<'a, I>
where
    I: VectorIndex + ?Sized,
{
    repository: &'a mut SqliteRepository,
    vector_index: &'a mut I,
}

impl<'a, I> ForgetPipeline<'a, I>
where
    I: VectorIndex + ?Sized,
{
    pub fn new(repository: &'a mut SqliteRepository, vector_index: &'a mut I) -> Self {
        Self {
            repository,
            vector_index,
        }
    }

    pub fn forget(&mut self, request: ForgetRequest) -> Result<ForgetResult, ForgetError> {
        validate_request(&request)?;

        let deletion = if let Some(file_id) = request.file_id {
            self.repository
                .soft_delete_files(&request.namespace, &[file_id])?
        } else {
            self.repository
                .soft_delete_chunks(&request.namespace, &dedup_chunk_ids(&request.chunk_ids))?
        };

        self.cleanup_index(request, deletion)
    }

    fn cleanup_index(
        &mut self,
        request: ForgetRequest,
        deletion: PersistedDeletion,
    ) -> Result<ForgetResult, ForgetError> {
        if deletion.deleted_chunk_ids.is_empty() {
            return Ok(ForgetResult {
                affected_chunks: 0,
                index_cleanup_status: "completed".to_owned(),
                repair_task_id: None,
            });
        }

        match self
            .vector_index
            .mark_deleted(&request.namespace, &deletion.deleted_chunk_ids)
        {
            Ok(_) => Ok(ForgetResult {
                affected_chunks: deletion.deleted_chunk_ids.len(),
                index_cleanup_status: "completed".to_owned(),
                repair_task_id: None,
            }),
            Err(index_error) => {
                let repair_payload = build_delete_repair_payload(
                    request.file_id,
                    &deletion.deleted_chunk_ids,
                    &index_error,
                );
                let repair_task_id = self.repository.enqueue_repair_task(
                    &request.namespace,
                    "index_delete",
                    if request.file_id.is_some() {
                        "file"
                    } else {
                        "chunk"
                    },
                    request
                        .file_id
                        .or_else(|| deletion.deleted_chunk_ids.first().copied()),
                    Some(&repair_payload),
                )?;

                Ok(ForgetResult {
                    affected_chunks: deletion.deleted_chunk_ids.len(),
                    index_cleanup_status: "pending".to_owned(),
                    repair_task_id: Some(repair_task_id),
                })
            }
        }
    }
}

fn validate_request(request: &ForgetRequest) -> Result<(), ForgetError> {
    if request.namespace != "default" {
        return Err(ForgetError::InvalidInput {
            message: "only namespace=default is supported in MVP".to_owned(),
        });
    }

    if request.mode != ForgetMode::Soft {
        return Err(ForgetError::InvalidInput {
            message: "only mode=soft is supported in MVP".to_owned(),
        });
    }

    if request.file_id.is_some() && !request.chunk_ids.is_empty() {
        return Err(ForgetError::InvalidInput {
            message: "provide either file_id or chunk_ids, not both".to_owned(),
        });
    }

    if request.file_id.is_none() && request.chunk_ids.is_empty() {
        return Err(ForgetError::InvalidInput {
            message: "either file_id or chunk_ids is required".to_owned(),
        });
    }

    if let Some(file_id) = request.file_id {
        if file_id <= 0 {
            return Err(ForgetError::InvalidInput {
                message: "file_id must be positive".to_owned(),
            });
        }
    }

    if request.chunk_ids.iter().any(|chunk_id| *chunk_id <= 0) {
        return Err(ForgetError::InvalidInput {
            message: "chunk_ids must contain only positive integers".to_owned(),
        });
    }

    Ok(())
}

fn dedup_chunk_ids(chunk_ids: &[i64]) -> Vec<i64> {
    let mut ids = chunk_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn build_delete_repair_payload(
    file_id: Option<i64>,
    chunk_ids: &[i64],
    index_error: &IndexError,
) -> String {
    let chunk_ids_json = chunk_ids
        .iter()
        .map(|chunk_id| chunk_id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let file_id_json = file_id
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_owned());
    format!(
        "{{\"file_id\":{file_id_json},\"chunk_ids\":[{chunk_ids_json}],\"error\":\"{}\"}}",
        escape_json(&index_error.to_string()),
    )
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{ForgetMode, ForgetPipeline, ForgetRequest};
    use crate::embedding::StubEmbeddingProvider;
    use crate::index::{
        InMemoryVectorIndex, IndexEntry, IndexError, IndexResult, SearchHit, SearchRequest,
        VectorIndex,
    };
    use crate::pipeline::{IngestPipeline, IngestRequest, RecallPipeline, RecallRequest};
    use crate::storage::{apply_sqlite_migrations, SqliteRepository};

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        apply_sqlite_migrations(&connection).expect("apply migration");
        SqliteRepository::new(connection).expect("repository")
    }

    #[test]
    fn forgets_file_by_file_id() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut index = InMemoryVectorIndex::new(4);

        let mut ingest_pipeline = IngestPipeline::new(&mut repository, &provider, &mut index);
        let ingest = ingest_pipeline
            .ingest(IngestRequest::new("forget me"))
            .expect("ingest");

        let mut forget_pipeline = ForgetPipeline::new(&mut repository, &mut index);
        let result = forget_pipeline
            .forget(ForgetRequest::for_file(ingest.file_id))
            .expect("forget");
        let recall = RecallPipeline::new(&repository, &provider, &index)
            .recall(RecallRequest::new("forget me"))
            .expect("recall");

        assert_eq!(result.affected_chunks, ingest.chunk_ids.len());
        assert_eq!(result.index_cleanup_status, "completed");
        assert!(result.repair_task_id.is_none());
        assert!(recall.results.is_empty());
    }

    #[test]
    fn forgets_specific_chunks() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = InMemoryVectorIndex::new(4);

        let mut request = IngestRequest::new("alpha beta gamma delta epsilon");
        request.options.chunk_size = 5;
        let mut ingest_pipeline =
            IngestPipeline::new(&mut repository, &provider, &mut ingest_index);
        let ingest = ingest_pipeline.ingest(request).expect("ingest");

        let target_chunk_id = ingest.chunk_ids[0];
        let mut forget_pipeline = ForgetPipeline::new(&mut repository, &mut ingest_index);
        let result = forget_pipeline
            .forget(ForgetRequest::for_chunks(vec![target_chunk_id]))
            .expect("forget chunk");
        let record = repository
            .get_chunk_record(target_chunk_id)
            .expect("load chunk")
            .is_none();

        assert_eq!(result.affected_chunks, 1);
        assert_eq!(result.index_cleanup_status, "completed");
        assert!(record);
    }

    #[test]
    fn enqueues_repair_when_index_cleanup_fails() {
        let mut repository = repository_with_schema();
        let provider = StubEmbeddingProvider::from_dimension(4).expect("provider");
        let mut ingest_index = InMemoryVectorIndex::new(4);
        let mut ingest_pipeline =
            IngestPipeline::new(&mut repository, &provider, &mut ingest_index);
        let ingest = ingest_pipeline
            .ingest(IngestRequest::new("forget me later"))
            .expect("ingest");
        let mut failing_index = DeleteFailingIndex::default();

        let mut forget_pipeline = ForgetPipeline::new(&mut repository, &mut failing_index);
        let result = forget_pipeline
            .forget(ForgetRequest::for_file(ingest.file_id))
            .expect("forget should degrade");

        assert_eq!(result.affected_chunks, ingest.chunk_ids.len());
        assert_eq!(result.index_cleanup_status, "pending");
        assert!(result.repair_task_id.is_some());
    }

    #[test]
    fn rejects_hard_mode_in_mvp() {
        let mut repository = repository_with_schema();
        let mut index = InMemoryVectorIndex::new(4);

        let mut request = ForgetRequest::for_file(1);
        request.mode = ForgetMode::Hard;

        let mut forget_pipeline = ForgetPipeline::new(&mut repository, &mut index);
        let error = forget_pipeline
            .forget(request)
            .expect_err("hard mode should be rejected");

        assert!(error.to_string().contains("only mode=soft"));
    }

    #[derive(Debug, Default)]
    struct DeleteFailingIndex;

    impl VectorIndex for DeleteFailingIndex {
        fn dimension(&self) -> usize {
            4
        }

        fn insert(&mut self, _entries: &[IndexEntry]) -> IndexResult<()> {
            Ok(())
        }

        fn search(&self, _request: &SearchRequest) -> IndexResult<Vec<SearchHit>> {
            Ok(Vec::new())
        }

        fn mark_deleted(&mut self, _namespace: &str, _chunk_ids: &[i64]) -> IndexResult<usize> {
            Err(IndexError::InvalidTopK { top_k: 0 })
        }

        fn rebuild(&mut self, _entries: &[IndexEntry]) -> IndexResult<()> {
            Ok(())
        }
    }
}
