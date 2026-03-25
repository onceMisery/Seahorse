use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use seahorse_core::{
    apply_sqlite_migrations, EmbeddingProvider, ForgetError, ForgetPipeline,
    ForgetRequest as CoreForgetRequest, ForgetResult as CoreForgetResult, InMemoryVectorIndex,
    IndexEntry, IngestError, IngestPipeline, IngestRequest as CoreIngestRequest,
    IngestResult as CoreIngestResult, MaintenanceJob, RebuildChunkRecord, RebuildError,
    RebuildRequest as CoreRebuildRequest, RebuildScope, RecallError, RecallPipeline,
    RecallRequest as CoreRecallRequest, RecallResult as CoreRecallResult, RepairTask,
    RepairTaskExecutor, RepairWorker, RepairWorkerConfig, SqliteRepository, StorageError,
    StubEmbeddingProvider, VectorIndex,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;

const DEFAULT_DB_PATH: &str = "./data/seahorse.db";
const DEFAULT_EMBEDDING_DIMENSION: usize = 1024;
const DEFAULT_NAMESPACE: &str = "default";
#[cfg(test)]
const REPAIR_POLL_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(not(test))]
const REPAIR_POLL_INTERVAL: Duration = Duration::from_millis(500);
const REPAIR_RECOVERY_ERROR: &str = "repair task recovered after unclean shutdown";
const REPAIR_WORKER_CONFIG: RepairWorkerConfig = RepairWorkerConfig {
    max_retries: 3,
    batch_size: 1,
};

#[derive(Debug, Clone)]
struct RuntimeConfig {
    rebuild_watchdog_interval: Duration,
    rebuild_stale_after: Duration,
    repair_watchdog_interval: Duration,
    repair_stale_after: Duration,
    repair_restart_backoff_initial: Duration,
    repair_restart_backoff_max: Duration,
}

impl RuntimeConfig {
    fn production() -> Self {
        if cfg!(test) {
            return Self {
                rebuild_watchdog_interval: Duration::from_secs(60),
                rebuild_stale_after: Duration::from_secs(60),
                repair_watchdog_interval: Duration::from_secs(60),
                repair_stale_after: Duration::from_secs(60),
                repair_restart_backoff_initial: Duration::from_millis(250),
                repair_restart_backoff_max: Duration::from_secs(2),
            };
        }

        Self {
            rebuild_watchdog_interval: Duration::from_millis(250),
            rebuild_stale_after: Duration::from_secs(10),
            repair_watchdog_interval: Duration::from_millis(250),
            repair_stale_after: Duration::from_secs(10),
            repair_restart_backoff_initial: Duration::from_millis(250),
            repair_restart_backoff_max: Duration::from_secs(2),
        }
    }

    #[cfg(test)]
    fn fast_for_tests() -> Self {
        Self {
            rebuild_watchdog_interval: Duration::from_millis(25),
            rebuild_stale_after: Duration::from_millis(100),
            repair_watchdog_interval: Duration::from_millis(25),
            repair_stale_after: Duration::from_millis(100),
            repair_restart_backoff_initial: Duration::from_millis(25),
            repair_restart_backoff_max: Duration::from_millis(100),
        }
    }

    #[cfg(test)]
    fn submission_recovery_for_tests() -> Self {
        Self {
            rebuild_watchdog_interval: Duration::from_secs(5),
            rebuild_stale_after: Duration::from_secs(5),
            repair_watchdog_interval: Duration::from_millis(25),
            repair_stale_after: Duration::from_millis(100),
            repair_restart_backoff_initial: Duration::from_millis(25),
            repair_restart_backoff_max: Duration::from_millis(100),
        }
    }

    #[cfg(test)]
    fn repair_fast_for_tests() -> Self {
        Self {
            rebuild_watchdog_interval: Duration::from_millis(25),
            rebuild_stale_after: Duration::from_millis(100),
            repair_watchdog_interval: Duration::from_millis(25),
            repair_stale_after: Duration::from_millis(100),
            repair_restart_backoff_initial: Duration::from_millis(25),
            repair_restart_backoff_max: Duration::from_millis(100),
        }
    }
}

#[cfg(test)]
#[derive(Debug)]
struct RuntimeTestHooks {
    rebuild_should_panic: std::sync::atomic::AtomicBool,
    rebuild_block_for: Mutex<Option<Duration>>,
    repair_should_panic: std::sync::atomic::AtomicBool,
    repair_block_for: Mutex<Option<Duration>>,
}

#[cfg(test)]
impl Default for RuntimeTestHooks {
    fn default() -> Self {
        Self {
            rebuild_should_panic: std::sync::atomic::AtomicBool::new(false),
            rebuild_block_for: Mutex::new(None),
            repair_should_panic: std::sync::atomic::AtomicBool::new(false),
            repair_block_for: Mutex::new(None),
        }
    }
}

#[cfg(test)]
impl RuntimeTestHooks {
    fn trigger_rebuild_panic(&self) {
        self.rebuild_should_panic
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn maybe_panic_rebuild(&self) {
        if self
            .rebuild_should_panic
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            panic!("rebuild worker panic requested by test hook");
        }
    }

    fn block_rebuild_for(&self, duration: Duration) {
        *self
            .rebuild_block_for
            .lock()
            .expect("runtime test hook lock poisoned") = Some(duration);
    }

    fn block_next_rebuild_for(&self, duration: Duration) {
        self.block_rebuild_for(duration);
    }

    fn maybe_block_rebuild(&self) {
        let duration = self
            .rebuild_block_for
            .lock()
            .expect("runtime test hook lock poisoned")
            .take();
        if let Some(duration) = duration {
            thread::sleep(duration);
        }
    }

    fn trigger_repair_panic(&self) {
        self.repair_should_panic
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn maybe_panic_repair(&self) {
        if self
            .repair_should_panic
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            panic!("repair worker panic requested by test hook");
        }
    }

    fn block_next_repair_for(&self, duration: Duration) {
        *self
            .repair_block_for
            .lock()
            .expect("runtime test hook lock poisoned") = Some(duration);
    }

    fn maybe_block_repair(&self) {
        let duration = self
            .repair_block_for
            .lock()
            .expect("runtime test hook lock poisoned")
            .take();
        if let Some(duration) = duration {
            thread::sleep(duration);
        }
    }
}

#[derive(Debug, Default)]
struct RuntimeState {
    rebuild_workers: Mutex<HashMap<i64, Arc<RebuildWorkerHeartbeat>>>,
    repair_worker: Mutex<Option<Arc<RepairWorkerHeartbeat>>>,
    next_repair_generation: AtomicU64,
}

impl RuntimeState {
    fn register_rebuild_worker(&self, job_id: i64) -> Arc<RebuildWorkerHeartbeat> {
        let heartbeat = Arc::new(RebuildWorkerHeartbeat::new());
        self.rebuild_workers
            .lock()
            .expect("runtime rebuild registry lock poisoned")
            .insert(job_id, Arc::clone(&heartbeat));
        heartbeat
    }

    fn rebuild_worker(&self, job_id: i64) -> Option<Arc<RebuildWorkerHeartbeat>> {
        self.rebuild_workers
            .lock()
            .expect("runtime rebuild registry lock poisoned")
            .get(&job_id)
            .cloned()
    }

    fn unregister_rebuild_worker(&self, job_id: i64) {
        self.rebuild_workers
            .lock()
            .expect("runtime rebuild registry lock poisoned")
            .remove(&job_id);
    }

    fn register_repair_worker(&self) -> Arc<RepairWorkerHeartbeat> {
        let generation = self.next_repair_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let heartbeat = Arc::new(RepairWorkerHeartbeat::new(generation));
        *self
            .repair_worker
            .lock()
            .expect("runtime repair registry lock poisoned") = Some(Arc::clone(&heartbeat));
        heartbeat
    }

    fn current_repair_generation(&self) -> u64 {
        self.repair_worker
            .lock()
            .expect("runtime repair registry lock poisoned")
            .as_ref()
            .map(|heartbeat| heartbeat.generation)
            .unwrap_or(0)
    }

    fn repair_worker(&self) -> Option<Arc<RepairWorkerHeartbeat>> {
        self.repair_worker
            .lock()
            .expect("runtime repair registry lock poisoned")
            .as_ref()
            .cloned()
    }
}

#[derive(Debug)]
struct RebuildWorkerHeartbeat {
    last_seen: Mutex<Instant>,
}

impl RebuildWorkerHeartbeat {
    fn new() -> Self {
        Self {
            last_seen: Mutex::new(Instant::now()),
        }
    }

    fn touch(&self) {
        *self
            .last_seen
            .lock()
            .expect("rebuild heartbeat lock poisoned") = Instant::now();
    }

    fn age(&self) -> Duration {
        self.last_seen
            .lock()
            .expect("rebuild heartbeat lock poisoned")
            .elapsed()
    }
}

#[derive(Debug)]
struct RepairWorkerHeartbeat {
    generation: u64,
    last_seen: Mutex<Instant>,
}

impl RepairWorkerHeartbeat {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            last_seen: Mutex::new(Instant::now()),
        }
    }

    fn touch(&self) {
        *self
            .last_seen
            .lock()
            .expect("repair heartbeat lock poisoned") = Instant::now();
    }

    fn age(&self) -> Duration {
        self.last_seen
            .lock()
            .expect("repair heartbeat lock poisoned")
            .elapsed()
    }
}

#[derive(Debug)]
struct RepairRestartBackoff {
    initial: Duration,
    max: Duration,
    current: Duration,
}

impl RepairRestartBackoff {
    fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max,
            current: initial,
        }
    }

    fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = self.current.saturating_mul(2).min(self.max);
        delay
    }

    fn reset(&mut self) {
        self.current = self.initial;
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    services: Arc<Mutex<AppServices>>,
    embedding_provider: StubEmbeddingProvider,
    vector_index: Arc<Mutex<InMemoryVectorIndex>>,
    repair_db_path: String,
    runtime: Arc<RuntimeState>,
    runtime_config: RuntimeConfig,
    #[cfg(test)]
    test_hooks: Arc<RuntimeTestHooks>,
}

#[derive(Debug)]
struct AppServices {
    repository: SqliteRepository,
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

#[derive(Debug)]
struct ServerRepairTaskExecutor {
    db_path: String,
    embedding_provider: StubEmbeddingProvider,
    vector_index: Arc<Mutex<InMemoryVectorIndex>>,
    #[cfg(test)]
    test_hooks: Arc<RuntimeTestHooks>,
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
        let db_path =
            std::env::var("SEAHORSE_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_owned());
        Self::new_with_runtime_config(
            &db_path,
            RuntimeConfig::production(),
            #[cfg(test)]
            Arc::new(RuntimeTestHooks::default()),
        )
    }

    pub(crate) fn new_with_db_path(db_path: &str) -> Result<Self, String> {
        Self::new_with_runtime_config(
            db_path,
            RuntimeConfig::production(),
            #[cfg(test)]
            Arc::new(RuntimeTestHooks::default()),
        )
    }

    fn new_with_runtime_config(
        db_path: &str,
        runtime_config: RuntimeConfig,
        #[cfg(test)] test_hooks: Arc<RuntimeTestHooks>,
    ) -> Result<Self, String> {
        let embedding_provider = StubEmbeddingProvider::from_dimension(DEFAULT_EMBEDDING_DIMENSION)
            .map_err(|error| format!("failed to initialize embedding provider: {error}"))?;
        let (mut repository, bootstrap_chunks) = open_repository(db_path)?;
        repository
            .recover_running_repair_tasks(
                DEFAULT_NAMESPACE,
                REPAIR_WORKER_CONFIG.max_retries,
                REPAIR_RECOVERY_ERROR,
            )
            .map_err(|error| format!("failed to recover running repair tasks: {error}"))?;
        let mut vector_index = InMemoryVectorIndex::new(embedding_provider.dimension());
        let bootstrap_entries = build_bootstrap_entries(&embedding_provider, &bootstrap_chunks)?;
        vector_index
            .insert(&bootstrap_entries)
            .map_err(|error| format!("failed to warm in-memory index from sqlite: {error}"))?;
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
            runtime: Arc::new(RuntimeState::default()),
            runtime_config,
            #[cfg(test)]
            test_hooks,
        };
        state.recover_active_rebuild_job()?;
        state.spawn_repair_worker()?;
        state.spawn_runtime_watchdog()?;

        Ok(state)
    }

    #[cfg(test)]
    fn new_with_runtime_for_tests(
        db_path: &str,
        runtime_config: RuntimeConfig,
        test_hooks: Arc<RuntimeTestHooks>,
    ) -> Result<Self, String> {
        Self::new_with_runtime_config(db_path, runtime_config, test_hooks)
    }

    #[cfg(test)]
    fn current_repair_generation_for_tests(&self) -> u64 {
        self.runtime.current_repair_generation()
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
        let services = self
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
        let pipeline = RecallPipeline::new(&services.repository, &provider, &*vector_index);
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
        self.recover_stale_active_rebuild_job(&request.namespace)?;

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
}

impl AppState {
    fn spawn_repair_worker(&self) -> Result<(), String> {
        if self.repair_db_path == ":memory:" {
            return Ok(());
        }

        let db_path = self.repair_db_path.clone();
        let embedding_provider = self.embedding_provider.clone();
        let vector_index = Arc::clone(&self.vector_index);
        let heartbeat = self.runtime.register_repair_worker();
        let runtime = Arc::clone(&self.runtime);
        let runtime_config = self.runtime_config.clone();
        #[cfg(test)]
        let test_hooks = Arc::clone(&self.test_hooks);
        thread::Builder::new()
            .name("seahorse-repair-default".to_owned())
            .spawn(move || {
                run_repair_worker_loop(
                    db_path,
                    embedding_provider,
                    vector_index,
                    runtime,
                    heartbeat,
                    runtime_config,
                    #[cfg(test)]
                    test_hooks,
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
            }

            latest_job
        };
        let job = active_job;

        let request = match rebuild_request_from_job(&job) {
            Ok(request) => request,
            Err(message) => {
                let _ = self.fail_rebuild_submission(job.id, &job.namespace, &message);
                return Ok(());
            }
        };

        if let Err(error) = self.spawn_rebuild_worker(job.id, request) {
            let error_message = format!("failed to resume rebuild worker: {error}");
            let _ = self.fail_rebuild_submission(job.id, &job.namespace, &error_message);
        }

        Ok(())
    }

    fn spawn_rebuild_worker(
        &self,
        job_id: i64,
        request: CoreRebuildRequest,
    ) -> std::io::Result<()> {
        let state = self.clone();
        let namespace = request.namespace.clone();
        let heartbeat = self.runtime.register_rebuild_worker(job_id);
        thread::Builder::new()
            .name(format!("seahorse-rebuild-{job_id}"))
            .spawn(move || {
                heartbeat.touch();
                if let Err(payload) =
                    panic::catch_unwind(AssertUnwindSafe(|| state.run_rebuild_job(job_id, request)))
                {
                    let error_message = format!(
                        "rebuild worker panic: {}",
                        panic_payload_to_string(payload.as_ref())
                    );
                    let _ = state.fail_rebuild_submission(job_id, &namespace, &error_message);
                }
                state.runtime.unregister_rebuild_worker(job_id);
            })
            .map(|_| ())
    }

    fn run_rebuild_job(&self, job_id: i64, request: CoreRebuildRequest) {
        #[cfg(test)]
        {
            self.test_hooks.maybe_panic_rebuild();
            self.test_hooks.maybe_block_rebuild();
        }

        self.touch_rebuild_worker(job_id);

        let work = match self.prepare_rebuild_job(job_id, &request) {
            Ok(Some(work)) => work,
            Ok(None) => return,
            Err(error) => {
                let error_message = error.to_string();
                let _ = self.fail_rebuild_submission(job_id, &request.namespace, &error_message);
                return;
            }
        };

        self.touch_rebuild_worker(job_id);

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

    fn spawn_runtime_watchdog(&self) -> Result<(), String> {
        let state = self.clone();
        thread::Builder::new()
            .name("seahorse-runtime-watchdog".to_owned())
            .spawn(move || state.run_runtime_watchdog_loop())
            .map(|_| ())
            .map_err(|error| format!("failed to spawn runtime watchdog: {error}"))
    }

    fn run_runtime_watchdog_loop(&self) {
        let interval = self
            .runtime_config
            .rebuild_watchdog_interval
            .min(self.runtime_config.repair_watchdog_interval);
        loop {
            thread::sleep(interval);
            self.recover_stale_rebuild_jobs();
            self.recover_stale_repair_worker();
        }
    }

    fn recover_stale_active_rebuild_job(&self, namespace: &str) -> Result<(), AppStateError> {
        let active_job = {
            let services = self
                .services
                .lock()
                .map_err(|_| AppStateError::Unavailable {
                    message: "application state lock poisoned",
                })?;
            services
                .repository
                .find_active_maintenance_job("rebuild", namespace)
                .map_err(AppStateError::Storage)?
        };

        let Some(active_job) = active_job else {
            return Ok(());
        };

        let Some(error_message) = self.rebuild_stale_reason(active_job.id) else {
            return Ok(());
        };

        self.fail_rebuild_submission(active_job.id, namespace, &error_message)?;
        self.runtime.unregister_rebuild_worker(active_job.id);
        Ok(())
    }

    fn recover_stale_rebuild_jobs(&self) {
        let active_jobs = {
            let services = match self.services.lock() {
                Ok(services) => services,
                Err(_) => return,
            };
            match services
                .repository
                .list_active_maintenance_jobs("rebuild", DEFAULT_NAMESPACE)
            {
                Ok(jobs) => jobs,
                Err(_) => return,
            }
        };

        for job in active_jobs {
            if let Some(error_message) = self.rebuild_stale_reason(job.id) {
                let _ = self.fail_rebuild_submission(job.id, &job.namespace, &error_message);
                self.runtime.unregister_rebuild_worker(job.id);
            }
        }
    }

    fn touch_rebuild_worker(&self, job_id: i64) {
        if let Some(heartbeat) = self.runtime.rebuild_worker(job_id) {
            heartbeat.touch();
        }
    }

    fn rebuild_stale_reason(&self, job_id: i64) -> Option<String> {
        let Some(heartbeat) = self.runtime.rebuild_worker(job_id) else {
            return Some("rebuild worker missing from runtime registry".to_owned());
        };

        if heartbeat.age() > self.runtime_config.rebuild_stale_after {
            return Some(format!(
                "rebuild worker watchdog timeout after {:?}",
                self.runtime_config.rebuild_stale_after
            ));
        }

        None
    }

    fn recover_stale_repair_worker(&self) {
        let Some(heartbeat) = self.runtime.repair_worker() else {
            let _ = self.spawn_repair_worker();
            return;
        };

        if heartbeat.age() <= self.runtime_config.repair_stale_after {
            return;
        }

        let _ = recover_running_repair_tasks_runtime(
            &self.repair_db_path,
            DEFAULT_NAMESPACE,
            REPAIR_WORKER_CONFIG.max_retries,
            "repair worker watchdog recovered stalled task",
        );
        let _ = self.spawn_repair_worker();
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
        if job.status != "queued" && job.status != "running" {
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
            if job.status != "running" {
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

        let fallback_index_state =
            derive_index_state(&services.repository).map_err(AppStateError::Storage)?;
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

        let fallback_index_state =
            derive_index_state(repository).map_err(AppStateError::Storage)?;
        repository
            .set_schema_meta_value("index_state", &fallback_index_state)
            .map_err(AppStateError::Storage)?;

        Ok(())
    }
}

impl ServerRepairTaskExecutor {
    fn execute_index_insert(
        &mut self,
        task: &RepairTask,
        payload: IndexInsertRepairPayload,
    ) -> Result<(), String> {
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
        let index_state = derive_index_state(&repository)
            .map_err(|error| format!("failed to derive repair index state: {error}"))?;
        sync_runtime_schema_meta(&mut repository, &self.embedding_provider, &index_state)?;

        Ok(())
    }

    fn execute_index_delete(
        &mut self,
        task: &RepairTask,
        payload: IndexDeleteRepairPayload,
    ) -> Result<(), String> {
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
        let index_state = derive_index_state(&repository)
            .map_err(|error| format!("failed to derive repair index state: {error}"))?;
        sync_runtime_schema_meta(&mut repository, &self.embedding_provider, &index_state)?;

        Ok(())
    }
}

impl RepairTaskExecutor for ServerRepairTaskExecutor {
    fn execute(&mut self, task: &RepairTask) -> Result<(), String> {
        #[cfg(test)]
        {
            self.test_hooks.maybe_panic_repair();
            self.test_hooks.maybe_block_repair();
        }

        match task.task_type.as_str() {
            "index_insert" => {
                let payload = parse_repair_payload::<IndexInsertRepairPayload>(task)?;
                self.execute_index_insert(task, payload)
            }
            "index_delete" => {
                let payload = parse_repair_payload::<IndexDeleteRepairPayload>(task)?;
                self.execute_index_delete(task, payload)
            }
            other => Err(format!("unsupported repair task_type: {other}")),
        }
    }
}

fn run_repair_worker_loop(
    db_path: String,
    embedding_provider: StubEmbeddingProvider,
    vector_index: Arc<Mutex<InMemoryVectorIndex>>,
    runtime: Arc<RuntimeState>,
    heartbeat: Arc<RepairWorkerHeartbeat>,
    runtime_config: RuntimeConfig,
    #[cfg(test)] test_hooks: Arc<RuntimeTestHooks>,
) {
    let mut current_heartbeat = heartbeat;
    let mut restart_backoff = RepairRestartBackoff::new(
        runtime_config.repair_restart_backoff_initial,
        runtime_config.repair_restart_backoff_max,
    );
    loop {
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            run_repair_worker_generation(
                &db_path,
                &embedding_provider,
                &vector_index,
                Arc::clone(&current_heartbeat),
                runtime_config
                    .repair_watchdog_interval
                    .min(runtime_config.repair_stale_after),
                &mut restart_backoff,
                #[cfg(test)]
                Arc::clone(&test_hooks),
            )
        }));

        let recovery_error = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(payload) => Some(format!(
                "repair worker panic recovered by supervisor: {}",
                panic_payload_to_string(payload.as_ref())
            )),
        };

        if let Some(error_message) = recovery_error {
            let _ = recover_running_repair_tasks_runtime(
                &db_path,
                DEFAULT_NAMESPACE,
                REPAIR_WORKER_CONFIG.max_retries,
                &error_message,
            );
            thread::sleep(restart_backoff.next_delay());
            current_heartbeat = runtime.register_repair_worker();
        }
    }
}

fn run_repair_worker_generation(
    db_path: &str,
    embedding_provider: &StubEmbeddingProvider,
    vector_index: &Arc<Mutex<InMemoryVectorIndex>>,
    heartbeat: Arc<RepairWorkerHeartbeat>,
    idle_heartbeat_interval: Duration,
    restart_backoff: &mut RepairRestartBackoff,
    #[cfg(test)] test_hooks: Arc<RuntimeTestHooks>,
) -> Result<(), String> {
    loop {
        heartbeat.touch();
        let mut repository = open_runtime_repository(db_path)?;
        let mut executor = ServerRepairTaskExecutor {
            db_path: db_path.to_owned(),
            embedding_provider: embedding_provider.clone(),
            vector_index: Arc::clone(vector_index),
            #[cfg(test)]
            test_hooks: Arc::clone(&test_hooks),
        };
        let mut worker =
            match RepairWorker::new(&mut repository, &mut executor, REPAIR_WORKER_CONFIG) {
                Ok(worker) => worker,
                Err(error) => return Err(format!("failed to initialize repair worker: {error}")),
            };

        loop {
            heartbeat.touch();
            match worker.run_once(DEFAULT_NAMESPACE) {
                Ok(result) => {
                    restart_backoff.reset();
                    heartbeat.touch();
                    if result.scanned == 0 {
                        sleep_with_repair_heartbeat(
                            heartbeat.as_ref(),
                            REPAIR_POLL_INTERVAL,
                            idle_heartbeat_interval,
                        );
                    }
                }
                Err(error) => {
                    heartbeat.touch();
                    return Err(format!("repair worker run_once failed: {error}"));
                }
            }
        }
    }
}

fn sleep_with_repair_heartbeat(
    heartbeat: &RepairWorkerHeartbeat,
    total_sleep: Duration,
    step: Duration,
) {
    let mut slept = Duration::ZERO;
    while slept < total_sleep {
        let remaining = total_sleep.saturating_sub(slept);
        let slice = remaining.min(step);
        thread::sleep(slice);
        heartbeat.touch();
        slept += slice;
    }
}

fn recover_running_repair_tasks_runtime(
    db_path: &str,
    namespace: &str,
    max_retry_count: u32,
    last_error: &str,
) -> Result<usize, String> {
    let mut repository = open_runtime_repository(db_path)?;
    repository
        .recover_running_repair_tasks(namespace, max_retry_count, last_error)
        .map_err(|error| format!("failed to recover running repair tasks: {error}"))
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

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }

    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }

    "unknown panic payload".to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use seahorse_core::IngestRequest as CoreIngestRequest;

    use super::{
        open_runtime_repository, AppState, MaintenanceJob, RepairRestartBackoff, RuntimeConfig,
        RuntimeTestHooks, DEFAULT_NAMESPACE,
    };
    use seahorse_core::RebuildScope;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);
    const JOB_POLL_ATTEMPTS: usize = 500;
    const JOB_POLL_SLEEP_MS: u64 = 20;

    #[test]
    fn rebuild_worker_panic_marks_job_failed() {
        let db_path = unique_db_path("rebuild-worker-panic");
        let hooks = Arc::new(RuntimeTestHooks::default());
        hooks.trigger_rebuild_panic();

        let state = AppState::new_with_runtime_for_tests(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
            RuntimeConfig::fast_for_tests(),
            Arc::clone(&hooks),
        )
        .expect("create app state");
        seed_rebuild_dataset(&state, "rebuild-worker-panic");

        let job = state
            .rebuild(
                seahorse_core::RebuildRequest {
                    namespace: DEFAULT_NAMESPACE.to_owned(),
                    scope: RebuildScope::All,
                },
                false,
            )
            .expect("submit rebuild");
        let terminal = wait_for_job(&state, job.id);

        assert_eq!(terminal.status, "failed");
        assert!(terminal
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("panic"));

        cleanup_db_path(&db_path);
    }

    #[test]
    fn rebuild_watchdog_marks_stalled_job_failed() {
        let db_path = unique_db_path("rebuild-watchdog-timeout");
        let hooks = Arc::new(RuntimeTestHooks::default());
        hooks.block_rebuild_for(Duration::from_millis(250));

        let state = AppState::new_with_runtime_for_tests(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
            RuntimeConfig::fast_for_tests(),
            Arc::clone(&hooks),
        )
        .expect("create app state");
        seed_rebuild_dataset(&state, "rebuild-watchdog-timeout");

        let job = state
            .rebuild(
                seahorse_core::RebuildRequest {
                    namespace: DEFAULT_NAMESPACE.to_owned(),
                    scope: RebuildScope::All,
                },
                false,
            )
            .expect("submit rebuild");
        let terminal = wait_for_job(&state, job.id);

        assert_eq!(terminal.status, "failed");
        let error_message = terminal.error_message.as_deref().unwrap_or_default();
        assert!(error_message.contains("watchdog") || error_message.contains("runtime registry"));

        cleanup_db_path(&db_path);
    }

    #[test]
    fn rebuild_submission_recovers_orphaned_active_job_immediately() {
        let db_path = unique_db_path("rebuild-submit-recovery");
        let hooks = Arc::new(RuntimeTestHooks::default());
        hooks.block_next_rebuild_for(Duration::from_millis(250));

        let state = AppState::new_with_runtime_for_tests(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
            RuntimeConfig::submission_recovery_for_tests(),
            Arc::clone(&hooks),
        )
        .expect("create app state");
        seed_rebuild_dataset(&state, "rebuild-submit-recovery");

        let first_job = state
            .rebuild(
                seahorse_core::RebuildRequest {
                    namespace: DEFAULT_NAMESPACE.to_owned(),
                    scope: RebuildScope::All,
                },
                false,
            )
            .expect("submit first rebuild");
        state.runtime.unregister_rebuild_worker(first_job.id);

        let second_job = state
            .rebuild(
                seahorse_core::RebuildRequest {
                    namespace: DEFAULT_NAMESPACE.to_owned(),
                    scope: RebuildScope::All,
                },
                false,
            )
            .expect("submit second rebuild");

        assert_ne!(first_job.id, second_job.id);

        let first_terminal = wait_for_job(&state, first_job.id);
        assert_eq!(first_terminal.status, "failed");
        assert!(first_terminal
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("runtime registry"));

        cleanup_db_path(&db_path);
    }

    #[test]
    fn healthy_idle_repair_worker_does_not_restart() {
        let db_path = unique_db_path("repair-idle-heartbeat");
        let hooks = Arc::new(RuntimeTestHooks::default());

        let state = AppState::new_with_runtime_for_tests(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
            RuntimeConfig::repair_fast_for_tests(),
            Arc::clone(&hooks),
        )
        .expect("create app state");

        let initial_generation = state.current_repair_generation_for_tests();
        thread::sleep(Duration::from_millis(250));
        let current_generation = state.current_repair_generation_for_tests();

        assert_eq!(current_generation, initial_generation);

        cleanup_db_path(&db_path);
    }

    #[test]
    fn repair_worker_panic_recovers_running_task_and_restarts() {
        let db_path = unique_db_path("repair-panic-restart");
        let hooks = Arc::new(RuntimeTestHooks::default());
        hooks.trigger_repair_panic();

        let state = AppState::new_with_runtime_for_tests(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
            RuntimeConfig::repair_fast_for_tests(),
            Arc::clone(&hooks),
        )
        .expect("create app state");
        seed_rebuild_dataset(&state, "repair-panic-restart");

        let initial_generation = state.current_repair_generation_for_tests();
        let chunk_id = first_chunk_id(&db_path);
        let task_id = enqueue_index_delete_repair_task(&db_path, chunk_id, "panic-repair");

        let terminal_task = wait_for_repair_task_terminal(&db_path, task_id);
        assert_ne!(terminal_task.status, "running");

        let restarted_generation = wait_for_repair_generation_change(&state, initial_generation);
        assert!(restarted_generation > initial_generation);

        cleanup_db_path(&db_path);
    }

    #[test]
    fn repair_watchdog_recovers_stalled_task_and_restarts() {
        let db_path = unique_db_path("repair-watchdog-stall");
        let hooks = Arc::new(RuntimeTestHooks::default());
        hooks.block_next_repair_for(Duration::from_millis(250));

        let state = AppState::new_with_runtime_for_tests(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
            RuntimeConfig::repair_fast_for_tests(),
            Arc::clone(&hooks),
        )
        .expect("create app state");
        seed_rebuild_dataset(&state, "repair-watchdog-stall");

        let initial_generation = state.current_repair_generation_for_tests();
        let chunk_id = first_chunk_id(&db_path);
        let task_id = enqueue_index_delete_repair_task(&db_path, chunk_id, "watchdog-repair");

        let terminal_task = wait_for_repair_task_terminal(&db_path, task_id);
        assert_ne!(terminal_task.status, "running");

        let restarted_generation = wait_for_repair_generation_change(&state, initial_generation);
        assert!(restarted_generation > initial_generation);

        cleanup_db_path(&db_path);
    }

    #[test]
    fn repair_restart_backoff_grows_and_resets() {
        let mut backoff =
            RepairRestartBackoff::new(Duration::from_millis(10), Duration::from_millis(40));

        assert_eq!(backoff.next_delay(), Duration::from_millis(10));
        assert_eq!(backoff.next_delay(), Duration::from_millis(20));
        assert_eq!(backoff.next_delay(), Duration::from_millis(40));
        assert_eq!(backoff.next_delay(), Duration::from_millis(40));

        backoff.reset();

        assert_eq!(backoff.next_delay(), Duration::from_millis(10));
    }

    fn seed_rebuild_dataset(state: &AppState, prefix: &str) {
        for index in 0..8 {
            let mut ingest_request = CoreIngestRequest::new(heavy_rebuild_content(prefix, index));
            ingest_request.filename = format!("{prefix}-{index}.txt");
            state.ingest(ingest_request).expect("seed ingest");
        }
    }

    fn wait_for_job(state: &AppState, job_id: i64) -> MaintenanceJob {
        let mut last_status = String::new();
        for _ in 0..JOB_POLL_ATTEMPTS {
            let job = state.get_job(job_id).expect("load job");
            last_status = job.status.clone();
            if job.status == "succeeded" || job.status == "failed" || job.status == "cancelled" {
                return job;
            }

            thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
        }

        panic!("job {job_id} did not reach terminal status in time (last_status={last_status})");
    }

    fn wait_for_repair_task_terminal(db_path: &PathBuf, task_id: i64) -> seahorse_core::RepairTask {
        let mut last_status = String::new();
        for _ in 0..JOB_POLL_ATTEMPTS {
            let repository = match open_runtime_repository(
                db_path
                    .to_str()
                    .expect("temp db path must be valid unicode"),
            ) {
                Ok(repository) => repository,
                Err(_) => {
                    thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
                    continue;
                }
            };
            let task = repository
                .get_repair_task(task_id)
                .expect("load repair task")
                .expect("repair task exists");
            last_status = task.status.clone();
            if task.status != "running" && task.status != "pending" {
                return task;
            }

            thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
        }

        panic!("repair task {task_id} did not reach terminal status in time (last_status={last_status})");
    }

    fn wait_for_repair_generation_change(state: &AppState, initial_generation: u64) -> u64 {
        for _ in 0..JOB_POLL_ATTEMPTS {
            let generation = state.current_repair_generation_for_tests();
            if generation > initial_generation {
                return generation;
            }

            thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
        }

        panic!("repair worker generation did not advance beyond {initial_generation} in time");
    }

    fn first_chunk_id(db_path: &PathBuf) -> i64 {
        let repository = open_runtime_repository(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
        )
        .expect("open runtime repository");
        repository
            .list_rebuild_chunks(DEFAULT_NAMESPACE)
            .expect("list rebuild chunks")
            .first()
            .expect("at least one chunk should exist")
            .chunk_id
    }

    fn enqueue_index_delete_repair_task(db_path: &PathBuf, chunk_id: i64, error: &str) -> i64 {
        let mut repository = open_runtime_repository(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
        )
        .expect("open runtime repository");
        repository
            .enqueue_repair_task(
                DEFAULT_NAMESPACE,
                "index_delete",
                "chunk",
                Some(chunk_id),
                Some(
                    &serde_json::json!({
                        "chunk_ids": [chunk_id],
                        "error": error,
                    })
                    .to_string(),
                ),
            )
            .expect("enqueue repair task")
    }

    fn unique_db_path(name: &str) -> PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_millis();
        std::env::temp_dir().join(format!("seahorse-state-{name}-{millis}-{counter}.db"))
    }

    fn cleanup_db_path(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
    }

    fn heavy_rebuild_content(prefix: &str, index: usize) -> String {
        let unit = format!(
            "{prefix}-{index} alpha beta gamma delta epsilon zeta eta theta iota kappa lambda "
        );
        unit.repeat(256)
    }
}
