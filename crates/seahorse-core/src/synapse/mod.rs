#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynapseConfig {
    pub max_signals: usize,
    pub neighbor_limit: usize,
}

impl Default for SynapseConfig {
    fn default() -> Self {
        Self {
            max_signals: 128,
            neighbor_limit: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SynapticSignal {
    pub namespace: String,
    pub tag: String,
    pub potential: f32,
}

#[derive(Debug)]
pub struct Synapse {
    config: SynapseConfig,
    signals: Vec<SynapticSignal>,
}

impl Synapse {
    pub fn new(config: SynapseConfig) -> Self {
        Self {
            config,
            signals: Vec::new(),
        }
    }

    pub fn prime(&mut self, namespace: &str, tag: &str, potential: f32) {
        self.record_signal(namespace, tag, potential);
    }

    pub fn signals(&self) -> &[SynapticSignal] {
        &self.signals
    }

    pub fn activate_connectome_neighbors(
        &mut self,
        repository: &crate::storage::SqliteRepository,
        namespace: &str,
        seed_tag: &str,
        potential: f32,
    ) -> crate::storage::StorageResult<&[SynapticSignal]> {
        self.record_signal(namespace, seed_tag, potential);

        let neighbors = repository.list_connectome_neighbors(
            namespace,
            seed_tag,
            self.config.neighbor_limit,
        )?;
        let max_weight = neighbors
            .iter()
            .map(|edge| edge.weight)
            .fold(0.0_f64, f64::max)
            .max(1.0);

        for edge in neighbors {
            let scaled = potential * (edge.weight / max_weight) as f32;
            self.record_signal(namespace, &edge.target_tag, scaled);
        }

        Ok(self.signals())
    }

    fn record_signal(&mut self, namespace: &str, tag: &str, potential: f32) {
        if let Some(existing) = self
            .signals
            .iter_mut()
            .find(|signal| signal.namespace == namespace && signal.tag == tag)
        {
            existing.potential = existing.potential.max(potential);
            return;
        }

        if self.signals.len() >= self.config.max_signals {
            return;
        }

        self.signals.push(SynapticSignal {
            namespace: namespace.to_owned(),
            tag: tag.to_owned(),
            potential,
        });
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{Synapse, SynapseConfig};
    use crate::storage::models::{
        ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, TagWrite,
    };
    use crate::storage::{apply_sqlite_migrations, SqliteRepository};

    fn repository_with_schema() -> SqliteRepository {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        apply_sqlite_migrations(&connection).expect("apply migration");
        SqliteRepository::new(connection).expect("repository")
    }

    #[test]
    fn activates_connectome_neighbors_from_repository() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("synapse.txt", "hash-synapse"),
            chunks: vec![ChunkWrite::new(
                0,
                "project rust memory",
                "chunk-synapse",
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
            .expect("write ingest batch");

        let mut synapse = Synapse::new(SynapseConfig::default());
        let signals = synapse
            .activate_connectome_neighbors(&repository, "default", "project", 1.0)
            .expect("activate connectome neighbors");

        assert_eq!(signals.len(), 3);
        assert!(signals.iter().any(|signal| signal.tag == "project"));
        assert!(signals.iter().any(|signal| signal.tag == "rust"));
        assert!(signals.iter().any(|signal| signal.tag == "memory"));
    }

    #[test]
    fn respects_signal_capacity_when_loading_neighbors() {
        let mut repository = repository_with_schema();
        let batch = IngestWriteBatch {
            file: FileWrite::new("synapse-cap.txt", "hash-synapse-cap"),
            chunks: vec![ChunkWrite::new(
                0,
                "project rust memory graph",
                "chunk-synapse-cap",
                "test-model",
                3,
            )],
            tags: vec![
                TagWrite::new("Project", "project"),
                TagWrite::new("Rust", "rust"),
                TagWrite::new("Memory", "memory"),
                TagWrite::new("Graph", "graph"),
            ],
            chunk_tags: vec![
                ChunkTagInsert::new(0, "project"),
                ChunkTagInsert::new(0, "rust"),
                ChunkTagInsert::new(0, "memory"),
                ChunkTagInsert::new(0, "graph"),
            ],
        };
        repository
            .write_ingest_batch(&batch)
            .expect("write ingest batch");

        let mut synapse = Synapse::new(SynapseConfig {
            max_signals: 2,
            neighbor_limit: 8,
        });
        let signals = synapse
            .activate_connectome_neighbors(&repository, "default", "project", 1.0)
            .expect("activate connectome neighbors");

        assert_eq!(signals.len(), 2);
    }
}
