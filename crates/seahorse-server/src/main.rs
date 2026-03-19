mod api;
mod handlers;
mod state;

use axum::{
    routing::{get, post},
    Router,
};

use state::AppState;

#[tokio::main]
async fn main() {
    let state = AppState::new().expect("failed to initialize seahorse application state");
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(listen_addr())
        .await
        .expect("failed to bind seahorse server listener");

    axum::serve(listener, app)
        .await
        .expect("seahorse server failed");
}

fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/ingest", post(handlers::ingest::post_ingest))
        .route("/recall", post(handlers::recall::post_recall))
        .route("/forget", post(handlers::forget::post_forget))
        .route("/admin/rebuild", post(handlers::rebuild::post_rebuild))
        .route("/admin/jobs/{job_id}", get(handlers::jobs::get_job))
        .route("/stats", get(handlers::stats::get_stats))
        .route("/health", get(handlers::health::get_health))
        .with_state(state)
}

fn listen_addr() -> String {
    std::env::var("SEAHORSE_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use http_body_util::BodyExt as _;
    use rusqlite::Connection;
    use seahorse_core::{IngestRequest as CoreIngestRequest, SqliteRepository};
    use serde_json::{json, Value};
    use tower::util::ServiceExt;

    use super::build_app;
    use crate::state::AppState;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

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
        assert!(
            final_body["data"]["error_message"]
                .as_str()
                .unwrap_or_default()
                .contains("invalid rebuild job scope")
        );

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

    fn seed_rebuild_dataset(state: &AppState, prefix: &str) {
        for index in 0..8 {
            let mut ingest_request =
                CoreIngestRequest::new(heavy_rebuild_content(prefix, index));
            ingest_request.filename = format!("{prefix}-{index}.txt");
            state.ingest(ingest_request).expect("seed ingest");
        }
    }

    async fn poll_job_until_terminal(app: axum::Router, job_id: &str) -> Value {
        for _ in 0..100 {
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
            if status == "succeeded" || status == "failed" || status == "cancelled" {
                return body;
            }

            thread::sleep(Duration::from_millis(20));
        }

        panic!("job {job_id} did not reach terminal status in time");
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

    fn heavy_rebuild_content(prefix: &str, index: usize) -> String {
        let unit = format!(
            "{prefix}-{index} alpha beta gamma delta epsilon zeta eta theta iota kappa lambda "
        );
        unit.repeat(256)
    }
}
