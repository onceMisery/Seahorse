use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde_json::json;
use seahorse_core::{
    EmbeddingProvider, InMemoryVectorIndex, IndexEntry, IngestError, IngestPipeline,
    IngestRequest as CoreIngestRequest, IngestResult as CoreIngestResult, ForgetError,
    ForgetPipeline, ForgetRequest as CoreForgetRequest, ForgetResult as CoreForgetResult,
    MaintenanceJob, RecallError, RecallPipeline, RecallRequest as CoreRecallRequest,
    RecallResult as CoreRecallResult, RebuildError, RebuildPipeline,
    RebuildRequest as CoreRebuildRequest, SqliteRepository, StorageError,
    StubEmbeddingProvider, VectorIndex, apply_sqlite_migrations,
};

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

#[derive(Debug)]
struct BootstrapChunk {
    chunk_id: i64,
    namespace: String,
    chunk_text: String,
}

#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub status: String,
    pub db: String,
    pub index: String,
    pub embedding_provider: String,
}

#[derive(Debug, Clone)]
pub struct StatsSnapshot {
    pub chunk_count: usize,
    pub tag_count: usize,
    pub deleted_chunk_count: usize,
    pub repair_queue_size: usize,
    pub index_status: String,
}

#[derive(Debug)]
pub enum AppStateError {
    Unavailable { message: &'static str },
    Ingest(IngestError),
    Forget(ForgetError),
    Recall(RecallError),
    Rebuild(RebuildError),
    Storage(StorageError),
    NotFound { message: String },
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let db_path = std::env::var("SEAHORSE_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_owned());
        let embedding_provider = StubEmbeddingProvider::from_dimension(DEFAULT_EMBEDDING_DIMENSION)
            .map_err(|error| format!("failed to initialize embedding provider: {error}"))?;
        let (mut repository, bootstrap_chunks) = open_repository(&db_path)?;
        let mut vector_index = InMemoryVectorIndex::new(embedding_provider.dimension());
        let bootstrap_entries = build_bootstrap_entries(&embedding_provider, &bootstrap_chunks)?;
        vector_index
            .insert(&bootstrap_entries)
            .map_err(|error| format!("failed to warm in-memory index from sqlite: {error}"))?;
        let initial_index_state = if repository
            .list_missing_index_chunks("default")
            .map_err(|error| format!("failed to inspect pending rebuild chunks: {error}"))?
            .is_empty()
        {
            "ready"
        } else {
            "degraded"
        };
        sync_runtime_schema_meta(&mut repository, &embedding_provider, initial_index_state)?;
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
        let mut result = pipeline.recall(request).map_err(AppStateError::Recall)?;
        let index_state = current_index_state(&services.repository)?;
        apply_runtime_index_state(&mut result, index_state);
        Ok(result)
    }

    pub fn forget(&self, request: CoreForgetRequest) -> Result<CoreForgetResult, AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let mut pipeline = ForgetPipeline::new(&mut services.repository, &mut services.vector_index);

        pipeline.forget(request).map_err(AppStateError::Forget)
    }

    pub fn rebuild(
        &self,
        request: CoreRebuildRequest,
        force: bool,
    ) -> Result<MaintenanceJob, AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;

        if let Some(active_job) = services
            .repository
            .find_active_maintenance_job("rebuild", &request.namespace)
            .map_err(AppStateError::Storage)?
        {
            if !force {
                return Ok(active_job);
            }

            services
                .repository
                .cancel_active_maintenance_jobs(
                    "rebuild",
                    &request.namespace,
                    "superseded by force rebuild request",
                )
                .map_err(AppStateError::Storage)?;
        }

        let payload_json = json!({
            "scope": request.scope.as_str(),
            "force": force,
        })
        .to_string();
        let job = services
            .repository
            .create_maintenance_job("rebuild", &request.namespace, Some(&payload_json))
            .map_err(AppStateError::Storage)?;
        services
            .repository
            .mark_maintenance_job_running(job.id, Some("running"))
            .map_err(AppStateError::Storage)?;

        let provider = services.embedding_provider.clone();
        let rebuild_result = {
            let mut pipeline = RebuildPipeline::new(
                &mut services.repository,
                &provider,
                &mut services.vector_index,
            );
            pipeline.rebuild(request)
        };

        match rebuild_result {
            Ok(result) => {
                let progress = format!("{}/{}", result.indexed_chunks, result.scanned_chunks);
                let result_summary = format!(
                    "scope={}, indexed_chunks={}, scanned_chunks={}",
                    result.scope.as_str(),
                    result.indexed_chunks,
                    result.scanned_chunks
                );
                services
                    .repository
                    .finish_maintenance_job(
                        job.id,
                        "succeeded",
                        Some(&progress),
                        Some(&result_summary),
                        None,
                    )
                    .map_err(AppStateError::Storage)?;
                load_job(&services.repository, job.id)
            }
            Err(error) => {
                let error_message = error.to_string();
                services
                    .repository
                    .finish_maintenance_job(job.id, "failed", None, None, Some(&error_message))
                    .map_err(AppStateError::Storage)?;
                Err(AppStateError::Rebuild(error))
            }
        }
    }

    pub fn get_job(&self, job_id: i64) -> Result<MaintenanceJob, AppStateError> {
        let services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;

        load_job(&services.repository, job_id)
    }

    pub fn health_snapshot(&self) -> Result<HealthSnapshot, AppStateError> {
        let services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let index_state = current_index_state(&services.repository)?;

        Ok(HealthSnapshot {
            status: health_status_from_index_state(&index_state).to_owned(),
            db: services.db_label.clone(),
            index: format!("memory-{}d", services.vector_index.dimension()),
            embedding_provider: services.embedding_provider.model_id().to_owned(),
        })
    }

    pub fn stats_snapshot(&self) -> Result<StatsSnapshot, AppStateError> {
        let services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let stats = services
            .repository
            .load_stats("default")
            .map_err(AppStateError::Storage)?;

        Ok(StatsSnapshot {
            chunk_count: stats.chunk_count,
            tag_count: stats.tag_count,
            deleted_chunk_count: stats.deleted_chunk_count,
            repair_queue_size: stats.repair_queue_size,
            index_status: stats.index_status,
        })
    }
}

fn open_repository(path: &str) -> Result<(SqliteRepository, Vec<BootstrapChunk>), String> {
    let connection = if path == ":memory:" {
        Connection::open_in_memory()
            .map_err(|error| format!("failed to open sqlite memory database: {error}"))?
    } else {
        ensure_parent_dir(path)?;
        Connection::open(path).map_err(|error| format!("failed to open sqlite database: {error}"))?
    };

    apply_sqlite_migrations(&connection)
        .map_err(|error| format!("failed to apply sqlite migration: {error}"))?;
    let bootstrap_chunks = load_bootstrap_chunks(&connection)?;
    let repository = SqliteRepository::new(connection)
        .map_err(|error| format!("failed to initialize repository: {error}"))?;

    Ok((repository, bootstrap_chunks))
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

fn load_bootstrap_chunks(connection: &Connection) -> Result<Vec<BootstrapChunk>, String> {
    let mut statement = connection
        .prepare(
            "SELECT c.id, c.namespace, c.chunk_text
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE c.is_deleted = 0
               AND c.index_status = 'ready'
               AND f.ingest_status != 'deleted'
             ORDER BY c.id ASC",
        )
        .map_err(|error| format!("failed to prepare bootstrap chunk query: {error}"))?;
    let rows = statement
        .query_map(params![], |row| {
            Ok(BootstrapChunk {
                chunk_id: row.get(0)?,
                namespace: row.get(1)?,
                chunk_text: row.get(2)?,
            })
        })
        .map_err(|error| format!("failed to query bootstrap chunks: {error}"))?;

    let mut chunks = Vec::new();
    for row in rows {
        chunks.push(row.map_err(|error| format!("failed to read bootstrap chunk row: {error}"))?);
    }

    Ok(chunks)
}

fn build_bootstrap_entries(
    embedding_provider: &StubEmbeddingProvider,
    chunks: &[BootstrapChunk],
) -> Result<Vec<IndexEntry>, String> {
    let mut entries = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let vector = embedding_provider
            .embed(&chunk.chunk_text)
            .map_err(|error| format!("failed to embed bootstrap chunk {}: {error}", chunk.chunk_id))?;
        entries.push(IndexEntry::new(
            chunk.chunk_id,
            chunk.namespace.clone(),
            vector,
        ));
    }

    Ok(entries)
}

fn sync_runtime_schema_meta(
    repository: &mut SqliteRepository,
    embedding_provider: &StubEmbeddingProvider,
    index_state: &str,
) -> Result<(), String> {
    repository
        .set_schema_meta_value("embedding_model_id", embedding_provider.model_id())
        .map_err(|error| format!("failed to persist embedding_model_id: {error}"))?;
    repository
        .set_schema_meta_value(
            "embedding_dimension",
            &embedding_provider.dimension().to_string(),
        )
        .map_err(|error| format!("failed to persist embedding_dimension: {error}"))?;
    repository
        .set_schema_meta_value("index_state", index_state)
        .map_err(|error| format!("failed to persist index_state: {error}"))?;

    Ok(())
}

fn current_index_state(repository: &SqliteRepository) -> Result<String, AppStateError> {
    Ok(repository
        .get_schema_meta_value("index_state")
        .map_err(AppStateError::Storage)?
        .unwrap_or_else(|| "ready".to_owned()))
}

fn apply_runtime_index_state(result: &mut CoreRecallResult, index_state: String) {
    result.metadata.index_state = index_state.clone();
    if index_state == "ready" {
        result.metadata.degraded = false;
        result.metadata.degraded_reason = None;
        return;
    }

    result.metadata.degraded = true;
    result.metadata.degraded_reason = Some(format!("index_state={index_state}"));
}

fn health_status_from_index_state(index_state: &str) -> &'static str {
    match index_state {
        "ready" => "ok",
        "rebuilding" | "degraded" => "degraded",
        "unavailable" => "failed",
        _ => "degraded",
    }
}

fn load_job(repository: &SqliteRepository, job_id: i64) -> Result<MaintenanceJob, AppStateError> {
    repository
        .get_maintenance_job(job_id)
        .map_err(AppStateError::Storage)?
        .ok_or(AppStateError::NotFound {
            message: format!("job_id {job_id} not found"),
        })
}
