use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use seahorse_core::{
    EmbeddingProvider, InMemoryVectorIndex, IngestError, IngestPipeline,
    IngestRequest as CoreIngestRequest, IngestResult as CoreIngestResult, RecallError,
    RecallPipeline, RecallRequest as CoreRecallRequest, RecallResult as CoreRecallResult,
    SqliteRepository, StubEmbeddingProvider, VectorIndex,
};

const MIGRATION: &str = include_str!("../../../../migrations/0001_init.sql");
const DEFAULT_DB_PATH: &str = "./data/seahorse.db";
const DEFAULT_EMBEDDING_DIMENSION: usize = 1024;

#[derive(Debug, Clone)]
pub struct AppState {
    services: Arc<Mutex<AppServices>>,
}

#[derive(Debug)]
struct AppServices {
    repository: SqliteRepository,
    embedding_provider: StubEmbeddingProvider,
    vector_index: InMemoryVectorIndex,
    db_label: String,
}

#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub status: String,
    pub db: String,
    pub index: String,
    pub embedding_provider: String,
}

#[derive(Debug)]
pub enum AppStateError {
    Unavailable { message: &'static str },
    Ingest(IngestError),
    Recall(RecallError),
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let db_path = std::env::var("SEAHORSE_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_owned());
        let repository = open_repository(&db_path)?;
        let embedding_provider = StubEmbeddingProvider::from_dimension(DEFAULT_EMBEDDING_DIMENSION)
            .map_err(|error| format!("failed to initialize embedding provider: {error}"))?;
        let vector_index = InMemoryVectorIndex::new(embedding_provider.dimension());
        let db_label = if db_path == ":memory:" {
            "sqlite-memory".to_owned()
        } else {
            "sqlite".to_owned()
        };

        Ok(Self {
            services: Arc::new(Mutex::new(AppServices {
                repository,
                embedding_provider,
                vector_index,
                db_label,
            })),
        })
    }

    pub fn ingest(&self, request: CoreIngestRequest) -> Result<CoreIngestResult, AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let provider = services.embedding_provider.clone();
        let mut pipeline =
            IngestPipeline::new(&mut services.repository, &provider, &mut services.vector_index);

        pipeline.ingest(request).map_err(AppStateError::Ingest)
    }

    pub fn recall(&self, request: CoreRecallRequest) -> Result<CoreRecallResult, AppStateError> {
        let services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let provider = services.embedding_provider.clone();
        let pipeline = RecallPipeline::new(&services.repository, &provider, &services.vector_index);

        pipeline.recall(request).map_err(AppStateError::Recall)
    }

    pub fn health_snapshot(&self) -> Result<HealthSnapshot, AppStateError> {
        let services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;

        Ok(HealthSnapshot {
            status: "ok".to_owned(),
            db: services.db_label.clone(),
            index: format!("memory-{}d", services.vector_index.dimension()),
            embedding_provider: services.embedding_provider.model_id().to_owned(),
        })
    }
}

fn open_repository(path: &str) -> Result<SqliteRepository, String> {
    let connection = if path == ":memory:" {
        Connection::open_in_memory()
            .map_err(|error| format!("failed to open sqlite memory database: {error}"))?
    } else {
        ensure_parent_dir(path)?;
        Connection::open(path).map_err(|error| format!("failed to open sqlite database: {error}"))?
    };

    connection
        .execute_batch(MIGRATION)
        .map_err(|error| format!("failed to apply sqlite migration: {error}"))?;
    SqliteRepository::new(connection)
        .map_err(|error| format!("failed to initialize repository: {error}"))
}

fn ensure_parent_dir(path: &str) -> Result<(), String> {
    let Some(parent) = Path::new(path).parent() else {
        return Ok(());
    };

    if parent.as_os_str().is_empty() {
        return Ok(());
    }

    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create database directory {}: {error}", parent.display()))
}
