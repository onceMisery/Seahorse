use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use super::error::IndexError;
use super::traits::{IndexResult, VectorIndex};
use super::types::{IndexEntry, SearchHit, SearchRequest};

#[derive(Debug, Clone)]
struct StoredEntry {
    namespace: String,
    vector: Vec<f32>,
    deleted: bool,
}

#[derive(Debug, Default)]
pub struct InMemoryVectorIndex {
    dimension: usize,
    entries: HashMap<i64, StoredEntry>,
}

impl InMemoryVectorIndex {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            entries: HashMap::new(),
        }
    }

    fn validate_vector(&self, vector: &[f32]) -> IndexResult<()> {
        if vector.len() != self.dimension {
            return Err(IndexError::DimensionMismatch {
                expected: self.dimension,
                actual: vector.len(),
            });
        }

        Ok(())
    }
}

impl VectorIndex for InMemoryVectorIndex {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn insert(&mut self, entries: &[IndexEntry]) -> IndexResult<()> {
        for entry in entries {
            self.validate_vector(&entry.vector)?;
            self.entries.insert(
                entry.chunk_id,
                StoredEntry {
                    namespace: entry.namespace.clone(),
                    vector: entry.vector.clone(),
                    deleted: false,
                },
            );
        }

        Ok(())
    }

    fn search(&self, request: &SearchRequest) -> IndexResult<Vec<SearchHit>> {
        self.validate_vector(&request.query_vector)?;

        if request.top_k == 0 {
            return Err(IndexError::InvalidTopK { top_k: request.top_k });
        }

        let mut hits = self
            .entries
            .iter()
            .filter(|(_, entry)| !entry.deleted && entry.namespace == request.namespace)
            .map(|(chunk_id, entry)| SearchHit {
                chunk_id: *chunk_id,
                namespace: entry.namespace.clone(),
                score: cosine_similarity(&request.query_vector, &entry.vector),
            })
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        hits.truncate(request.top_k);

        Ok(hits)
    }

    fn mark_deleted(&mut self, namespace: &str, chunk_ids: &[i64]) -> IndexResult<usize> {
        let to_delete = chunk_ids.iter().copied().collect::<HashSet<_>>();
        let mut affected = 0;

        for (chunk_id, entry) in &mut self.entries {
            if entry.namespace == namespace && to_delete.contains(chunk_id) && !entry.deleted {
                entry.deleted = true;
                affected += 1;
            }
        }

        Ok(affected)
    }

    fn rebuild(&mut self, entries: &[IndexEntry]) -> IndexResult<()> {
        let mut rebuilt = HashMap::with_capacity(entries.len());

        for entry in entries {
            self.validate_vector(&entry.vector)?;
            rebuilt.insert(
                entry.chunk_id,
                StoredEntry {
                    namespace: entry.namespace.clone(),
                    vector: entry.vector.clone(),
                    deleted: false,
                },
            );
        }

        self.entries = rebuilt;
        Ok(())
    }
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;

    for (l, r) in left.iter().zip(right.iter()) {
        dot += l * r;
        left_norm += l * l;
        right_norm += r * r;
    }

    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }

    dot / (left_norm.sqrt() * right_norm.sqrt())
}

#[cfg(test)]
mod tests {
    use super::InMemoryVectorIndex;
    use crate::index::{IndexEntry, SearchRequest, VectorIndex};

    #[test]
    fn returns_hits_in_descending_score_order() {
        let mut index = InMemoryVectorIndex::new(3);
        index
            .insert(&[
                IndexEntry::new(1, "default", vec![1.0, 0.0, 0.0]),
                IndexEntry::new(2, "default", vec![0.0, 1.0, 0.0]),
            ])
            .expect("insert entries");

        let hits = index
            .search(&SearchRequest::new("default", vec![1.0, 0.0, 0.0], 2))
            .expect("search");

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, 1);
    }

    #[test]
    fn hides_deleted_entries() {
        let mut index = InMemoryVectorIndex::new(2);
        index
            .insert(&[
                IndexEntry::new(10, "default", vec![1.0, 0.0]),
                IndexEntry::new(11, "default", vec![0.8, 0.2]),
            ])
            .expect("insert entries");

        let affected = index
            .mark_deleted("default", &[10])
            .expect("mark deleted");
        assert_eq!(affected, 1);

        let hits = index
            .search(&SearchRequest::new("default", vec![1.0, 0.0], 5))
            .expect("search");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, 11);
    }

    #[test]
    fn rebuild_replaces_existing_index_state() {
        let mut index = InMemoryVectorIndex::new(2);
        index
            .insert(&[IndexEntry::new(1, "default", vec![1.0, 0.0])])
            .expect("insert entry");

        index
            .rebuild(&[IndexEntry::new(2, "default", vec![0.0, 1.0])])
            .expect("rebuild");

        let hits = index
            .search(&SearchRequest::new("default", vec![0.0, 1.0], 5))
            .expect("search");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, 2);
    }
}
