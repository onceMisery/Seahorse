//! Seahorse core crate.

pub mod embedding;
pub mod index;
pub mod jobs;
pub mod pipeline;
pub mod storage;

pub use embedding::{
    EmbeddingError, EmbeddingProvider, EmbeddingResult, StubEmbeddingProvider, StubFailureMode,
};
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
    RebuildRequest, RebuildResult, RebuildScope, RecallError, RecallFilters, RecallPipeline,
    RecallRequest, RecallResponseMetadata, RecallResult, RecallResultItem,
};
pub use storage::{
    apply_sqlite_migrations, read_schema_meta, validate_schema_meta, ChunkTagInsert, ChunkWrite,
    FileWrite, IngestWriteBatch, MaintenanceJob, PersistedChunk, PersistedDeletion, PersistedFile,
    PersistedIngest, PersistedReplacement, RebuildChunkRecord, RecallChunkRecord, RepairTask,
    SchemaExpectation, SchemaMetaSnapshot, SqliteRepository, StatusCount, StorageError,
    StorageResult, StorageStatsSnapshot, TagWrite, LATEST_SCHEMA_VERSION,
};
