//! Seahorse core crate.

pub mod embedding;
pub mod pipeline;
pub mod storage;

pub use embedding::{EmbeddingError, EmbeddingProvider, EmbeddingResult};
pub use pipeline::chunker::{chunk_text, Chunk, ChunkerConfig};
pub use pipeline::hashing::{fnv1a_hash, stable_content_hash};
pub use pipeline::preprocessor::normalize_text;
pub use storage::{
    ChunkTagInsert, ChunkWrite, FileWrite, IngestWriteBatch, PersistedChunk, PersistedFile,
    PersistedIngest, SchemaExpectation, SchemaMetaSnapshot, SqliteRepository, StorageError,
    StorageResult, TagWrite, read_schema_meta, validate_schema_meta,
};
