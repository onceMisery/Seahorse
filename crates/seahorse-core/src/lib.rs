//! Seahorse core crate.

pub mod cerebellum;
pub mod cortex;
pub mod embedding;
pub mod engine;
pub mod hippocampus;
pub mod index;
pub mod jobs;
pub mod pipeline;
pub mod storage;
pub mod synapse;
pub mod thalamus;

pub use cerebellum::{Cerebellum, CerebellumConfig, ScheduledTask};
pub use cortex::archive::CortexArchiveError;
pub use cortex::archive::{CortexArchiveHeader, CortexArchiveSnapshot};
pub use cortex::hnsw::{BootstrapHnswConfig, BootstrapHnswEntry, BootstrapHnswIndex};
pub use cortex::{Cortex, CortexConfig};
pub use embedding::{
    EmbeddingError, EmbeddingProvider, EmbeddingResult, StubEmbeddingProvider, StubFailureMode,
};
pub use engine::{SeahorseEngine, SeahorseEngineConfig};
pub use hippocampus::Hippocampus;
pub use index::{
    InMemoryVectorIndex, IndexEntry, IndexError, IndexResult, SearchHit, SearchRequest, VectorIndex,
};
pub use jobs::{
    NoopRepairTaskExecutor, RepairTaskExecutor, RepairWorker, RepairWorkerConfig,
    RepairWorkerError, RepairWorkerRunResult,
};
pub use pipeline::chunker::{chunk_text, Chunk, ChunkerConfig};
pub use pipeline::hashing::{fnv1a_hash, stable_content_hash};
pub use pipeline::preprocessor::normalize_text;
pub use pipeline::{
    DedupMode, ForgetError, ForgetMode, ForgetPipeline, ForgetRequest, ForgetResult, IngestError,
    IngestOptions, IngestPipeline, IngestRequest, IngestResult, RebuildError, RebuildPipeline,
    RebuildRequest, RebuildResult, RebuildScope, RecallError, RecallFilters, RecallMode,
    RecallPipeline, RecallRequest, RecallResponseMetadata, RecallResult, RecallResultItem,
};
pub use storage::{
    apply_sqlite_migrations, read_schema_meta, validate_schema_meta, CachedEmbedding,
    ChunkTagInsert, ChunkWrite, ConnectomeHealthSnapshot, FileWrite, IngestWriteBatch,
    MaintenanceJob, PersistedChunk, PersistedDeletion, PersistedFile, PersistedIngest,
    PersistedReplacement, RebuildChunkRecord, RecallChunkRecord, RepairTask, RetrievalLogRecord,
    RetrievalLogWrite, SchemaExpectation, SchemaMetaSnapshot, SqliteRepository, StatusCount,
    StorageError, StorageResult, StorageStatsSnapshot, TagWrite, LATEST_SCHEMA_VERSION,
};
pub use synapse::{Synapse, SynapseConfig, SynapticSignal};
pub use thalamus::{ThalamicAnalysis, Thalamus, ThalamusConfig};

#[cfg(test)]
mod design_all_phase1_tests {
    use crate::cerebellum::{Cerebellum, CerebellumConfig, ScheduledTask};
    use crate::cortex::{Cortex, CortexConfig};
    use crate::engine::{SeahorseEngine, SeahorseEngineConfig};
    use crate::hippocampus::Hippocampus;
    use crate::synapse::{Synapse, SynapseConfig};
    use crate::thalamus::{ThalamicAnalysis, Thalamus, ThalamusConfig};

    #[test]
    fn engine_facade_exposes_design_all_phase1_modules() {
        let cortex = Cortex::new(CortexConfig::new(3));

        let mut synapse = Synapse::new(SynapseConfig::default());
        synapse.prime("default", "rust", 0.9);
        assert_eq!(synapse.signals().len(), 1);

        let thalamus = Thalamus::new(ThalamusConfig::default());
        let analysis = thalamus.analyze("phase1 architecture", 1);
        assert_eq!(
            analysis,
            ThalamicAnalysis::open("default", analysis.entropy)
        );

        let hippocampus = Hippocampus::open_in_memory().expect("open in-memory hippocampus");

        let mut cerebellum = Cerebellum::new(CerebellumConfig::default());
        cerebellum.schedule(ScheduledTask::new("maintenance", "default"));
        assert_eq!(cerebellum.pending_tasks().len(), 1);

        let engine = SeahorseEngine::from_parts(
            SeahorseEngineConfig::default(),
            cortex,
            synapse,
            thalamus,
            hippocampus,
            cerebellum,
        );

        assert_eq!(engine.config.default_namespace, "default");
        assert_eq!(engine.cortex.config().dimension, 3);
    }
}
