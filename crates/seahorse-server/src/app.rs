use axum::{
    routing::{get, post},
    Router,
};

use crate::{config::ObservabilityConfig, handlers, state::AppState};

pub fn build_app(state: AppState) -> Router {
    let observability_config = ObservabilityConfig::default();
    build_app_with_observability(state, &observability_config)
}

pub fn build_app_with_observability(
    state: AppState,
    observability_config: &ObservabilityConfig,
) -> Router {
    let mut app = Router::new()
        .route("/ingest", post(handlers::ingest::post_ingest))
        .route("/recall", post(handlers::recall::post_recall))
        .route("/forget", post(handlers::forget::post_forget))
        .route("/admin/rebuild", post(handlers::rebuild::post_rebuild))
        .route("/admin/jobs/:job_id", get(handlers::jobs::get_job))
        .route("/live", get(handlers::live::get_live))
        .route("/ready", get(handlers::ready::get_ready))
        .route("/stats", get(handlers::stats::get_stats))
        .route("/health", get(handlers::health::get_health));

    if observability_config.enable_metrics {
        app = app.route(
            &observability_config.metrics_path,
            get(handlers::metrics::get_metrics),
        );
    }

    app.with_state(state).route_layer(axum::middleware::from_fn(
        crate::api::observability::request_context_middleware,
    ))
}

pub fn build_test_app(_name: &str) -> Router {
    let state = AppState::new_with_db_path(":memory:").expect("create app state");
    build_app(state)
}

pub fn build_test_app_with_observability(
    _name: &str,
    observability_config: &ObservabilityConfig,
) -> Router {
    let state = AppState::new_with_db_path(":memory:").expect("create app state");
    build_app_with_observability(state, observability_config)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use http_body_util::BodyExt as _;
    use rusqlite::Connection;
    use seahorse_core::{IngestRequest as CoreIngestRequest, SqliteRepository};
    use serde_json::{json, Value};
    use tower::util::ServiceExt;

    use super::{build_app, build_app_with_observability};
    use crate::{
        config::{ObservabilityConfig, ServerConfig},
        state::{AppState, AppStateTestOptions, RuntimeIndexFaultConfig},
    };

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);
    const JOB_POLL_ATTEMPTS: usize = 500;
    const JOB_POLL_SLEEP_MS: u64 = 20;

    #[tokio::test]
    async fn rebuild_endpoint_creates_job_and_job_query_reaches_succeeded() {
        let (state, db_path) = test_state("rebuild-success");
        seed_rebuild_dataset(&state, "rebuild-success");

        let app = build_app(state);
        let response = post_rebuild_request(app.clone(), false)
            .await
            .expect("execute rebuild request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(true));
        assert_eq!(body["data"]["status"], Value::String("queued".to_owned()));

        let job_id = body["data"]["job_id"]
            .as_str()
            .expect("job_id should be string")
            .to_owned();
        let final_body = poll_job_until_terminal(app, &job_id).await;

        assert_eq!(final_body["success"], Value::Bool(true));
        assert_eq!(
            final_body["data"]["status"],
            Value::String("succeeded".to_owned())
        );
        assert_eq!(
            final_body["data"]["job_type"],
            Value::String("rebuild".to_owned())
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn duplicate_rebuild_request_reuses_active_job_when_force_is_false() {
        let (state, db_path) = test_state("rebuild-duplicate");
        seed_rebuild_dataset(&state, "duplicate");

        let app = build_app(state);
        let first_body = read_json_body(
            post_rebuild_request(app.clone(), false)
                .await
                .expect("submit first rebuild"),
        )
        .await;
        let second_body = read_json_body(
            post_rebuild_request(app.clone(), false)
                .await
                .expect("submit duplicate rebuild"),
        )
        .await;

        let first_job_id = first_body["data"]["job_id"]
            .as_str()
            .expect("first job id")
            .to_owned();
        let second_job_id = second_body["data"]["job_id"]
            .as_str()
            .expect("second job id")
            .to_owned();

        assert_eq!(first_job_id, second_job_id);

        let final_body = poll_job_until_terminal(app, &first_job_id).await;
        assert_eq!(
            final_body["data"]["status"],
            Value::String("succeeded".to_owned())
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn force_rebuild_request_cancels_previous_active_job_and_creates_new_job() {
        let (state, db_path) = test_state("rebuild-force");
        seed_rebuild_dataset(&state, "force");

        let app = build_app(state);
        let first_body = read_json_body(
            post_rebuild_request(app.clone(), false)
                .await
                .expect("submit first rebuild"),
        )
        .await;
        let second_body = read_json_body(
            post_rebuild_request(app.clone(), true)
                .await
                .expect("submit forced rebuild"),
        )
        .await;

        let first_job_id = first_body["data"]["job_id"]
            .as_str()
            .expect("first job id")
            .to_owned();
        let second_job_id = second_body["data"]["job_id"]
            .as_str()
            .expect("second job id")
            .to_owned();

        assert_ne!(first_job_id, second_job_id);

        let first_terminal = poll_job_until_terminal(app.clone(), &first_job_id).await;
        let second_terminal = poll_job_until_terminal(app, &second_job_id).await;

        assert_eq!(
            first_terminal["data"]["status"],
            Value::String("cancelled".to_owned())
        );
        assert_eq!(
            second_terminal["data"]["status"],
            Value::String("succeeded".to_owned())
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn startup_recovers_queued_rebuild_job_and_completes_it() {
        let (state, db_path) = test_state("rebuild-recovery");
        seed_rebuild_dataset(&state, "recovery");
        drop(state);

        let job_id = enqueue_rebuild_job(&db_path, "all", false);
        let recovered_state = AppState::new_with_db_path(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
        )
        .expect("restart app state");
        let app = build_app(recovered_state);

        let final_body = poll_job_until_terminal(app, &format!("job-{job_id}")).await;

        assert_eq!(final_body["success"], Value::Bool(true));
        assert_eq!(
            final_body["data"]["status"],
            Value::String("succeeded".to_owned())
        );
        assert_eq!(
            final_body["data"]["job_id"],
            Value::String(format!("job-{job_id}"))
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn startup_recovers_running_rebuild_job_and_completes_it() {
        let (state, db_path) = test_state("rebuild-running-recovery");
        seed_rebuild_dataset(&state, "running-recovery");
        drop(state);

        let job_id = enqueue_rebuild_job(&db_path, "all", true);
        let recovered_state = AppState::new_with_db_path(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
        )
        .expect("restart app state");
        let app = build_app(recovered_state);

        let final_body = poll_job_until_terminal(app, &format!("job-{job_id}")).await;

        assert_eq!(final_body["success"], Value::Bool(true));
        assert_eq!(
            final_body["data"]["status"],
            Value::String("succeeded".to_owned())
        );
        assert_eq!(
            final_body["data"]["job_id"],
            Value::String(format!("job-{job_id}"))
        );
        assert!(final_body["data"]["started_at"].is_string());

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn startup_recovery_cancels_stale_active_rebuild_jobs_and_resumes_latest() {
        let (state, db_path) = test_state("rebuild-multi-recovery");
        seed_rebuild_dataset(&state, "multi-recovery");
        drop(state);

        let stale_job_id = enqueue_rebuild_job(&db_path, "all", true);
        let latest_job_id = enqueue_rebuild_job(&db_path, "all", false);
        let recovered_state = AppState::new_with_db_path(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
        )
        .expect("restart app state");
        let app = build_app(recovered_state);

        let latest_body =
            poll_job_until_terminal(app.clone(), &format!("job-{latest_job_id}")).await;
        let stale_body = poll_job_until_terminal(app, &format!("job-{stale_job_id}")).await;

        assert_eq!(latest_body["success"], Value::Bool(true));
        assert_eq!(
            latest_body["data"]["status"],
            Value::String("succeeded".to_owned())
        );
        assert_eq!(
            latest_body["data"]["job_id"],
            Value::String(format!("job-{latest_job_id}"))
        );

        assert_eq!(stale_body["success"], Value::Bool(true));
        assert_eq!(
            stale_body["data"]["status"],
            Value::String("cancelled".to_owned())
        );
        assert_eq!(
            stale_body["data"]["job_id"],
            Value::String(format!("job-{stale_job_id}"))
        );
        assert!(stale_body["data"]["error_message"]
            .as_str()
            .unwrap_or_default()
            .contains("superseded during startup recovery"));

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn startup_marks_invalid_rebuild_payload_job_failed() {
        let (state, db_path) = test_state("rebuild-invalid-payload");
        seed_rebuild_dataset(&state, "invalid-payload");
        drop(state);

        let job_id = enqueue_raw_rebuild_job(
            &db_path,
            json!({
                "scope": "not-supported",
                "force": false
            })
            .to_string(),
            false,
        );
        let recovered_state = AppState::new_with_db_path(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
        )
        .expect("restart app state");
        let app = build_app(recovered_state);

        let final_body = poll_job_until_terminal(app, &format!("job-{job_id}")).await;

        assert_eq!(final_body["success"], Value::Bool(true));
        assert_eq!(
            final_body["data"]["status"],
            Value::String("failed".to_owned())
        );
        assert!(final_body["data"]["error_message"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid rebuild job scope"));

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn jobs_endpoint_returns_not_found_for_unknown_job_id() {
        let (state, db_path) = test_state("job-not-found");
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/admin/jobs/job-999999")
                    .body(Body::empty())
                    .expect("build get job request"),
            )
            .await
            .expect("execute get job request");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(false));
        assert_eq!(
            body["error"]["code"],
            Value::String("INVALID_INPUT".to_owned())
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn forget_endpoint_rejects_hard_mode_at_api_boundary() {
        let (state, db_path) = test_state("forget-hard-mode");
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/forget")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "namespace": "default",
                            "chunk_ids": [1],
                            "mode": "hard"
                        })
                        .to_string(),
                    ))
                    .expect("build forget request"),
            )
            .await
            .expect("execute forget request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(false));
        assert_eq!(
            body["error"]["code"],
            Value::String("INVALID_INPUT".to_owned())
        );
        assert_eq!(
            body["error"]["message"],
            Value::String("mode must be soft; got hard".to_owned())
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn recall_endpoint_rejects_unknown_mode_at_api_boundary() {
        let (state, db_path) = test_state("recall-non-basic-mode");
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/recall")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "namespace": "default",
                            "query": "alpha",
                            "mode": "semantic"
                        })
                        .to_string(),
                    ))
                    .expect("build recall request"),
            )
            .await
            .expect("execute recall request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(false));
        assert_eq!(
            body["error"]["code"],
            Value::String("INVALID_INPUT".to_owned())
        );
        assert_eq!(
            body["error"]["message"],
            Value::String("mode must be one of basic, tagmemo; got semantic".to_owned())
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn recall_endpoint_accepts_explicit_basic_mode() {
        let (state, db_path) = test_state("recall-basic-mode");
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/recall")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "namespace": "default",
                            "query": "alpha",
                            "mode": "basic"
                        })
                        .to_string(),
                    ))
                    .expect("build recall request"),
            )
            .await
            .expect("execute recall request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(true));
        assert_eq!(body["error"], Value::Null);
        assert!(body["data"]["results"].is_array());
        assert_eq!(
            body["data"]["metadata"]["worldview"],
            Value::String("default".to_owned())
        );
        assert!(body["data"]["metadata"]["entropy"].is_number());

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn recall_endpoint_accepts_tagmemo_mode() {
        let (state, db_path) = test_state("recall-tagmemo-mode");

        let mut connectome_request = CoreIngestRequest::new("project rust anchor".to_owned());
        connectome_request.filename = "connectome.txt".to_owned();
        connectome_request.tags = vec!["project".to_owned(), "rust".to_owned()];
        state.ingest(connectome_request).expect("seed connectome");

        let mut associated_request = CoreIngestRequest::new("rust compiler deep dive".to_owned());
        associated_request.filename = "rust.txt".to_owned();
        associated_request.tags = vec!["rust".to_owned()];
        state
            .ingest(associated_request)
            .expect("seed associated chunk");

        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/recall")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "namespace": "default",
                            "query": "project",
                            "mode": "tagmemo"
                        })
                        .to_string(),
                    ))
                    .expect("build recall request"),
            )
            .await
            .expect("execute recall request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(true));
        assert_eq!(body["error"], Value::Null);
        assert!(body["data"]["results"].is_array());

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn metrics_endpoint_respects_observability_config() {
        let (state, db_path) = test_state("metrics-configured-path");
        let app = build_app_with_observability(
            state,
            &ObservabilityConfig {
                enable_metrics: true,
                metrics_path: "/internal/metrics".to_owned(),
                ..ObservabilityConfig::default()
            },
        );

        let default_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("build default metrics request"),
            )
            .await
            .expect("execute default metrics request");
        assert_eq!(default_response.status(), StatusCode::NOT_FOUND);

        let custom_response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/metrics")
                    .body(Body::empty())
                    .expect("build custom metrics request"),
            )
            .await
            .expect("execute custom metrics request");

        assert_eq!(custom_response.status(), StatusCode::OK);
        assert_eq!(
            custom_response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain; version=0.0.4")
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn metrics_endpoint_is_absent_when_disabled() {
        let (state, db_path) = test_state("metrics-disabled");
        let app = build_app_with_observability(
            state,
            &ObservabilityConfig {
                enable_metrics: false,
                metrics_path: "/internal/metrics".to_owned(),
                ..ObservabilityConfig::default()
            },
        );

        let default_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("build default metrics request"),
            )
            .await
            .expect("execute default metrics request");
        assert_eq!(default_response.status(), StatusCode::NOT_FOUND);

        let custom_response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/metrics")
                    .body(Body::empty())
                    .expect("build custom metrics request"),
            )
            .await
            .expect("execute custom metrics request");
        assert_eq!(custom_response.status(), StatusCode::NOT_FOUND);

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn ready_endpoint_returns_service_unavailable_when_index_state_is_unavailable() {
        let (state, db_path) = test_state("ready-unavailable");
        set_index_state(&db_path, "unavailable");
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("build ready request"),
            )
            .await
            .expect("execute ready request");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(false));
        assert_eq!(
            body["error"]["code"],
            Value::String("INDEX_UNAVAILABLE".to_owned())
        );

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn live_endpoint_returns_process_liveness_contract() {
        let (state, db_path) = test_state("live-ok");
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/live")
                    .body(Body::empty())
                    .expect("build live request"),
            )
            .await
            .expect("execute live request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = read_json_body(response).await;
        assert_eq!(body["success"], Value::Bool(true));
        assert_eq!(body["data"]["status"], Value::String("ok".to_owned()));
        assert!(body["data"]["version"].is_string());

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn metrics_endpoint_exposes_repair_and_rebuild_status_breakdowns() {
        let (state, db_path) = test_state_with_options(
            "metrics-status-breakdown",
            AppStateTestOptions::default()
                .with_spawn_repair_worker(false)
                .with_runtime_index_faults(RuntimeIndexFaultConfig::default().fail_insert_always()),
        );
        state
            .ingest(CoreIngestRequest::new(
                "metrics status breakdown alpha beta gamma".to_owned(),
            ))
            .expect("seed failed index ingest");
        enqueue_rebuild_job(&db_path, "all", false);
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("build metrics request"),
            )
            .await
            .expect("execute metrics request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = read_text_body(response).await;
        assert!(body.contains("seahorse_repair_queue_tasks{status=\"pending\"} 1"));
        assert!(body.contains("seahorse_rebuild_jobs{status=\"queued\"} 1"));

        cleanup_db_path(&db_path);
    }

    #[tokio::test]
    async fn metrics_endpoint_exposes_repair_and_rebuild_age_gauges() {
        let (state, db_path) = test_state_with_options(
            "metrics-age-gauges",
            AppStateTestOptions::default()
                .with_spawn_repair_worker(false)
                .with_runtime_index_faults(RuntimeIndexFaultConfig::default().fail_insert_always()),
        );
        state
            .ingest(CoreIngestRequest::new(
                "metrics age gauges alpha beta gamma".to_owned(),
            ))
            .expect("seed failed index ingest");
        enqueue_rebuild_job(&db_path, "all", false);
        age_repair_tasks(&db_path, 90);
        age_rebuild_jobs(&db_path, 150);
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("build metrics request"),
            )
            .await
            .expect("execute metrics request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = read_text_body(response).await;
        assert_metric_value_at_least(&body, "seahorse_repair_oldest_task_age_seconds", 90.0);
        assert_metric_value_at_least(
            &body,
            "seahorse_rebuild_oldest_active_job_age_seconds",
            150.0,
        );

        cleanup_db_path(&db_path);
    }

    fn test_state(name: &str) -> (AppState, PathBuf) {
        let db_path = unique_db_path(name);
        let state = AppState::new_with_db_path(
            db_path
                .to_str()
                .expect("temp db path must be valid unicode"),
        )
        .expect("create app state");

        (state, db_path)
    }

    fn test_state_with_options(name: &str, options: AppStateTestOptions) -> (AppState, PathBuf) {
        let db_path = unique_db_path(name);
        let mut config = ServerConfig::default();
        config.storage.db_path = db_path
            .to_str()
            .expect("temp db path must be valid unicode")
            .to_owned();
        let state = AppState::new_with_test_options(&config, options).expect("create app state");

        (state, db_path)
    }

    fn seed_rebuild_dataset(state: &AppState, prefix: &str) {
        for index in 0..8 {
            let mut ingest_request = CoreIngestRequest::new(heavy_rebuild_content(prefix, index));
            ingest_request.filename = format!("{prefix}-{index}.txt");
            state.ingest(ingest_request).expect("seed ingest");
        }
    }

    async fn poll_job_until_terminal(app: axum::Router, job_id: &str) -> Value {
        let mut last_status = String::new();
        for _ in 0..JOB_POLL_ATTEMPTS {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri(format!("/admin/jobs/{job_id}"))
                        .body(Body::empty())
                        .expect("build get job request"),
                )
                .await
                .expect("execute get job request");

            let body = read_json_body(response).await;
            let status = body["data"]["status"].as_str().unwrap_or_default();
            last_status = status.to_owned();
            if status == "succeeded" || status == "failed" || status == "cancelled" {
                return body;
            }

            thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
        }

        panic!("job {job_id} did not reach terminal status in time (last_status={last_status})");
    }

    async fn read_json_body(response: axum::response::Response) -> Value {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect response body")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("parse response body as json")
    }

    async fn read_text_body(response: axum::response::Response) -> String {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect response body")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("parse response body as text")
    }

    async fn post_rebuild_request(
        app: axum::Router,
        force: bool,
    ) -> Result<axum::response::Response, std::convert::Infallible> {
        app.oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/admin/rebuild")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "namespace": "default",
                        "scope": "all",
                        "force": force
                    })
                    .to_string(),
                ))
                .expect("build rebuild request"),
        )
        .await
    }

    fn enqueue_rebuild_job(db_path: &PathBuf, scope: &str, mark_running: bool) -> i64 {
        enqueue_raw_rebuild_job(
            db_path,
            json!({
                "scope": scope,
                "force": false
            })
            .to_string(),
            mark_running,
        )
    }

    fn enqueue_raw_rebuild_job(db_path: &PathBuf, payload_json: String, mark_running: bool) -> i64 {
        let connection = Connection::open(db_path).expect("open sqlite db for recovery job");
        let mut repository = SqliteRepository::new(connection).expect("create repository");
        let job = repository
            .create_maintenance_job("rebuild", "default", Some(&payload_json))
            .expect("enqueue rebuild job");

        if mark_running {
            repository
                .mark_maintenance_job_running(job.id, Some("0/unknown"))
                .expect("mark rebuild job running");
        }

        job.id
    }

    fn unique_db_path(name: &str) -> PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_millis();
        std::env::temp_dir().join(format!("seahorse-{name}-{millis}-{counter}.db"))
    }

    fn cleanup_db_path(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
    }

    fn set_index_state(db_path: &PathBuf, value: &str) {
        let connection = Connection::open(db_path).expect("open sqlite db for health mutation");
        let mut repository = SqliteRepository::new(connection).expect("create repository");
        repository
            .set_schema_meta_value("index_state", value)
            .expect("set index_state");
    }

    fn age_repair_tasks(db_path: &PathBuf, age_seconds: u64) {
        let connection = Connection::open(db_path).expect("open sqlite db for repair aging");
        connection
            .execute(
                "UPDATE repair_queue
                 SET created_at = datetime('now', ?1)",
                [format!("-{age_seconds} seconds")],
            )
            .expect("age repair tasks");
    }

    fn age_rebuild_jobs(db_path: &PathBuf, age_seconds: u64) {
        let connection = Connection::open(db_path).expect("open sqlite db for rebuild aging");
        connection
            .execute(
                "UPDATE maintenance_jobs
                 SET created_at = datetime('now', ?1)
                 WHERE job_type = 'rebuild'
                   AND namespace = 'default'",
                [format!("-{age_seconds} seconds")],
            )
            .expect("age rebuild jobs");
    }

    fn assert_metric_value_at_least(body: &str, metric_name: &str, min_value: f64) {
        let line = body
            .lines()
            .find(|line| line.starts_with(metric_name))
            .unwrap_or_else(|| panic!("missing metric line for {metric_name}"));
        let value = line
            .split_whitespace()
            .last()
            .unwrap_or_else(|| panic!("missing metric value for {metric_name}"))
            .parse::<f64>()
            .unwrap_or_else(|error| panic!("invalid metric value for {metric_name}: {error}"));
        assert!(
            value >= min_value,
            "expected {metric_name} >= {min_value}, got {value}"
        );
    }

    fn heavy_rebuild_content(prefix: &str, index: usize) -> String {
        let unit = format!(
            "{prefix}-{index} alpha beta gamma delta epsilon zeta eta theta iota kappa lambda "
        );
        unit.repeat(256)
    }
}
