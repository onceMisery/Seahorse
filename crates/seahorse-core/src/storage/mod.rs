pub mod error;
pub mod migrations;
pub mod models;
pub mod repository;
pub mod schema;

pub use error::{StorageError, StorageResult};
pub use migrations::{apply_sqlite_migrations, LATEST_SCHEMA_VERSION};
pub use models::{
    ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, PersistedChunk, PersistedFile,
    MaintenanceJob, PersistedDeletion, PersistedIngest, PersistedReplacement,
    RecallChunkRecord, RebuildChunkRecord, StorageStatsSnapshot, TagWrite,
};
pub use repository::SqliteRepository;
pub use schema::{
    read_schema_meta, validate_schema_meta, SchemaExpectation, SchemaMetaSnapshot,
};
