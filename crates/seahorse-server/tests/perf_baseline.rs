use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use seahorse_core::{
    IngestRequest as CoreIngestRequest, MaintenanceJob, RebuildRequest as CoreRebuildRequest,
    RebuildScope, RecallRequest as CoreRecallRequest,
};
use seahorse_server::{config::ServerConfig, state::AppState};

const PERF_DOCUMENT_COUNT: usize = 200;
const PERF_RECALL_TOP_K: usize = 5;
const PERF_CHUNK_SIZE: usize = 128;
const DEFAULT_RECALL_P95_GATE_MS: f64 = 300.0;
const REBUILD_TIMEOUT_SECS: u64 = 300;
const JOB_POLL_SLEEP_MS: u64 = 10;

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PerfBaselineReport {
    chunk_count: usize,
    document_count: usize,
    chunks_per_document: usize,
    recall_sample_count: usize,
    record_only: bool,
    recall_p95_gate_ms: f64,
    ingest_total_ms: f64,
    recall_p50_ms: f64,
    recall_p95_ms: f64,
    rebuild_total_ms: f64,
}

#[derive(Debug, Clone, Copy)]
struct PerfGateConfig {
    record_only: bool,
    recall_p95_gate_ms: f64,
}

#[derive(Debug, Clone)]
struct PerfDocumentSpec {
    filename: String,
    content: String,
}

#[derive(Debug, Clone)]
struct PerfDataset {
    documents: Vec<PerfDocumentSpec>,
    recall_queries: Vec<String>,
    chunk_count: usize,
    document_count: usize,
    chunks_per_document: usize,
}

#[test]
#[ignore = "manual perf gate"]
fn perf_baseline_10k_chunks() {
    let report = run_perf_baseline(10_000, 200);
    println!("{report:#?}");
    if !report.record_only {
        assert!(
            report.recall_p95_ms < report.recall_p95_gate_ms,
            "p95 too high: {:.2}ms >= {:.2}ms",
            report.recall_p95_ms,
            report.recall_p95_gate_ms
        );
    }
}

fn run_perf_baseline(chunk_count: usize, recall_sample_count: usize) -> PerfBaselineReport {
    let gate_config = PerfGateConfig::from_env();
    let dataset = PerfDataset::new(chunk_count, recall_sample_count);
    let db_path = unique_db_path("perf-baseline");
    let state = build_state(&db_path);

    let ingest_started_at = Instant::now();
    for document in &dataset.documents {
        let mut request = CoreIngestRequest::new(document.content.clone());
        request.filename = document.filename.clone();
        request.options.chunk_size = PERF_CHUNK_SIZE;
        let result = state.ingest(request).expect("perf ingest should succeed");
        assert_eq!(result.ingest_status, "ready");
        assert_eq!(result.index_status, "ready");
        assert!(result.repair_task_id.is_none());
    }
    let ingest_total_ms = elapsed_ms(ingest_started_at);

    let recall_latencies = dataset
        .recall_queries
        .iter()
        .map(|query| {
            let mut request = CoreRecallRequest::new(query.clone());
            request.top_k = PERF_RECALL_TOP_K;

            let started_at = Instant::now();
            let result = state.recall(request).expect("perf recall should succeed");
            let latency_ms = elapsed_ms(started_at);

            assert!(
                !result.results.is_empty(),
                "perf recall should return at least one hit"
            );
            latency_ms
        })
        .collect::<Vec<_>>();

    let rebuild_started_at = Instant::now();
    let job = state
        .rebuild(
            CoreRebuildRequest {
                namespace: "default".to_owned(),
                scope: RebuildScope::All,
            },
            false,
        )
        .expect("perf rebuild should enqueue");
    let completed_job = wait_for_terminal_job(&state, job.id);
    assert_eq!(completed_job.status, "succeeded");
    let rebuild_total_ms = elapsed_ms(rebuild_started_at);

    let stats = state.stats_snapshot().expect("perf stats should load");
    assert_eq!(stats.chunk_count, dataset.chunk_count);
    assert_eq!(stats.index_status, "ready");

    cleanup_db_path(&db_path);

    PerfBaselineReport {
        chunk_count: dataset.chunk_count,
        document_count: dataset.document_count,
        chunks_per_document: dataset.chunks_per_document,
        recall_sample_count: dataset.recall_queries.len(),
        record_only: gate_config.record_only,
        recall_p95_gate_ms: gate_config.recall_p95_gate_ms,
        ingest_total_ms,
        recall_p50_ms: percentile_ms(&recall_latencies, 0.50),
        recall_p95_ms: percentile_ms(&recall_latencies, 0.95),
        rebuild_total_ms,
    }
}

impl PerfGateConfig {
    fn from_env() -> Self {
        Self {
            record_only: parse_env_flag(std::env::var("SEAHORSE_PERF_RECORD_ONLY").ok().as_deref()),
            recall_p95_gate_ms: parse_env_f64(
                std::env::var("SEAHORSE_PERF_RECALL_P95_MS_MAX")
                    .ok()
                    .as_deref(),
                DEFAULT_RECALL_P95_GATE_MS,
            ),
        }
    }
}

impl PerfDataset {
    fn new(chunk_count: usize, recall_sample_count: usize) -> Self {
        assert!(chunk_count > 0, "chunk_count must be greater than zero");
        assert!(
            recall_sample_count > 0,
            "recall_sample_count must be greater than zero"
        );
        assert!(
            chunk_count >= recall_sample_count,
            "chunk_count must be >= recall_sample_count"
        );
        assert_eq!(
            chunk_count % PERF_DOCUMENT_COUNT,
            0,
            "chunk_count must be divisible by {PERF_DOCUMENT_COUNT}"
        );

        let chunks_per_document = chunk_count / PERF_DOCUMENT_COUNT;
        let mut documents = Vec::with_capacity(PERF_DOCUMENT_COUNT);
        let mut sampled_queries = Vec::with_capacity(recall_sample_count);
        let sample_stride = (chunk_count / recall_sample_count).max(1);

        for document_index in 0..PERF_DOCUMENT_COUNT {
            let mut content = String::with_capacity(chunks_per_document * PERF_CHUNK_SIZE);
            for chunk_index in 0..chunks_per_document {
                let global_chunk_index = document_index * chunks_per_document + chunk_index;
                let chunk_text = build_chunk_text(
                    global_chunk_index,
                    document_index,
                    chunk_index,
                    PERF_CHUNK_SIZE,
                );
                if global_chunk_index % sample_stride == 0
                    && sampled_queries.len() < recall_sample_count
                {
                    sampled_queries.push(chunk_text.clone());
                }
                content.push_str(&chunk_text);
            }

            documents.push(PerfDocumentSpec {
                filename: format!("perf-{document_index:04}.txt"),
                content,
            });
        }

        while sampled_queries.len() < recall_sample_count {
            let fallback_index = sampled_queries.len();
            sampled_queries.push(build_chunk_text(
                fallback_index,
                0,
                fallback_index,
                PERF_CHUNK_SIZE,
            ));
        }

        Self {
            documents,
            recall_queries: sampled_queries,
            chunk_count,
            document_count: PERF_DOCUMENT_COUNT,
            chunks_per_document,
        }
    }
}

fn build_state(db_path: &Path) -> AppState {
    let mut config = ServerConfig::default();
    config.storage.db_path = db_path.to_string_lossy().into_owned();
    AppState::new_with_config(&config).expect("create perf app state")
}

fn build_chunk_text(
    global_chunk_index: usize,
    document_index: usize,
    chunk_index: usize,
    chunk_size: usize,
) -> String {
    let prefix =
        format!("doc-{document_index:04}|chunk-{chunk_index:03}|global-{global_chunk_index:05}|");
    let fill = format!("payload-{global_chunk_index:05}|");
    let mut text = prefix;
    while text.len() < chunk_size {
        text.push_str(&fill);
    }
    text.truncate(chunk_size);
    text
}

fn wait_for_terminal_job(state: &AppState, job_id: i64) -> MaintenanceJob {
    let mut last_status = String::new();
    let started_at = Instant::now();
    while started_at.elapsed() < Duration::from_secs(REBUILD_TIMEOUT_SECS) {
        let job = state.get_job(job_id).expect("load perf maintenance job");
        last_status = job.status.clone();
        if matches!(job.status.as_str(), "succeeded" | "failed" | "cancelled") {
            return job;
        }

        thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
    }

    panic!(
        "job {job_id} did not reach terminal state within {REBUILD_TIMEOUT_SECS}s (last_status={last_status})"
    );
}

fn elapsed_ms(started_at: Instant) -> f64 {
    started_at.elapsed().as_secs_f64() * 1_000.0
}

fn percentile_ms(samples: &[f64], percentile: f64) -> f64 {
    assert!(!samples.is_empty(), "samples must not be empty");
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));

    let percentile = percentile.clamp(0.0, 1.0);
    let rank = ((sorted.len() as f64 * percentile).ceil() as usize).saturating_sub(1);
    sorted[rank.min(sorted.len() - 1)]
}

fn parse_env_flag(value: Option<&str>) -> bool {
    match value {
        Some(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        ),
        None => false,
    }
}

fn parse_env_f64(value: Option<&str>, default_value: f64) -> f64 {
    value
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .unwrap_or(default_value)
}

fn unique_db_path(name: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis();
    std::env::temp_dir().join(format!("seahorse-{name}-{millis}.db"))
}

fn cleanup_db_path(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

#[test]
fn perf_gate_env_parsers_support_record_only_overrides() {
    assert!(parse_env_flag(Some("1")));
    assert!(parse_env_flag(Some("true")));
    assert!(!parse_env_flag(Some("0")));
    assert!(!parse_env_flag(None));
    assert_eq!(
        parse_env_f64(Some("123.5"), DEFAULT_RECALL_P95_GATE_MS),
        123.5
    );
    assert_eq!(
        parse_env_f64(Some("not-a-number"), DEFAULT_RECALL_P95_GATE_MS),
        DEFAULT_RECALL_P95_GATE_MS
    );
}
