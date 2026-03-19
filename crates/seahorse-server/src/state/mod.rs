use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::{params, Connection};
use serde_json::json;
use seahorse_core::{
    EmbeddingProvider, InMemoryVectorIndex, IndexEntry, IngestError, IngestPipeline,
    IngestRequest as CoreIngestRequest, IngestResult as CoreIngestResult, ForgetError,
    ForgetPipeline, ForgetRequest as CoreForgetRequest, ForgetResult as CoreForgetResult,
    MaintenanceJob, RecallError, RecallPipeline, RecallRequest as CoreRecallRequest,
    RecallResult as CoreRecallResult, RebuildChunkRecord, RebuildError,
    RebuildRequest as CoreRebuildRequest, RebuildScope, SqliteRepository, StorageError,
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

#[derive(Debug)]
struct PreparedRebuildWork {
    namespace: String,
    scope: RebuildScope,
    provider: StubEmbeddingProvider,
    chunks: Vec<RebuildChunkRecord>,
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
        Self::new_with_db_path(&db_path)
    }

    pub(crate) fn new_with_db_path(db_path: &str) -> Result<Self, String> {
        let embedding_provider = StubEmbeddingProvider::from_dimension(DEFAULT_EMBEDDING_DIMENSION)
            .map_err(|error| format!("failed to initialize embedding provider: {error}"))?;
        let (mut repository, bootstrap_chunks) = open_repository(db_path)?;
        let mut vector_index = InMemoryVectorIndex::new(embedding_provider.dimension());
        let bootstrap_entries = build_bootstrap_entries(&embedding_provider, &bootstrap_chunks)?;
        vector_index
            .insert(&bootstrap_entries)
            .map_err(|error| format!("failed to warm in-memory index from sqlite: {error}"))?;
        let initial_index_state = derive_index_state(&repository)
            .map_err(|error| format!("failed to derive initial index state: {error}"))?;
        sync_runtime_schema_meta(&mut repository, &embedding_provider, &initial_index_state)?;
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
        let job = {
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
            services
                .repository
                .create_maintenance_job("rebuild", &request.namespace, Some(&payload_json))
                .map_err(AppStateError::Storage)?
        };

        if let Err(error) = self.spawn_rebuild_worker(job.id, request) {
            let error_message = format!("failed to spawn rebuild worker: {error}");
            let _ = self.fail_rebuild_submission(job.id, &job.namespace, &error_message);
            return Err(AppStateError::Unavailable {
                message: "failed to spawn rebuild worker",
            });
        }

        Ok(job)
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

impl AppState {
    fn spawn_rebuild_worker(
        &self,
        job_id: i64,
        request: CoreRebuildRequest,
    ) -> std::io::Result<()> {
        let state = self.clone();
        thread::Builder::new()
            .name(format!("seahorse-rebuild-{job_id}"))
            .spawn(move || {
                state.run_rebuild_job(job_id, request);
            })
            .map(|_| ())
    }

    fn run_rebuild_job(&self, job_id: i64, request: CoreRebuildRequest) {
        let work = match self.prepare_rebuild_job(job_id, &request) {
            Ok(Some(work)) => work,
            Ok(None) => return,
            Err(error) => {
                let error_message = error.to_string();
                let _ = self.fail_rebuild_submission(job_id, &request.namespace, &error_message);
                return;
            }
        };

        let entries = match build_rebuild_entries(&work.provider, &work.chunks) {
            Ok(entries) => entries,
            Err(error) => {
                let error_message = error.to_string();
                let _ = self.fail_rebuild_submission(job_id, &work.namespace, &error_message);
                return;
            }
        };

        if let Err(error) = self.apply_rebuild_result(job_id, &work, &entries) {
            let error_message = error.to_string();
            let _ = self.fail_rebuild_submission(job_id, &work.namespace, &error_message);
        }
    }

    fn prepare_rebuild_job(
        &self,
        job_id: i64,
        request: &CoreRebuildRequest,
    ) -> Result<Option<PreparedRebuildWork>, AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let job = load_job(&services.repository, job_id)?;
        if job.status == "cancelled" {
            return Ok(None);
        }
        let provider = services.embedding_provider.clone();
        let chunks = match request.scope {
            RebuildScope::All => services
                .repository
                .list_rebuild_chunks(&request.namespace)
                .map_err(AppStateError::Storage)?,
            RebuildScope::MissingIndex => services
                .repository
                .list_missing_index_chunks(&request.namespace)
                .map_err(AppStateError::Storage)?,
        };
        let progress = format!("0/{}", chunks.len());
        services
            .repository
            .mark_maintenance_job_running(job_id, Some(&progress))
            .map_err(AppStateError::Storage)?;
        services
            .repository
            .set_schema_meta_value("index_state", "rebuilding")
            .map_err(AppStateError::Storage)?;

        Ok(Some(PreparedRebuildWork {
            namespace: request.namespace.clone(),
            scope: request.scope,
            provider,
            chunks,
        }))
    }

    fn apply_rebuild_result(
        &self,
        job_id: i64,
        work: &PreparedRebuildWork,
        entries: &[IndexEntry],
    ) -> Result<(), AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let job = load_job(&services.repository, job_id)?;
        if job.status == "cancelled" {
            self.restore_index_state_after_cancel(&mut services.repository, &work.namespace)?;
            return Ok(());
        }

        match work.scope {
            RebuildScope::All => services
                .vector_index
                .rebuild(entries)
                .map_err(RebuildError::from)
                .map_err(AppStateError::Rebuild)?,
            RebuildScope::MissingIndex => {
                if !entries.is_empty() {
                    services
                        .vector_index
                        .insert(entries)
                        .map_err(RebuildError::from)
                        .map_err(AppStateError::Rebuild)?;
                }
            }
        }

        let chunk_ids = work.chunks.iter().map(|chunk| chunk.chunk_id).collect::<Vec<_>>();
        services
            .repository
            .mark_chunks_ready(&work.namespace, &chunk_ids)
            .map_err(AppStateError::Storage)?;
        services
            .repository
            .refresh_file_statuses(&work.namespace)
            .map_err(AppStateError::Storage)?;
        sync_runtime_schema_meta(&mut services.repository, &work.provider, "ready")
            .map_err(|_| AppStateError::Unavailable {
                message: "failed to persist ready rebuild state",
            })?;

        let progress = format!("{0}/{0}", chunk_ids.len());
        let result_summary = format!(
            "scope={}, indexed_chunks={}, scanned_chunks={}",
            work.scope.as_str(),
            chunk_ids.len(),
            chunk_ids.len()
        );
        services
            .repository
            .finish_maintenance_job(
                job_id,
                "succeeded",
                Some(&progress),
                Some(&result_summary),
                None,
            )
            .map_err(AppStateError::Storage)?;

        Ok(())
    }

    fn fail_rebuild_submission(
        &self,
        job_id: i64,
        namespace: &str,
        error_message: &str,
    ) -> Result<(), AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let job = load_job(&services.repository, job_id)?;
        if job.status == "cancelled" {
            self.restore_index_state_after_cancel(&mut services.repository, namespace)?;
            return Ok(());
        }

        let fallback_index_state = derive_index_state(&services.repository)
            .map_err(AppStateError::Storage)?;
        services
            .repository
            .finish_maintenance_job(job_id, "failed", None, None, Some(error_message))
            .map_err(AppStateError::Storage)?;

        if services
            .repository
            .find_active_maintenance_job("rebuild", namespace)
            .map_err(AppStateError::Storage)?
            .is_none()
        {
            services
                .repository
                .set_schema_meta_value("index_state", &fallback_index_state)
                .map_err(AppStateError::Storage)?;
        }

        Ok(())
    }

    fn restore_index_state_after_cancel(
        &self,
        repository: &mut SqliteRepository,
        namespace: &str,
    ) -> Result<(), AppStateError> {
        if repository
            .find_active_maintenance_job("rebuild", namespace)
            .map_err(AppStateError::Storage)?
            .is_some()
        {
            return Ok(());
        }

        let fallback_index_state = derive_index_state(repository).map_err(AppStateError::Storage)?;
        repository
            .set_schema_meta_value("index_state", &fallback_index_state)
            .map_err(AppStateError::Storage)?;

        Ok(())
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

fn build_rebuild_entries(
    embedding_provider: &StubEmbeddingProvider,
    chunks: &[RebuildChunkRecord],
) -> Result<Vec<IndexEntry>, RebuildError> {
    let mut entries = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let vector = embedding_provider.embed(&chunk.chunk_text)?;
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

fn derive_index_state(repository: &SqliteRepository) -> Result<String, StorageError> {
    if repository.list_missing_index_chunks("default")?.is_empty() {
        Ok("ready".to_owned())
    } else {
        Ok("degraded".to_owned())
    }
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
