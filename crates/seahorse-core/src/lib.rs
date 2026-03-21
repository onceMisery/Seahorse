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
    IndexEntry, IndexError, IndexResult, InMemoryVectorIndex, SearchHit, SearchRequest,
    VectorIndex,
};
pub use jobs::{
    NoopRepairTaskExecutor, RepairTaskExecutor, RepairWorker, RepairWorkerConfig,
    RepairWorkerError, RepairWorkerRunResult,
};
pub use pipeline::chunker::{chunk_text, Chunk, ChunkerConfig};
pub use pipeline::hashing::{fnv1a_hash, stable_content_hash};
pub use pipeline::{
    DedupMode, IngestError, IngestOptions, IngestPipeline, IngestRequest, IngestResult,
    ForgetError, ForgetMode, ForgetPipeline, ForgetRequest, ForgetResult, RecallError,
    RecallFilters, RecallPipeline, RecallRequest, RecallResponseMetadata, RecallResult,
    RecallResultItem, RebuildError, RebuildPipeline, RebuildRequest, RebuildResult,
    RebuildScope,
};
pub use pipeline::preprocessor::normalize_text;
pub use storage::{
    apply_sqlite_migrations,
    ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, PersistedChunk, PersistedFile,
    MaintenanceJob, PersistedDeletion, PersistedIngest, PersistedReplacement,
    RecallChunkRecord, RebuildChunkRecord, RepairTask, SchemaExpectation, SchemaMetaSnapshot,
    SqliteRepository, StorageError, StorageResult, StorageStatsSnapshot, TagWrite, LATEST_SCHEMA_VERSION,
    read_schema_meta,
    validate_schema_meta,
};
