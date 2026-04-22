use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rusqlite::{params, Connection};
use seahorse_core::{
    apply_sqlite_migrations, EmbeddingProvider, ForgetError, ForgetPipeline,
    ForgetRequest as CoreForgetRequest, ForgetResult as CoreForgetResult, InMemoryVectorIndex,
    IndexEntry, IndexError, IngestError, IngestPipeline, IngestRequest as CoreIngestRequest,
    IngestResult as CoreIngestResult, MaintenanceJob, RebuildChunkRecord, RebuildError,
    RebuildRequest as CoreRebuildRequest, RebuildScope, RecallError, RecallPipeline,
    RecallRequest as CoreRecallRequest, RecallResult as CoreRecallResult, RepairTask,
    RepairTaskExecutor, RepairWorker, RepairWorkerConfig, RepairWorkerRunResult, SqliteRepository,
    StatusCount as CoreStatusCount, StorageError, StubEmbeddingProvider, VectorIndex,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, error, info, warn};

use crate::config::{load_server_config_default, ServerConfig};

const DEFAULT_NAMESPACE: &str = "default";
const REPAIR_POLL_INTERVAL: Duration = Duration::from_millis(500);
const REPAIR_RECOVERY_ERROR: &str = "repair task recovered after unclean shutdown";

#[derive(Debug, Clone)]
pub struct AppState {
    services: Arc<Mutex<AppServices>>,
    embedding_provider: StubEmbeddingProvider,
    vector_index: Arc<Mutex<RuntimeVectorIndex>>,
    repair_db_path: String,
    repair_worker_config: RepairWorkerConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[doc(hidden)]
pub struct RuntimeIndexFaultConfig {
    fail_insert_attempts: usize,
    fail_rebuild_attempts: usize,
}

impl RuntimeIndexFaultConfig {
    pub fn fail_insert_times(mut self, attempts: usize) -> Self {
        self.fail_insert_attempts = attempts;
        self
    }

    pub fn fail_insert_always(mut self) -> Self {
        self.fail_insert_attempts = usize::MAX;
        self
    }

    pub fn fail_rebuild_times(mut self, attempts: usize) -> Self {
        self.fail_rebuild_attempts = attempts;
        self
    }

    fn consume_insert_failure(&mut self) -> bool {
        consume_fault_attempt(&mut self.fail_insert_attempts)
    }

    fn consume_rebuild_failure(&mut self) -> bool {
        consume_fault_attempt(&mut self.fail_rebuild_attempts)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
pub struct AppStateTestOptions {
    runtime_index_faults: RuntimeIndexFaultConfig,
    spawn_repair_worker: bool,
}

impl Default for AppStateTestOptions {
    fn default() -> Self {
        Self {
            runtime_index_faults: RuntimeIndexFaultConfig::default(),
            spawn_repair_worker: true,
        }
    }
}

impl AppStateTestOptions {
    pub fn with_runtime_index_faults(mut self, faults: RuntimeIndexFaultConfig) -> Self {
        self.runtime_index_faults = faults;
        self
    }

    pub fn with_spawn_repair_worker(mut self, spawn_repair_worker: bool) -> Self {
        self.spawn_repair_worker = spawn_repair_worker;
        self
    }
}

#[derive(Debug)]
struct AppServices {
    repository: SqliteRepository,
    db_label: String,
}

#[derive(Debug)]
struct RuntimeVectorIndex {
    inner: InMemoryVectorIndex,
    faults: RuntimeIndexFaultConfig,
}

impl RuntimeVectorIndex {
    fn new(dimension: usize) -> Self {
        Self {
            inner: InMemoryVectorIndex::new(dimension),
            faults: RuntimeIndexFaultConfig::default(),
        }
    }

    fn set_faults(&mut self, faults: RuntimeIndexFaultConfig) {
        self.faults = faults;
    }

    fn maybe_fail_insert(&mut self, entries: &[IndexEntry]) -> Result<(), IndexError> {
        if self.faults.consume_insert_failure() {
            return Err(synthetic_dimension_mismatch(
                self.inner.dimension(),
                entries.first().map(|entry| entry.vector.len()),
            ));
        }

        Ok(())
    }

    fn maybe_fail_rebuild(&mut self, entries: &[IndexEntry]) -> Result<(), IndexError> {
        if self.faults.consume_rebuild_failure() {
            return Err(synthetic_dimension_mismatch(
                self.inner.dimension(),
                entries.first().map(|entry| entry.vector.len()),
            ));
        }

        Ok(())
    }
}

impl VectorIndex for RuntimeVectorIndex {
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn insert(&mut self, entries: &[IndexEntry]) -> seahorse_core::IndexResult<()> {
        self.maybe_fail_insert(entries)?;
        self.inner.insert(entries)
    }

    fn search(
        &self,
        request: &seahorse_core::SearchRequest,
    ) -> seahorse_core::IndexResult<Vec<seahorse_core::SearchHit>> {
        self.inner.search(request)
    }

    fn mark_deleted(
        &mut self,
        namespace: &str,
        chunk_ids: &[i64],
    ) -> seahorse_core::IndexResult<usize> {
        self.inner.mark_deleted(namespace, chunk_ids)
    }

    fn rebuild(&mut self, entries: &[IndexEntry]) -> seahorse_core::IndexResult<()> {
        self.maybe_fail_rebuild(entries)?;
        self.inner.rebuild(entries)
    }
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

#[derive(Debug, Deserialize)]
struct RebuildJobPayload {
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IndexInsertRepairPayload {
    file_id: i64,
    chunk_ids: Vec<i64>,
    model_id: String,
    dimension: usize,
    error: String,
}

#[derive(Debug, Deserialize)]
struct IndexDeleteRepairPayload {
    chunk_ids: Vec<i64>,
    error: String,
}

#[derive(Debug, Deserialize)]
struct ConnectomeRebuildRepairPayload {
    deleted_chunk_ids: Vec<i64>,
    reason: String,
}

#[derive(Debug)]
struct ServerRepairTaskExecutor {
    db_path: String,
    embedding_provider: StubEmbeddingProvider,
    vector_index: Arc<Mutex<RuntimeVectorIndex>>,
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

#[derive(Debug, Clone)]
pub struct StatusCountSnapshot {
    pub status: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub stats: StatsSnapshot,
    pub health: HealthSnapshot,
    pub repair_queue_statuses: Vec<StatusCountSnapshot>,
    pub rebuild_job_statuses: Vec<StatusCountSnapshot>,
    pub repair_oldest_task_age_seconds: Option<f64>,
    pub rebuild_oldest_active_job_age_seconds: Option<f64>,
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

impl fmt::Display for AppStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppStateError::Unavailable { message } => write!(f, "unavailable: {message}"),
            AppStateError::Ingest(error) => write!(f, "ingest error: {error}"),
            AppStateError::Forget(error) => write!(f, "forget error: {error}"),
            AppStateError::Recall(error) => write!(f, "recall error: {error}"),
            AppStateError::Rebuild(error) => write!(f, "rebuild error: {error}"),
            AppStateError::Storage(error) => write!(f, "storage error: {error}"),
            AppStateError::NotFound { message } => write!(f, "not found: {message}"),
        }
    }
}

impl std::error::Error for AppStateError {}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let config = load_server_config_default()?;
        Self::new_with_config(&config)
    }

    pub fn new_with_config(config: &ServerConfig) -> Result<Self, String> {
        Self::new_with_runtime_options(config, AppStateTestOptions::default())
    }

    #[doc(hidden)]
    pub fn new_with_test_options(
        config: &ServerConfig,
        options: AppStateTestOptions,
    ) -> Result<Self, String> {
        Self::new_with_runtime_options(config, options)
    }

    fn new_with_runtime_options(
        config: &ServerConfig,
        options: AppStateTestOptions,
    ) -> Result<Self, String> {
        let repair_worker_config = config.jobs.repair_worker_config();
        if repair_worker_config.max_retries == 0 {
            return Err("repair_max_retries must be greater than zero".to_owned());
        }
        if repair_worker_config.batch_size == 0 {
            return Err("repair_batch_size must be greater than zero".to_owned());
        }

        let db_path = config.storage.db_path.as_str();
        let embedding_provider = StubEmbeddingProvider::from_dimension(config.embedding.dimension)
            .map_err(|error| format!("failed to initialize embedding provider: {error}"))?;
        let (mut repository, bootstrap_chunks) = open_repository(db_path)?;
        repository
            .recover_running_repair_tasks(
                DEFAULT_NAMESPACE,
                repair_worker_config.max_retries,
                REPAIR_RECOVERY_ERROR,
            )
            .map_err(|error| format!("failed to recover running repair tasks: {error}"))?;
        let mut vector_index = RuntimeVectorIndex::new(embedding_provider.dimension());
        let bootstrap_entries = build_bootstrap_entries(&embedding_provider, &bootstrap_chunks)?;
        vector_index
            .insert(&bootstrap_entries)
            .map_err(|error| format!("failed to warm in-memory index from sqlite: {error}"))?;
        vector_index.set_faults(options.runtime_index_faults.clone());
        let has_active_rebuild = repository
            .find_active_maintenance_job("rebuild", DEFAULT_NAMESPACE)
            .map_err(|error| format!("failed to inspect active rebuild job: {error}"))?
            .is_some();
        let initial_index_state = if has_active_rebuild {
            "rebuilding".to_owned()
        } else {
            derive_index_state(&repository)
                .map_err(|error| format!("failed to derive initial index state: {error}"))?
        };
        sync_runtime_schema_meta(&mut repository, &embedding_provider, &initial_index_state)?;
        info!(
            event = "server.bootstrap.ready",
            db_path = db_path,
            bootstrap_chunk_count = bootstrap_chunks.len(),
            index_state = %initial_index_state,
            has_active_rebuild = has_active_rebuild,
            "application state initialized"
        );
        let db_label = if db_path == ":memory:" {
            "sqlite-memory".to_owned()
        } else {
            "sqlite".to_owned()
        };

        let state = Self {
            services: Arc::new(Mutex::new(AppServices {
                repository,
                db_label,
            })),
            embedding_provider,
            vector_index: Arc::new(Mutex::new(vector_index)),
            repair_db_path: db_path.to_owned(),
            repair_worker_config,
        };
        state.recover_active_rebuild_job()?;
        if options.spawn_repair_worker {
            state.spawn_repair_worker()?;
        }

        Ok(state)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new_with_db_path(db_path: &str) -> Result<Self, String> {
        let mut config = ServerConfig::default();
        config.storage.db_path = db_path.to_owned();
        Self::new_with_config(&config)
    }

    #[doc(hidden)]
    pub fn run_repair_worker_once_for_tests(&self) -> Result<RepairWorkerRunResult, String> {
        let mut repository = open_runtime_repository(&self.repair_db_path)?;
        let mut executor = ServerRepairTaskExecutor {
            db_path: self.repair_db_path.clone(),
            embedding_provider: self.embedding_provider.clone(),
            vector_index: Arc::clone(&self.vector_index),
        };
        let mut worker =
            RepairWorker::new(&mut repository, &mut executor, self.repair_worker_config)
                .map_err(|error| format!("failed to create test repair worker: {error}"))?;

        worker
            .run_once(DEFAULT_NAMESPACE)
            .map_err(|error| format!("failed to run test repair worker: {error}"))
    }

    pub fn ingest(&self, request: CoreIngestRequest) -> Result<CoreIngestResult, AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let provider = self.embedding_provider.clone();
        let mut vector_index =
            self.vector_index
                .lock()
                .map_err(|_| AppStateError::Unavailable {
                    message: "vector index lock poisoned",
                })?;
        let mut pipeline =
            IngestPipeline::new(&mut services.repository, &provider, &mut *vector_index);

        pipeline.ingest(request).map_err(AppStateError::Ingest)
    }

    pub fn recall(&self, request: CoreRecallRequest) -> Result<CoreRecallResult, AppStateError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let provider = self.embedding_provider.clone();
        let vector_index = self
            .vector_index
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "vector index lock poisoned",
            })?;
        let mut pipeline = RecallPipeline::new(&mut services.repository, &provider, &*vector_index);
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
        let mut vector_index =
            self.vector_index
                .lock()
                .map_err(|_| AppStateError::Unavailable {
                    message: "vector index lock poisoned",
                })?;
        let mut pipeline = ForgetPipeline::new(&mut services.repository, &mut *vector_index);

        pipeline.forget(request).map_err(AppStateError::Forget)
    }

    pub fn rebuild(
        &self,
        request: CoreRebuildRequest,
        force: bool,
    ) -> Result<MaintenanceJob, AppStateError> {
        let scope = request.scope.as_str().to_owned();
        info!(
            event = "rebuild.submit.received",
            namespace = %request.namespace,
            scope = %scope,
            force = force,
            "rebuild submission received"
        );
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
                    info!(
                        event = "rebuild.submit.reused_active_job",
                        namespace = %request.namespace,
                        scope = %scope,
                        active_job_id = active_job.id,
                        active_job_status = %active_job.status,
                        "reusing active rebuild job"
                    );
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
                warn!(
                    event = "rebuild.submit.cancelled_active_jobs",
                    namespace = %request.namespace,
                    scope = %scope,
                    "force rebuild cancelled active rebuild jobs"
                );
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
        info!(
            event = "rebuild.job.queued",
            namespace = %job.namespace,
            scope = %scope,
            force = force,
            job_id = job.id,
            "rebuild job queued"
        );

        if let Err(error) = self.spawn_rebuild_worker(job.id, request) {
            let error_message = format!("failed to spawn rebuild worker: {error}");
            error!(
                event = "rebuild.worker.spawn_failed",
                namespace = %job.namespace,
                scope = %scope,
                force = force,
                job_id = job.id,
                error = %error,
                "failed to spawn rebuild worker"
            );
            let _ = self.fail_rebuild_submission(job.id, &job.namespace, &error_message);
            return Err(AppStateError::Unavailable {
                message: "failed to spawn rebuild worker",
            });
        }
        info!(
            event = "rebuild.worker.spawned",
            namespace = %job.namespace,
            scope = %scope,
            force = force,
            job_id = job.id,
            "rebuild worker spawned"
        );

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
        let vector_index = self
            .vector_index
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "vector index lock poisoned",
            })?;

        Ok(HealthSnapshot {
            status: health_status_from_index_state(&index_state).to_owned(),
            db: services.db_label.clone(),
            index: format!("memory-{}d", vector_index.dimension()),
            embedding_provider: self.embedding_provider.model_id().to_owned(),
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
            .load_stats(DEFAULT_NAMESPACE)
            .map_err(AppStateError::Storage)?;

        Ok(StatsSnapshot {
            chunk_count: stats.chunk_count,
            tag_count: stats.tag_count,
            deleted_chunk_count: stats.deleted_chunk_count,
            repair_queue_size: stats.repair_queue_size,
            index_status: stats.index_status,
        })
    }

    pub fn metrics_snapshot(&self) -> Result<MetricsSnapshot, AppStateError> {
        let services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let index_state = current_index_state(&services.repository)?;
        let stats = services
            .repository
            .load_stats(DEFAULT_NAMESPACE)
            .map_err(AppStateError::Storage)?;
        let repair_queue_statuses = services
            .repository
            .load_repair_queue_status_counts(DEFAULT_NAMESPACE)
            .map_err(AppStateError::Storage)?
            .into_iter()
            .map(map_status_count)
            .collect();
        let rebuild_job_statuses = services
            .repository
            .load_maintenance_job_status_counts("rebuild", DEFAULT_NAMESPACE)
            .map_err(AppStateError::Storage)?
            .into_iter()
            .map(map_status_count)
            .collect();
        let repair_oldest_task_age_seconds = services
            .repository
            .load_oldest_repair_task_age_seconds(DEFAULT_NAMESPACE)
            .map_err(AppStateError::Storage)?;
        let rebuild_oldest_active_job_age_seconds = services
            .repository
            .load_oldest_active_maintenance_job_age_seconds("rebuild", DEFAULT_NAMESPACE)
            .map_err(AppStateError::Storage)?;
        let vector_index = self
            .vector_index
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "vector index lock poisoned",
            })?;

        Ok(MetricsSnapshot {
            stats: StatsSnapshot {
                chunk_count: stats.chunk_count,
                tag_count: stats.tag_count,
                deleted_chunk_count: stats.deleted_chunk_count,
                repair_queue_size: stats.repair_queue_size,
                index_status: stats.index_status,
            },
            health: HealthSnapshot {
                status: health_status_from_index_state(&index_state).to_owned(),
                db: services.db_label.clone(),
                index: format!("memory-{}d", vector_index.dimension()),
                embedding_provider: self.embedding_provider.model_id().to_owned(),
            },
            repair_queue_statuses,
            rebuild_job_statuses,
            repair_oldest_task_age_seconds,
            rebuild_oldest_active_job_age_seconds,
        })
    }
}

impl AppState {
    fn spawn_repair_worker(&self) -> Result<(), String> {
        if self.repair_db_path == ":memory:" {
            return Ok(());
        }

        let db_path = self.repair_db_path.clone();
        let repair_worker_config = self.repair_worker_config;
        let embedding_provider = self.embedding_provider.clone();
        let vector_index = Arc::clone(&self.vector_index);
        thread::Builder::new()
            .name("seahorse-repair-default".to_owned())
            .spawn(move || {
                run_repair_worker_loop(
                    db_path,
                    repair_worker_config,
                    embedding_provider,
                    vector_index,
                );
            })
            .map(|_| ())
            .map_err(|error| format!("failed to spawn repair worker: {error}"))
    }

    fn recover_active_rebuild_job(&self) -> Result<(), String> {
        let active_job = {
            let mut services = self.services.lock().map_err(|_| {
                "application state lock poisoned during startup recovery".to_owned()
            })?;
            let active_jobs = services
                .repository
                .list_active_maintenance_jobs("rebuild", DEFAULT_NAMESPACE)
                .map_err(|error| {
                    format!("failed to query active rebuild jobs during startup recovery: {error}")
                })?;
            let Some(latest_job) = active_jobs.first().cloned() else {
                debug!(
                    event = "rebuild.startup.no_active_job",
                    namespace = DEFAULT_NAMESPACE,
                    "startup recovery found no active rebuild job"
                );
                return Ok(());
            };

            for stale_job in active_jobs.iter().skip(1) {
                services
                    .repository
                    .cancel_maintenance_job(stale_job.id, "superseded during startup recovery")
                    .map_err(|error| {
                        format!(
                            "failed to cancel stale rebuild job {} during startup recovery: {error}",
                            stale_job.id
                        )
                    })?;
                warn!(
                    event = "rebuild.startup.cancel_stale_job",
                    namespace = %stale_job.namespace,
                    stale_job_id = stale_job.id,
                    "cancelled stale rebuild job during startup recovery"
                );
            }

            latest_job
        };
        let job = active_job;
        info!(
            event = "rebuild.startup.recover_active_job",
            namespace = %job.namespace,
            job_id = job.id,
            status = %job.status,
            "startup recovery will resume active rebuild job"
        );

        let request = match rebuild_request_from_job(&job) {
            Ok(request) => request,
            Err(message) => {
                error!(
                    event = "rebuild.startup.invalid_job_payload",
                    namespace = %job.namespace,
                    job_id = job.id,
                    reason = %message,
                    "startup recovery found invalid rebuild job payload"
                );
                let _ = self.fail_rebuild_submission(job.id, &job.namespace, &message);
                return Ok(());
            }
        };

        if let Err(error) = self.spawn_rebuild_worker(job.id, request) {
            let error_message = format!("failed to resume rebuild worker: {error}");
            error!(
                event = "rebuild.startup.resume_worker_failed",
                namespace = %job.namespace,
                job_id = job.id,
                error = %error,
                "startup recovery failed to resume rebuild worker"
            );
            let _ = self.fail_rebuild_submission(job.id, &job.namespace, &error_message);
        } else {
            info!(
                event = "rebuild.startup.resume_worker_spawned",
                namespace = %job.namespace,
                job_id = job.id,
                "startup recovery resumed rebuild worker"
            );
        }

        Ok(())
    }

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
        let scope = request.scope.as_str().to_owned();
        info!(
            event = "rebuild.job.started",
            namespace = %request.namespace,
            scope = %scope,
            job_id = job_id,
            "rebuild worker started"
        );
        let work = match self.prepare_rebuild_job(job_id, &request) {
            Ok(Some(work)) => work,
            Ok(None) => {
                warn!(
                    event = "rebuild.job.cancelled_before_start",
                    namespace = %request.namespace,
                    scope = %scope,
                    job_id = job_id,
                    "rebuild job already cancelled before execution"
                );
                return;
            }
            Err(error) => {
                let error_message = error.to_string();
                error!(
                    event = "rebuild.job.prepare_failed",
                    namespace = %request.namespace,
                    scope = %scope,
                    job_id = job_id,
                    error = %error_message,
                    "failed to prepare rebuild job"
                );
                let _ = self.fail_rebuild_submission(job_id, &request.namespace, &error_message);
                return;
            }
        };

        let entries = match build_rebuild_entries(&work.provider, &work.chunks) {
            Ok(entries) => entries,
            Err(error) => {
                let error_message = error.to_string();
                error!(
                    event = "rebuild.job.embedding_failed",
                    namespace = %work.namespace,
                    scope = %scope,
                    job_id = job_id,
                    error = %error_message,
                    "failed to build rebuild vectors"
                );
                let _ = self.fail_rebuild_submission(job_id, &work.namespace, &error_message);
                return;
            }
        };

        if let Err(error) = self.apply_rebuild_result(job_id, &work, &entries) {
            let error_message = error.to_string();
            error!(
                event = "rebuild.job.apply_failed",
                namespace = %work.namespace,
                scope = %scope,
                job_id = job_id,
                error = %error_message,
                "failed to apply rebuild result"
            );
            let _ = self.fail_rebuild_submission(job_id, &work.namespace, &error_message);
        } else {
            info!(
                event = "rebuild.job.completed",
                namespace = %work.namespace,
                scope = %scope,
                job_id = job_id,
                indexed_chunk_count = work.chunks.len(),
                "rebuild worker completed"
            );
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
            warn!(
                event = "rebuild.job.prepare_cancelled",
                namespace = %request.namespace,
                scope = %request.scope.as_str(),
                job_id = job_id,
                "rebuild job is cancelled and will not run"
            );
            return Ok(None);
        }
        let provider = self.embedding_provider.clone();
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
        info!(
            event = "rebuild.job.running",
            namespace = %request.namespace,
            scope = %request.scope.as_str(),
            job_id = job_id,
            chunk_count = chunks.len(),
            progress = %progress,
            index_state = "rebuilding",
            "rebuild job marked running"
        );

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
        {
            let mut services = self
                .services
                .lock()
                .map_err(|_| AppStateError::Unavailable {
                    message: "application state lock poisoned",
                })?;
            let job = load_job(&services.repository, job_id)?;
            if job.status == "cancelled" {
                warn!(
                    event = "rebuild.job.cancelled_during_execution",
                    namespace = %work.namespace,
                    scope = %work.scope.as_str(),
                    job_id = job_id,
                    "rebuild job cancelled while worker was running"
                );
                self.restore_index_state_after_cancel(&mut services.repository, &work.namespace)?;
                return Ok(());
            }
        }

        {
            let mut vector_index =
                self.vector_index
                    .lock()
                    .map_err(|_| AppStateError::Unavailable {
                        message: "vector index lock poisoned",
                    })?;
            match work.scope {
                RebuildScope::All => vector_index
                    .rebuild(entries)
                    .map_err(RebuildError::from)
                    .map_err(AppStateError::Rebuild)?,
                RebuildScope::MissingIndex => {
                    if !entries.is_empty() {
                        vector_index
                            .insert(entries)
                            .map_err(RebuildError::from)
                            .map_err(AppStateError::Rebuild)?;
                    }
                }
            }
        }

        let mut services = self
            .services
            .lock()
            .map_err(|_| AppStateError::Unavailable {
                message: "application state lock poisoned",
            })?;
        let chunk_ids = work
            .chunks
            .iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        services
            .repository
            .mark_chunks_ready(&work.namespace, &chunk_ids)
            .map_err(AppStateError::Storage)?;
        services
            .repository
            .refresh_file_statuses(&work.namespace)
            .map_err(AppStateError::Storage)?;
        sync_runtime_schema_meta(&mut services.repository, &work.provider, "ready").map_err(
            |_| AppStateError::Unavailable {
                message: "failed to persist ready rebuild state",
            },
        )?;

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
        info!(
            event = "rebuild.job.succeeded",
            namespace = %work.namespace,
            scope = %work.scope.as_str(),
            job_id = job_id,
            indexed_chunk_count = chunk_ids.len(),
            progress = %progress,
            index_state = "ready",
            "rebuild job completed successfully"
        );

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
            warn!(
                event = "rebuild.job.fail_ignored_cancelled",
                namespace = %namespace,
                job_id = job_id,
                "skip marking failed because rebuild job is already cancelled"
            );
            self.restore_index_state_after_cancel(&mut services.repository, namespace)?;
            return Ok(());
        }

        let fallback_index_state =
            derive_index_state(&services.repository).map_err(AppStateError::Storage)?;
        services
            .repository
            .finish_maintenance_job(job_id, "failed", None, None, Some(error_message))
            .map_err(AppStateError::Storage)?;
        error!(
            event = "rebuild.job.failed",
            namespace = %namespace,
            job_id = job_id,
            fallback_index_state = %fallback_index_state,
            error = %error_message,
            "rebuild job marked failed"
        );

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
            info!(
                event = "rebuild.index_state.restored_after_failure",
                namespace = %namespace,
                job_id = job_id,
                index_state = %fallback_index_state,
                "index_state restored after rebuild failure"
            );
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
            debug!(
                event = "rebuild.index_state.restore_skipped_active_exists",
                namespace = %namespace,
                "skip restoring index_state because active rebuild job still exists"
            );
            return Ok(());
        }

        let fallback_index_state =
            derive_index_state(repository).map_err(AppStateError::Storage)?;
        repository
            .set_schema_meta_value("index_state", &fallback_index_state)
            .map_err(AppStateError::Storage)?;
        info!(
            event = "rebuild.index_state.restored_after_cancel",
            namespace = %namespace,
            index_state = %fallback_index_state,
            "index_state restored after rebuild cancellation"
        );

        Ok(())
    }
}

impl ServerRepairTaskExecutor {
    fn execute_index_insert(
        &mut self,
        task: &RepairTask,
        payload: IndexInsertRepairPayload,
    ) -> Result<(), String> {
        info!(
            event = "repair.task.started",
            task_id = task.id,
            task_type = "index_insert",
            namespace = %task.namespace,
            chunk_count = payload.chunk_ids.len(),
            "repair task started"
        );
        if payload.chunk_ids.is_empty() {
            return Err(format!("repair task {} has empty chunk_ids", task.id));
        }
        if payload.error.trim().is_empty() {
            return Err(format!("repair task {} is missing source error", task.id));
        }
        if payload.model_id != self.embedding_provider.model_id() {
            return Err(format!(
                "repair task {} model_id mismatch: expected {}, got {}",
                task.id,
                self.embedding_provider.model_id(),
                payload.model_id
            ));
        }
        if payload.dimension != self.embedding_provider.dimension() {
            return Err(format!(
                "repair task {} embedding dimension mismatch: expected {}, got {}",
                task.id,
                self.embedding_provider.dimension(),
                payload.dimension
            ));
        }

        let mut repository = open_runtime_repository(&self.db_path)?;
        let mut entries = Vec::with_capacity(payload.chunk_ids.len());
        for chunk_id in &payload.chunk_ids {
            let record = repository
                .get_chunk_record(*chunk_id)
                .map_err(|error| format!("failed to load chunk {chunk_id} for repair: {error}"))?
                .ok_or_else(|| format!("repair task {} chunk {} not found", task.id, chunk_id))?;
            if record.namespace != task.namespace {
                return Err(format!(
                    "repair task {} chunk {} namespace mismatch: expected {}, got {}",
                    task.id, chunk_id, task.namespace, record.namespace
                ));
            }
            if record.file_id != payload.file_id {
                return Err(format!(
                    "repair task {} chunk {} file_id mismatch: expected {}, got {}",
                    task.id, chunk_id, payload.file_id, record.file_id
                ));
            }

            let vector = self
                .embedding_provider
                .embed(&record.chunk_text)
                .map_err(|error| {
                    format!(
                        "failed to embed chunk {} for repair: {error}",
                        record.chunk_id
                    )
                })?;
            entries.push(IndexEntry::new(record.chunk_id, record.namespace, vector));
        }

        {
            let mut vector_index = self
                .vector_index
                .lock()
                .map_err(|_| "vector index lock poisoned".to_owned())?;
            vector_index
                .insert(&entries)
                .map_err(|error| format!("repair task {} index insert failed: {error}", task.id))?;
        }

        repository
            .mark_chunks_ready(&task.namespace, &payload.chunk_ids)
            .map_err(|error| {
                format!(
                    "failed to mark chunks ready for repair task {}: {error}",
                    task.id
                )
            })?;
        repository
            .refresh_file_statuses(&task.namespace)
            .map_err(|error| {
                format!(
                    "failed to refresh file statuses for repair task {}: {error}",
                    task.id
                )
            })?;
        let index_state = derive_index_state_after_repair(&repository, &task.namespace, task.id)
            .map_err(|error| format!("failed to derive repair index state: {error}"))?;
        sync_runtime_schema_meta(&mut repository, &self.embedding_provider, &index_state)?;
        info!(
            event = "repair.task.succeeded",
            task_id = task.id,
            task_type = "index_insert",
            namespace = %task.namespace,
            chunk_count = payload.chunk_ids.len(),
            index_state = %index_state,
            "repair task completed"
        );

        Ok(())
    }

    fn execute_index_delete(
        &mut self,
        task: &RepairTask,
        payload: IndexDeleteRepairPayload,
    ) -> Result<(), String> {
        info!(
            event = "repair.task.started",
            task_id = task.id,
            task_type = "index_delete",
            namespace = %task.namespace,
            chunk_count = payload.chunk_ids.len(),
            "repair task started"
        );
        if payload.chunk_ids.is_empty() {
            return Err(format!("repair task {} has empty chunk_ids", task.id));
        }
        if payload.error.trim().is_empty() {
            return Err(format!("repair task {} is missing source error", task.id));
        }

        let mut vector_index = self
            .vector_index
            .lock()
            .map_err(|_| "vector index lock poisoned".to_owned())?;
        vector_index
            .mark_deleted(&task.namespace, &payload.chunk_ids)
            .map_err(|error| format!("repair task {} index delete failed: {error}", task.id))?;

        drop(vector_index);

        let mut repository = open_runtime_repository(&self.db_path)?;
        let index_state = derive_index_state_after_repair(&repository, &task.namespace, task.id)
            .map_err(|error| format!("failed to derive repair index state: {error}"))?;
        sync_runtime_schema_meta(&mut repository, &self.embedding_provider, &index_state)?;
        info!(
            event = "repair.task.succeeded",
            task_id = task.id,
            task_type = "index_delete",
            namespace = %task.namespace,
            chunk_count = payload.chunk_ids.len(),
            index_state = %index_state,
            "repair task completed"
        );

        Ok(())
    }

    fn execute_connectome_rebuild(
        &mut self,
        task: &RepairTask,
        payload: ConnectomeRebuildRepairPayload,
    ) -> Result<(), String> {
        info!(
            event = "repair.task.started",
            task_id = task.id,
            task_type = "connectome_rebuild",
            namespace = %task.namespace,
            deleted_chunk_count = payload.deleted_chunk_ids.len(),
            "repair task started"
        );
        if payload.reason.trim().is_empty() {
            return Err(format!("repair task {} is missing reason", task.id));
        }

        let mut repository = open_runtime_repository(&self.db_path)?;
        repository
            .rebuild_connectome(&task.namespace)
            .map_err(|error| {
                format!("repair task {} connectome rebuild failed: {error}", task.id)
            })?;

        info!(
            event = "repair.task.succeeded",
            task_id = task.id,
            task_type = "connectome_rebuild",
            namespace = %task.namespace,
            deleted_chunk_count = payload.deleted_chunk_ids.len(),
            "repair task completed"
        );

        Ok(())
    }
}

impl RepairTaskExecutor for ServerRepairTaskExecutor {
    fn execute(&mut self, task: &RepairTask) -> Result<(), String> {
        match task.task_type.as_str() {
            "index_insert" => {
                let payload = parse_repair_payload::<IndexInsertRepairPayload>(task)?;
                self.execute_index_insert(task, payload)
            }
            "index_delete" => {
                let payload = parse_repair_payload::<IndexDeleteRepairPayload>(task)?;
                self.execute_index_delete(task, payload)
            }
            "connectome_rebuild" => {
                let payload = parse_repair_payload::<ConnectomeRebuildRepairPayload>(task)?;
                self.execute_connectome_rebuild(task, payload)
            }
            other => {
                warn!(
                    event = "repair.task.unsupported_type",
                    task_id = task.id,
                    namespace = %task.namespace,
                    task_type = %other,
                    "repair task has unsupported task_type"
                );
                Err(format!("unsupported repair task_type: {other}"))
            }
        }
    }
}

fn run_repair_worker_loop(
    db_path: String,
    repair_worker_config: RepairWorkerConfig,
    embedding_provider: StubEmbeddingProvider,
    vector_index: Arc<Mutex<RuntimeVectorIndex>>,
) {
    info!(
        event = "repair.worker.started",
        namespace = DEFAULT_NAMESPACE,
        db_path = %db_path,
        "repair worker loop started"
    );
    loop {
        let mut repository = match open_runtime_repository(&db_path) {
            Ok(repository) => repository,
            Err(error) => {
                warn!(
                    event = "repair.worker.repository_open_failed",
                    namespace = DEFAULT_NAMESPACE,
                    db_path = %db_path,
                    error = %error,
                    "repair worker failed to open repository"
                );
                thread::sleep(REPAIR_POLL_INTERVAL);
                continue;
            }
        };
        let mut executor = ServerRepairTaskExecutor {
            db_path: db_path.clone(),
            embedding_provider: embedding_provider.clone(),
            vector_index: Arc::clone(&vector_index),
        };
        let mut worker =
            match RepairWorker::new(&mut repository, &mut executor, repair_worker_config) {
                Ok(worker) => worker,
                Err(error) => {
                    error!(
                        event = "repair.worker.init_failed",
                        namespace = DEFAULT_NAMESPACE,
                        db_path = %db_path,
                        error = %error,
                        "repair worker initialization failed"
                    );
                    return;
                }
            };

        loop {
            match worker.run_once(DEFAULT_NAMESPACE) {
                Ok(result) => {
                    if result.scanned > 0 {
                        info!(
                            event = "repair.worker.batch_processed",
                            namespace = DEFAULT_NAMESPACE,
                            scanned = result.scanned,
                            "repair worker processed batch"
                        );
                    }
                    if result.scanned == 0 {
                        thread::sleep(REPAIR_POLL_INTERVAL);
                    }
                }
                Err(error) => {
                    warn!(
                        event = "repair.worker.run_once_failed",
                        namespace = DEFAULT_NAMESPACE,
                        error = %error,
                        "repair worker run failed and will retry"
                    );
                    thread::sleep(REPAIR_POLL_INTERVAL);
                    break;
                }
            }
        }
    }
}

fn consume_fault_attempt(remaining: &mut usize) -> bool {
    if *remaining == 0 {
        return false;
    }

    if *remaining != usize::MAX {
        *remaining -= 1;
    }

    true
}

fn synthetic_dimension_mismatch(expected: usize, actual: Option<usize>) -> IndexError {
    let actual = actual.unwrap_or_else(|| expected.saturating_add(1));
    let actual = if actual == expected {
        actual.saturating_add(1)
    } else {
        actual
    };

    IndexError::DimensionMismatch { expected, actual }
}

fn open_repository(path: &str) -> Result<(SqliteRepository, Vec<BootstrapChunk>), String> {
    let connection = if path == ":memory:" {
        Connection::open_in_memory()
            .map_err(|error| format!("failed to open sqlite memory database: {error}"))?
    } else {
        ensure_parent_dir(path)?;
        Connection::open(path)
            .map_err(|error| format!("failed to open sqlite database: {error}"))?
    };

    apply_sqlite_migrations(&connection)
        .map_err(|error| format!("failed to apply sqlite migration: {error}"))?;
    let bootstrap_chunks = load_bootstrap_chunks(&connection)?;
    let repository = SqliteRepository::new(connection)
        .map_err(|error| format!("failed to initialize repository: {error}"))?;

    Ok((repository, bootstrap_chunks))
}

fn open_runtime_repository(path: &str) -> Result<SqliteRepository, String> {
    let connection = if path == ":memory:" {
        Connection::open_in_memory()
            .map_err(|error| format!("failed to open sqlite memory database: {error}"))?
    } else {
        ensure_parent_dir(path)?;
        Connection::open(path)
            .map_err(|error| format!("failed to open sqlite database: {error}"))?
    };

    apply_sqlite_migrations(&connection)
        .map_err(|error| format!("failed to apply sqlite migration: {error}"))?;
    SqliteRepository::new(connection)
        .map_err(|error| format!("failed to initialize repository: {error}"))
}

fn parse_repair_payload<T>(task: &RepairTask) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let payload_json = task
        .payload_json
        .as_deref()
        .ok_or_else(|| format!("repair task {} is missing payload_json", task.id))?;
    serde_json::from_str(payload_json)
        .map_err(|error| format!("repair task {} payload_json is invalid: {error}", task.id))
}

fn ensure_parent_dir(path: &str) -> Result<(), String> {
    let Some(parent) = Path::new(path).parent() else {
        return Ok(());
    };

    if parent.as_os_str().is_empty() {
        return Ok(());
    }

    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create database directory {}: {error}",
            parent.display()
        )
    })
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
            .map_err(|error| {
                format!(
                    "failed to embed bootstrap chunk {}: {error}",
                    chunk.chunk_id
                )
            })?;
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
    if repository.has_repair_backlog(DEFAULT_NAMESPACE)?
        || !repository
            .list_missing_index_chunks(DEFAULT_NAMESPACE)?
            .is_empty()
    {
        Ok("degraded".to_owned())
    } else {
        Ok("ready".to_owned())
    }
}

fn derive_index_state_after_repair(
    repository: &SqliteRepository,
    namespace: &str,
    completed_task_id: i64,
) -> Result<String, StorageError> {
    if repository.has_repair_backlog_excluding(namespace, completed_task_id)?
        || !repository.list_missing_index_chunks(namespace)?.is_empty()
    {
        Ok("degraded".to_owned())
    } else {
        Ok("ready".to_owned())
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

fn map_status_count(value: CoreStatusCount) -> StatusCountSnapshot {
    StatusCountSnapshot {
        status: value.status,
        count: value.count,
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

fn rebuild_request_from_job(job: &MaintenanceJob) -> Result<CoreRebuildRequest, String> {
    let payload = match job.payload_json.as_deref() {
        Some(payload_json) => {
            serde_json::from_str::<RebuildJobPayload>(payload_json).map_err(|error| {
                format!("invalid rebuild job payload for job_id {}: {error}", job.id)
            })?
        }
        None => RebuildJobPayload { scope: None },
    };

    let scope = match payload.scope.as_deref().unwrap_or("all") {
        "all" => RebuildScope::All,
        "missing_index" => RebuildScope::MissingIndex,
        other => {
            return Err(format!(
                "invalid rebuild job scope for job_id {}: {other}",
                job.id
            ))
        }
    };

    Ok(CoreRebuildRequest {
        namespace: job.namespace.clone(),
        scope,
    })
}
