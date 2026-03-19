use super::error::IndexError;
use super::types::{IndexEntry, SearchHit, SearchRequest};

pub type IndexResult<T> = Result<T, IndexError>;

pub trait VectorIndex: Send + Sync {
    fn dimension(&self) -> usize;

    fn insert(&mut self, entries: &[IndexEntry]) -> IndexResult<()>;

    fn search(&self, request: &SearchRequest) -> IndexResult<Vec<SearchHit>>;

    fn mark_deleted(&mut self, namespace: &str, chunk_ids: &[i64]) -> IndexResult<usize>;

    fn rebuild(&mut self, entries: &[IndexEntry]) -> IndexResult<()>;
}
