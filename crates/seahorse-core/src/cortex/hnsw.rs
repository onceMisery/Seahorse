use std::collections::BTreeMap;

use crate::index::{
    InMemoryVectorIndex, IndexEntry, IndexResult, SearchHit, SearchRequest, VectorIndex,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapHnswConfig {
    pub dimension: usize,
}

impl BootstrapHnswConfig {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension: dimension.max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapHnswEntry {
    pub chunk_id: i64,
    pub namespace: String,
    pub vector: Vec<f32>,
}

#[derive(Debug)]
pub struct BootstrapHnswIndex {
    config: BootstrapHnswConfig,
    index: InMemoryVectorIndex,
    entries: BTreeMap<i64, BootstrapHnswEntry>,
}

impl BootstrapHnswIndex {
    pub fn new(config: BootstrapHnswConfig) -> Self {
        Self {
            index: InMemoryVectorIndex::new(config.dimension),
            config,
            entries: BTreeMap::new(),
        }
    }

    pub fn dimension(&self) -> usize {
        self.config.dimension
    }

    pub fn insert(&mut self, entries: &[IndexEntry]) -> IndexResult<()> {
        self.index.insert(entries)?;

        for entry in entries {
            self.entries.insert(
                entry.chunk_id,
                BootstrapHnswEntry {
                    chunk_id: entry.chunk_id,
                    namespace: entry.namespace.clone(),
                    vector: entry.vector.clone(),
                },
            );
        }

        Ok(())
    }

    pub fn search(&self, request: &SearchRequest) -> IndexResult<Vec<SearchHit>> {
        self.index.search(request)
    }

    pub fn snapshot_entries(&self) -> Vec<BootstrapHnswEntry> {
        self.entries.values().cloned().collect()
    }

    pub fn rebuild_from_snapshot(&mut self, entries: &[BootstrapHnswEntry]) -> IndexResult<()> {
        let index_entries = entries
            .iter()
            .map(|entry| IndexEntry::new(entry.chunk_id, &entry.namespace, entry.vector.clone()))
            .collect::<Vec<_>>();
        self.index.rebuild(&index_entries)?;
        self.entries = entries
            .iter()
            .map(|entry| (entry.chunk_id, entry.clone()))
            .collect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BootstrapHnswConfig, BootstrapHnswIndex};
    use crate::index::{IndexEntry, SearchRequest};

    #[test]
    fn searches_inserted_vectors_through_cortex_hnsw() {
        let mut index = BootstrapHnswIndex::new(BootstrapHnswConfig::new(3));
        index
            .insert(&[
                IndexEntry::new(1, "default", vec![1.0, 0.0, 0.0]),
                IndexEntry::new(2, "default", vec![0.0, 1.0, 0.0]),
            ])
            .expect("insert bootstrap hnsw entries");

        let hits = index
            .search(&SearchRequest::new("default", vec![1.0, 0.0, 0.0], 2))
            .expect("search bootstrap hnsw entries");

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, 1);
    }
}
