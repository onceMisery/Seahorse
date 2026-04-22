use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use seahorse_core::{
    ForgetRequest as CoreForgetRequest, IngestRequest as CoreIngestRequest, MaintenanceJob,
    RebuildRequest as CoreRebuildRequest, RebuildScope, SqliteRepository,
};
use seahorse_server::{
    config::ServerConfig,
    state::{AppState, AppStateTestOptions, RuntimeIndexFaultConfig},
};
use serde_json::json;

static TEST_DB_COUNTER: AtomicU64 = AtomicU64::new(1);
const JOB_POLL_ATTEMPTS: usize = 500;
const JOB_POLL_SLEEP_MS: u64 = 20;

#[test]
fn repair_worker_recovers_single_insert_failure_to_ready() {
    let db_path = unique_db_path("repair-once");
    let state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default()
            .with_spawn_repair_worker(false)
            .with_runtime_index_faults(RuntimeIndexFaultConfig::default().fail_insert_times(1)),
    );

    let ingest = state
        .ingest(ingest_request(
            "repair-once.txt",
            "fault recovery insert once",
        ))
        .expect("ingest should succeed with queued repair");
    let repair_task_id = ingest.repair_task_id.expect("repair task id");

    assert_eq!(ingest.ingest_status, "partial");
    assert_eq!(ingest.index_status, "pending_repair");

    let repository = open_repository(&db_path);
    let pending = repository
        .get_repair_task(repair_task_id)
        .expect("load pending repair task")
        .expect("repair task should exist");
    assert_eq!(pending.status, "pending");

    let worker_result = state
        .run_repair_worker_once_for_tests()
        .expect("repair worker should run");
    assert_eq!(worker_result.scanned, 1);
    assert_eq!(worker_result.succeeded, 1);

    let repository = open_repository(&db_path);
    let repaired = repository
        .get_repair_task(repair_task_id)
        .expect("load repaired task")
        .expect("repaired task should exist");
    assert_eq!(repaired.status, "succeeded");

    let file = repository
        .find_file_by_hash("default", &ingest.file_hash)
        .expect("find ingested file")
        .expect("ingested file should exist");
    assert_eq!(file.ingest_status, "ready");

    let chunks = repository
        .list_chunks_by_file_id(file.id)
        .expect("list file chunks");
    assert!(!chunks.is_empty(), "expected persisted chunks");
    assert!(chunks.iter().all(|chunk| chunk.index_status == "ready"));

    let stats = state.stats_snapshot().expect("load stats");
    assert_eq!(stats.repair_queue_size, 0);
    assert_eq!(stats.index_status, "ready");

    cleanup_db_path(&db_path);
}

#[test]
fn repair_worker_deadletters_after_repeated_insert_failures() {
    let db_path = unique_db_path("repair-deadletter");
    let state = build_state(
        &db_path,
        2,
        AppStateTestOptions::default()
            .with_spawn_repair_worker(false)
            .with_runtime_index_faults(RuntimeIndexFaultConfig::default().fail_insert_always()),
    );

    let ingest = state
        .ingest(ingest_request(
            "repair-deadletter.txt",
            "fault recovery deadletter path",
        ))
        .expect("ingest should queue repair");
    let repair_task_id = ingest.repair_task_id.expect("repair task id");

    let first_run = state
        .run_repair_worker_once_for_tests()
        .expect("first repair worker run");
    assert_eq!(first_run.failed, 1);

    let repository = open_repository(&db_path);
    let failed = repository
        .get_repair_task(repair_task_id)
        .expect("load failed task")
        .expect("failed task should exist");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.retry_count, 1);

    let second_run = state
        .run_repair_worker_once_for_tests()
        .expect("second repair worker run");
    assert_eq!(second_run.deadlettered, 1);

    let repository = open_repository(&db_path);
    let deadlettered = repository
        .get_repair_task(repair_task_id)
        .expect("load deadletter task")
        .expect("deadletter task should exist");
    assert_eq!(deadlettered.status, "deadletter");
    assert_eq!(deadlettered.retry_count, 2);

    let file = repository
        .find_file_by_hash("default", &ingest.file_hash)
        .expect("find ingested file")
        .expect("ingested file should exist");
    assert_eq!(file.ingest_status, "partial");
    let chunks = repository
        .list_chunks_by_file_id(file.id)
        .expect("list file chunks");
    assert!(chunks.iter().all(|chunk| chunk.index_status == "failed"));
    assert_eq!(
        repository
            .get_schema_meta_value("index_state")
            .expect("load index_state")
            .as_deref(),
        Some("degraded")
    );

    let stats = state.stats_snapshot().expect("load stats");
    assert_eq!(stats.repair_queue_size, 1);
    assert_eq!(stats.index_status, "degraded");

    cleanup_db_path(&db_path);
}

#[test]
fn startup_recovers_running_repair_task_and_retries_it() {
    let db_path = unique_db_path("repair-running-recovery");
    let initial_state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default()
            .with_spawn_repair_worker(false)
            .with_runtime_index_faults(RuntimeIndexFaultConfig::default().fail_insert_times(1)),
    );

    let ingest = initial_state
        .ingest(ingest_request(
            "repair-running-recovery.txt",
            "fault recovery running task recovery",
        ))
        .expect("ingest should queue repair");
    let repair_task_id = ingest.repair_task_id.expect("repair task id");

    let mut repository = open_repository(&db_path);
    let claimed = repository
        .claim_next_repair_task("default", 3)
        .expect("claim repair task")
        .expect("repair task should be claimable");
    assert_eq!(claimed.id, repair_task_id);
    assert_eq!(claimed.status, "running");
    drop(repository);
    drop(initial_state);

    let recovered_state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );

    let repository = open_repository(&db_path);
    let recovered = repository
        .get_repair_task(repair_task_id)
        .expect("load recovered task")
        .expect("recovered task should exist");
    assert_eq!(recovered.status, "failed");
    assert_eq!(recovered.retry_count, 1);
    assert_eq!(
        recovered.last_error.as_deref(),
        Some("repair task recovered after unclean shutdown")
    );

    let retry_run = recovered_state
        .run_repair_worker_once_for_tests()
        .expect("retry repair worker run");
    assert_eq!(retry_run.succeeded, 1);

    let repository = open_repository(&db_path);
    let repaired = repository
        .get_repair_task(repair_task_id)
        .expect("load repaired task after recovery")
        .expect("repaired task should exist");
    assert_eq!(repaired.status, "succeeded");
    assert_eq!(repaired.retry_count, 1);
    assert_eq!(repaired.last_error, None);

    cleanup_db_path(&db_path);
}

#[test]
fn startup_recovery_keeps_only_latest_active_rebuild_job() {
    let db_path = unique_db_path("rebuild-latest-only");
    let state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );
    seed_rebuild_dataset(&state, "startup-recovery");
    drop(state);

    let stale_job_id = enqueue_rebuild_job(&db_path, "all", true);
    let latest_job_id = enqueue_rebuild_job(&db_path, "all", false);

    let recovered_state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );

    let latest = wait_for_terminal_job(&recovered_state, latest_job_id);
    let stale = recovered_state
        .get_job(stale_job_id)
        .expect("load stale rebuild job");

    assert_eq!(latest.status, "succeeded");
    assert_eq!(stale.status, "cancelled");
    assert!(stale
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("superseded during startup recovery"));

    cleanup_db_path(&db_path);
}

#[test]
fn rebuild_failure_restores_index_state_from_rebuilding_to_degraded() {
    let db_path = unique_db_path("rebuild-fallback");
    let state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default()
            .with_spawn_repair_worker(false)
            .with_runtime_index_faults(
                RuntimeIndexFaultConfig::default()
                    .fail_insert_always()
                    .fail_rebuild_times(1),
            ),
    );

    let ingest = state
        .ingest(ingest_request(
            "rebuild-fallback.txt",
            "fault recovery rebuild fallback content",
        ))
        .expect("ingest should queue repair");
    assert_eq!(ingest.ingest_status, "partial");

    let job = state
        .rebuild(
            CoreRebuildRequest {
                namespace: "default".to_owned(),
                scope: RebuildScope::All,
            },
            false,
        )
        .expect("rebuild should enqueue job");
    let failed_job = wait_for_terminal_job(&state, job.id);

    assert_eq!(failed_job.status, "failed");

    let repository = open_repository(&db_path);
    assert_eq!(
        repository
            .get_schema_meta_value("index_state")
            .expect("load index_state")
            .as_deref(),
        Some("degraded")
    );

    cleanup_db_path(&db_path);
}

#[test]
fn repair_worker_rebuilds_connectome_after_forget() {
    let db_path = unique_db_path("connectome-repair");
    let state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );

    let mut connectome_request = ingest_request("connectome.txt", "project rust anchor");
    connectome_request.tags = vec!["project".to_owned(), "rust".to_owned()];
    let connectome_ingest = state
        .ingest(connectome_request)
        .expect("seed connectome file");

    let mut associated_request = ingest_request("rust.txt", "rust compiler deep dive");
    associated_request.tags = vec!["rust".to_owned()];
    state
        .ingest(associated_request)
        .expect("seed associated file");

    let forget_result = state
        .forget(CoreForgetRequest::for_file(connectome_ingest.file_id))
        .expect("forget should succeed");
    assert_eq!(forget_result.index_cleanup_status, "completed");
    assert!(forget_result.repair_task_id.is_none());

    let repository = open_repository(&db_path);
    let stale_neighbors = repository
        .list_connectome_neighbors("default", "project", 10)
        .expect("load stale connectome neighbors");
    assert!(
        stale_neighbors.iter().any(|edge| edge.target_tag == "rust"),
        "forget should leave stale connectome edge until repair worker rebuilds it"
    );
    let pending_task = repository
        .find_active_repair_task("default", "connectome_rebuild", "namespace")
        .expect("find active connectome rebuild task")
        .expect("connectome rebuild task should exist");
    assert_eq!(pending_task.status, "pending");
    drop(repository);

    let worker_result = state
        .run_repair_worker_once_for_tests()
        .expect("repair worker should run connectome rebuild");
    assert_eq!(worker_result.scanned, 1);
    assert_eq!(worker_result.succeeded, 1);

    let repository = open_repository(&db_path);
    let repaired_task = repository
        .get_repair_task(pending_task.id)
        .expect("load repaired connectome task")
        .expect("repaired connectome task should exist");
    assert_eq!(repaired_task.status, "succeeded");

    let rebuilt_neighbors = repository
        .list_connectome_neighbors("default", "project", 10)
        .expect("load rebuilt connectome neighbors");
    assert!(
        rebuilt_neighbors
            .iter()
            .all(|edge| edge.target_tag != "rust"),
        "connectome rebuild should remove stale edge from deleted file"
    );

    let stats = state
        .stats_snapshot()
        .expect("load stats after connectome rebuild");
    assert_eq!(stats.repair_queue_size, 0);

    cleanup_db_path(&db_path);
}

#[test]
fn startup_enqueues_connectome_repair_when_edges_are_missing() {
    let db_path = unique_db_path("connectome-startup-bootstrap");
    let state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );

    let mut connectome_request = ingest_request("connectome.txt", "project rust anchor");
    connectome_request.tags = vec!["project".to_owned(), "rust".to_owned()];
    state
        .ingest(connectome_request)
        .expect("seed connectome file");
    drop(state);

    let connection = Connection::open(&db_path).expect("open sqlite db for connectome mutation");
    connection
        .execute("DELETE FROM connectome WHERE namespace = 'default'", [])
        .expect("delete connectome rows");
    drop(connection);

    let recovered_state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );

    let repository = open_repository(&db_path);
    let pending_task = repository
        .find_active_repair_task("default", "connectome_rebuild", "namespace")
        .expect("find startup connectome repair task")
        .expect("startup connectome repair task should exist");
    assert_eq!(pending_task.status, "pending");
    assert_eq!(
        pending_task.payload_json.as_deref(),
        Some("{\"deleted_chunk_ids\":[],\"reason\":\"startup_connectome_drift\"}")
    );
    let missing_neighbors = repository
        .list_connectome_neighbors("default", "project", 10)
        .expect("load missing connectome neighbors");
    assert!(
        missing_neighbors.is_empty(),
        "connectome should remain empty until repair worker runs"
    );
    drop(repository);

    let worker_result = recovered_state
        .run_repair_worker_once_for_tests()
        .expect("repair worker should rebuild startup connectome");
    assert_eq!(worker_result.scanned, 1);
    assert_eq!(worker_result.succeeded, 1);

    let repository = open_repository(&db_path);
    let rebuilt_neighbors = repository
        .list_connectome_neighbors("default", "project", 10)
        .expect("load rebuilt connectome neighbors");
    assert!(
        rebuilt_neighbors
            .iter()
            .any(|edge| edge.target_tag == "rust"),
        "startup repair should restore missing connectome edge"
    );

    cleanup_db_path(&db_path);
}

#[test]
fn startup_enqueues_connectome_repair_when_counts_drift() {
    let db_path = unique_db_path("connectome-startup-drift");
    let state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );

    let mut first_request = ingest_request("connectome-1.txt", "project rust first");
    first_request.tags = vec!["project".to_owned(), "rust".to_owned()];
    state
        .ingest(first_request)
        .expect("seed first connectome file");

    let mut second_request = ingest_request("connectome-2.txt", "project rust second");
    second_request.tags = vec!["project".to_owned(), "rust".to_owned()];
    state
        .ingest(second_request)
        .expect("seed second connectome file");
    drop(state);

    let connection = Connection::open(&db_path).expect("open sqlite db for connectome drift");
    connection
        .execute(
            "UPDATE connectome
             SET cooccur_count = 1,
                 weight = 1.0
             WHERE namespace = 'default'",
            [],
        )
        .expect("mutate connectome drift");
    drop(connection);

    let recovered_state = build_state(
        &db_path,
        3,
        AppStateTestOptions::default().with_spawn_repair_worker(false),
    );

    let repository = open_repository(&db_path);
    let pending_task = repository
        .find_active_repair_task("default", "connectome_rebuild", "namespace")
        .expect("find startup drift repair task")
        .expect("startup drift repair task should exist");
    assert_eq!(pending_task.status, "pending");
    drop(repository);

    let worker_result = recovered_state
        .run_repair_worker_once_for_tests()
        .expect("repair worker should rebuild drifted connectome");
    assert_eq!(worker_result.scanned, 1);
    assert_eq!(worker_result.succeeded, 1);

    let repository = open_repository(&db_path);
    let rebuilt_neighbors = repository
        .list_connectome_neighbors("default", "project", 10)
        .expect("load rebuilt drift neighbors");
    let rust_edge = rebuilt_neighbors
        .iter()
        .find(|edge| edge.target_tag == "rust")
        .expect("rebuilt rust edge should exist");
    assert_eq!(rust_edge.cooccur_count, 2);
    assert_eq!(rust_edge.weight, 2.0);

    cleanup_db_path(&db_path);
}

fn build_state(db_path: &Path, max_retries: u32, options: AppStateTestOptions) -> AppState {
    let mut config = ServerConfig::default();
    config.storage.db_path = db_path.to_string_lossy().into_owned();
    config.jobs.repair_max_retries = max_retries;
    config.jobs.repair_batch_size = 1;

    AppState::new_with_test_options(&config, options).expect("create fault recovery state")
}

fn ingest_request(filename: &str, content: &str) -> CoreIngestRequest {
    let mut request = CoreIngestRequest::new(content.to_owned());
    request.filename = filename.to_owned();
    request
}

fn seed_rebuild_dataset(state: &AppState, prefix: &str) {
    for index in 0..8 {
        state
            .ingest(ingest_request(
                &format!("{prefix}-{index}.txt"),
                &heavy_rebuild_content(prefix, index),
            ))
            .expect("seed ingest");
    }
}

fn wait_for_terminal_job(state: &AppState, job_id: i64) -> MaintenanceJob {
    let mut last_status = String::new();
    for _ in 0..JOB_POLL_ATTEMPTS {
        let job = state.get_job(job_id).expect("load maintenance job");
        last_status = job.status.clone();
        if matches!(job.status.as_str(), "succeeded" | "failed" | "cancelled") {
            return job;
        }

        thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
    }

    panic!("job {job_id} did not reach terminal state (last_status={last_status})");
}

fn enqueue_rebuild_job(db_path: &Path, scope: &str, mark_running: bool) -> i64 {
    let connection = Connection::open(db_path).expect("open sqlite db for rebuild job");
    let mut repository = SqliteRepository::new(connection).expect("create repository");
    let job = repository
        .create_maintenance_job(
            "rebuild",
            "default",
            Some(
                &json!({
                    "scope": scope,
                    "force": false
                })
                .to_string(),
            ),
        )
        .expect("create rebuild job");

    if mark_running {
        repository
            .mark_maintenance_job_running(job.id, Some("0/unknown"))
            .expect("mark rebuild job running");
    }

    job.id
}

fn open_repository(db_path: &Path) -> SqliteRepository {
    let connection = Connection::open(db_path).expect("open sqlite db");
    SqliteRepository::new(connection).expect("create repository")
}

fn unique_db_path(name: &str) -> PathBuf {
    let counter = TEST_DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis();
    std::env::temp_dir().join(format!(
        "seahorse-fault-recovery-{name}-{millis}-{counter}.db"
    ))
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
