use std::fmt;

use crate::embedding::{EmbeddingError, EmbeddingProvider};
use crate::index::{IndexEntry, IndexError, VectorIndex};
use crate::storage::{SqliteRepository, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildScope {
    All,
    MissingIndex,
}

impl RebuildScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::MissingIndex => "missing_index",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildRequest {
    pub namespace: String,
    pub scope: RebuildScope,
}

impl Default for RebuildRequest {
    fn default() -> Self {
        Self {
            namespace: "default".to_owned(),
            scope: RebuildScope::All,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildResult {
    pub scope: RebuildScope,
    pub scanned_chunks: usize,
    pub indexed_chunks: usize,
    pub index_state: String,
}

#[derive(Debug)]
pub enum RebuildError {
    InvalidInput { message: String },
    Embedding(EmbeddingError),
    Storage(StorageError),
    Index(IndexError),
}

impl fmt::Display for RebuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { message } => write!(f, "invalid rebuild input: {message}"),
            Self::Embedding(source) => write!(f, "rebuild embedding failed: {source}"),
            Self::Storage(source) => write!(f, "rebuild storage failed: {source}"),
            Self::Index(source) => write!(f, "rebuild index failed: {source}"),
        }
    }
}

impl std::error::Error for RebuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Embedding(source) => Some(source),
            Self::Storage(source) => Some(source),
            Self::Index(source) => Some(source),
            Self::InvalidInput { .. } => None,
        }
    }
}

impl From<EmbeddingError> for RebuildError {
    fn from(value: EmbeddingError) -> Self {
        Self::Embedding(value)
    }
}

impl From<StorageError> for RebuildError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<IndexError> for RebuildError {
    fn from(value: IndexError) -> Self {
        Self::Index(value)
    }
}

pub struct RebuildPipeline<'a, P, I>
where
    P: EmbeddingProvider + ?Sized,
    I: VectorIndex + ?Sized,
{
    repository: &'a mut SqliteRepository,
    embedding_provider: &'a P,
    vector_index: &'a mut I,
}

impl<'a, P, I> RebuildPipeline<'a, P, I>
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

    pub fn rebuild(&mut self, request: RebuildRequest) -> Result<RebuildResult, RebuildError> {
        validate_request(&request)?;
        self.repository
            .set_schema_meta_value("index_state", "rebuilding")?;

        let result = match request.scope {
            RebuildScope::All => self.rebuild_all(&request.namespace),
            RebuildScope::MissingIndex => self.repair_missing_index(&request.namespace),
        };

        match result {
            Ok(result) => {
                self.repository.set_schema_meta_value(
                    "embedding_model_id",
                    self.embedding_provider.model_id(),
                )?;
                self.repository.set_schema_meta_value(
                    "embedding_dimension",
                    &self.embedding_provider.dimension().to_string(),
                )?;
                self.repository
                    .set_schema_meta_value("index_state", "ready")?;
                Ok(result)
            }
            Err(error) => {
                let _ = self
                    .repository
                    .set_schema_meta_value("index_state", "degraded");
                Err(error)
            }
        }
    }

    fn rebuild_all(&mut self, namespace: &str) -> Result<RebuildResult, RebuildError> {
        let chunks = self.repository.list_rebuild_chunks(namespace)?;
        let entries = self.build_entries(&chunks)?;
        self.vector_index.rebuild(&entries)?;
        self.repository.rebuild_connectome(namespace)?;

        let chunk_ids = chunks
            .iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        self.repository.mark_chunks_ready(namespace, &chunk_ids)?;
        self.repository.refresh_file_statuses(namespace)?;

        Ok(RebuildResult {
            scope: RebuildScope::All,
            scanned_chunks: chunk_ids.len(),
            indexed_chunks: chunk_ids.len(),
            index_state: "ready".to_owned(),
        })
    }

    fn repair_missing_index(&mut self, namespace: &str) -> Result<RebuildResult, RebuildError> {
        let chunks = self.repository.list_missing_index_chunks(namespace)?;
        let entries = self.build_entries(&chunks)?;
        if !entries.is_empty() {
            self.vector_index.insert(&entries)?;
        }

        let chunk_ids = chunks
            .iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        self.repository.mark_chunks_ready(namespace, &chunk_ids)?;
        self.repository.refresh_file_statuses(namespace)?;

        Ok(RebuildResult {
            scope: RebuildScope::MissingIndex,
            scanned_chunks: chunk_ids.len(),
            indexed_chunks: chunk_ids.len(),
            index_state: "ready".to_owned(),
        })
    }

    fn build_entries(
        &self,
        chunks: &[crate::storage::RebuildChunkRecord],
    ) -> Result<Vec<IndexEntry>, RebuildError> {
        let mut entries = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let vector = self.embedding_provider.embed(&chunk.chunk_text)?;
            entries.push(IndexEntry::new(
                chunk.chunk_id,
                chunk.namespace.clone(),
                vector,
            ));
        }

        Ok(entries)
    }
}

fn validate_request(request: &RebuildRequest) -> Result<(), RebuildError> {
    if request.namespace != "default" {
        return Err(RebuildError::InvalidInput {
            message: "only namespace=default is supported in MVP".to_owned(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{RebuildPipeline, RebuildRequest, RebuildScope};
    use crate::embedding::{EmbeddingProvider, StubEmbeddingProvider};
    use crate::index::{InMemoryVectorIndex, SearchRequest, VectorIndex};
    use crate::storage::{
        apply_sqlite_migrations, ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch,
        SqliteRepository, TagWrite,
    };

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        apply_sqlite_migrations(&connection).expect("apply migration");
        SqliteRepository::new(connection).expect("repository")
    }

    fn write_batch(repository: &mut SqliteRepository) -> crate::storage::PersistedIngest {
        repository
            .write_ingest_batch(&IngestWriteBatch {
                file: FileWrite::new("rebuild.txt", "file-hash-rebuild"),
                chunks: vec![
                    ChunkWrite::new(0, "alpha", "chunk-hash-alpha", "stub-4d", 4),
                    ChunkWrite::new(1, "beta", "chunk-hash-beta", "stub-4d", 4),
                ],
                tags: vec![],
                chunk_tags: vec![],
            })
            .expect("write ingest batch")
    }

    #[test]
    fn rebuild_all_restores_index_and_marks_chunks_ready() {
        let mut repository = repository_with_schema();
        let persisted = write_batch(&mut repository);
        let provider = StubEmbeddingProvider::from_dimension(4).expect("stub provider");
        let mut index = InMemoryVectorIndex::new(provider.dimension());

        let mut pipeline = RebuildPipeline::new(&mut repository, &provider, &mut index);
        let result = pipeline
            .rebuild(RebuildRequest::default())
            .expect("rebuild all");

        assert_eq!(result.scope, RebuildScope::All);
        assert_eq!(result.indexed_chunks, 2);
        assert_eq!(result.scanned_chunks, 2);

        let hits = index
            .search(&SearchRequest::new(
                "default",
                provider.embed("alpha").expect("embed query"),
                10,
            ))
            .expect("search rebuilt index");
        let hit_ids = hits.iter().map(|hit| hit.chunk_id).collect::<Vec<_>>();
        assert!(hit_ids.contains(&persisted.chunks[0].id));
        assert!(hit_ids.contains(&persisted.chunks[1].id));

        let chunks = repository
            .list_chunks_by_file_id(persisted.file.id)
            .expect("load chunks");
        let index_state = repository
            .get_schema_meta_value("index_state")
            .expect("load index_state");

        assert!(chunks.iter().all(|chunk| chunk.index_status == "ready"));
        assert_eq!(index_state.as_deref(), Some("ready"));
    }

    #[test]
    fn missing_index_repairs_failed_chunks_without_dropping_existing_entries() {
        let mut repository = repository_with_schema();
        let persisted = write_batch(&mut repository);
        let provider = StubEmbeddingProvider::from_dimension(4).expect("stub provider");
        let mut index = InMemoryVectorIndex::new(provider.dimension());

        let ready_entry = crate::index::IndexEntry::new(
            persisted.chunks[0].id,
            "default",
            provider.embed("alpha").expect("embed ready chunk"),
        );
        index.insert(&[ready_entry]).expect("seed existing index");
        repository
            .mark_chunks_ready("default", &[persisted.chunks[0].id])
            .expect("mark first chunk ready");
        repository
            .update_indexing_result(
                persisted.file.id,
                &[persisted.chunks[1].id],
                "partial",
                "failed",
            )
            .expect("mark second chunk failed");

        let mut pipeline = RebuildPipeline::new(&mut repository, &provider, &mut index);
        let result = pipeline
            .rebuild(RebuildRequest {
                namespace: "default".to_owned(),
                scope: RebuildScope::MissingIndex,
            })
            .expect("repair missing index entries");

        assert_eq!(result.scope, RebuildScope::MissingIndex);
        assert_eq!(result.indexed_chunks, 1);

        let hits = index
            .search(&SearchRequest::new(
                "default",
                provider.embed("beta").expect("embed query"),
                10,
            ))
            .expect("search repaired index");
        let hit_ids = hits.iter().map(|hit| hit.chunk_id).collect::<Vec<_>>();
        assert!(hit_ids.contains(&persisted.chunks[0].id));
        assert!(hit_ids.contains(&persisted.chunks[1].id));

        let file = repository
            .find_file_by_hash("default", "file-hash-rebuild")
            .expect("find file")
            .expect("file exists");
        let repaired_chunk = repository
            .list_chunks_by_file_id(persisted.file.id)
            .expect("load chunks")
            .into_iter()
            .find(|chunk| chunk.id == persisted.chunks[1].id)
            .expect("repaired chunk exists");

        assert_eq!(file.ingest_status, "ready");
        assert_eq!(repaired_chunk.index_status, "ready");
    }

    #[test]
    fn rebuild_all_restores_connectome_from_active_chunks() {
        let mut repository = repository_with_schema();
        repository
            .write_ingest_batch(&IngestWriteBatch {
                file: FileWrite::new("first.txt", "hash-rebuild-connectome-first"),
                chunks: vec![ChunkWrite::new(
                    0,
                    "project rust rebuild",
                    "chunk-rebuild-connectome-first",
                    "stub-4d",
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
            .expect("write first connectome file");
        let second = repository
            .write_ingest_batch(&IngestWriteBatch {
                file: FileWrite::new("second.txt", "hash-rebuild-connectome-second"),
                chunks: vec![ChunkWrite::new(
                    0,
                    "project memory rebuild",
                    "chunk-rebuild-connectome-second",
                    "stub-4d",
                    4,
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
            .expect("write second connectome file");

        repository
            .soft_delete_files("default", &[second.file.id])
            .expect("soft delete second file");

        let provider = StubEmbeddingProvider::from_dimension(4).expect("stub provider");
        let mut index = InMemoryVectorIndex::new(provider.dimension());
        let mut pipeline = RebuildPipeline::new(&mut repository, &provider, &mut index);
        pipeline
            .rebuild(RebuildRequest::default())
            .expect("rebuild all should restore connectome");

        let neighbors = repository
            .list_connectome_neighbors("default", "project", 10)
            .expect("list rebuilt connectome neighbors");

        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].target_tag, "rust");
    }
}
