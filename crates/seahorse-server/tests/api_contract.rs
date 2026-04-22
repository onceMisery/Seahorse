use std::collections::BTreeSet;
use std::thread;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt as _;
use seahorse_server::app::build_test_app;
use serde_json::{json, Map, Value};
use tower::util::ServiceExt;

const JOB_POLL_ATTEMPTS: usize = 500;
const JOB_POLL_SLEEP_MS: u64 = 20;
const INGEST_STATUSES: &[&str] = &["pending_index", "ready", "partial", "deleted"];
const INGEST_INDEX_STATUSES: &[&str] = &["ready", "pending_repair"];
const FORGET_INDEX_CLEANUP_STATUSES: &[&str] = &["pending", "running", "completed"];
const RUNTIME_INDEX_STATUSES: &[&str] = &["ready", "rebuilding", "degraded", "unavailable"];
const HEALTH_STATUSES: &[&str] = &["ok", "degraded", "failed"];

#[tokio::test]
async fn json_endpoints_match_formal_response_envelope_contract() {
    let app = build_test_app("api-contract-envelope");

    let ingest_body = json_request(
        &app,
        Method::POST,
        "/ingest",
        Some(json!({
            "namespace": "default",
            "content": "contract envelope alpha beta gamma",
            "source": {
                "type": "inline",
                "filename": "contract.txt"
            },
            "tags": ["contract", "alpha"],
            "metadata": {
                "suite": "api_contract"
            }
        })),
    )
    .await;
    assert_success_envelope(&ingest_body);

    let chunk_ids = ingest_body["data"]["chunk_ids"]
        .as_array()
        .expect("ingest chunk_ids should be array")
        .iter()
        .map(|value| value.as_i64().expect("chunk_id should be integer"))
        .collect::<Vec<_>>();
    assert!(
        !chunk_ids.is_empty(),
        "ingest should create at least one chunk"
    );
    assert!(ingest_body["data"]["file_id"].is_i64());
    assert_string_enum(
        &ingest_body["data"]["ingest_status"],
        INGEST_STATUSES,
        "data.ingest_status",
    );
    assert_string_enum(
        &ingest_body["data"]["index_status"],
        INGEST_INDEX_STATUSES,
        "data.index_status",
    );

    let recall_body = json_request(
        &app,
        Method::POST,
        "/recall",
        Some(json!({
            "namespace": "default",
            "query": "contract alpha",
            "mode": "basic"
        })),
    )
    .await;
    assert_success_envelope(&recall_body);
    assert!(recall_body["data"]["results"].is_array());
    assert!(recall_body["data"]["metadata"].is_object());

    let stats_body = json_request(&app, Method::GET, "/stats", None).await;
    assert_success_envelope(&stats_body);
    assert!(stats_body["data"]["chunk_count"].is_u64());
    assert!(stats_body["data"]["tag_count"].is_u64());
    assert!(stats_body["data"]["deleted_chunk_count"].is_u64());
    assert!(stats_body["data"]["repair_queue_size"].is_u64());
    assert_string_enum(
        &stats_body["data"]["index_status"],
        RUNTIME_INDEX_STATUSES,
        "data.index_status",
    );

    let health_body = json_request(&app, Method::GET, "/health", None).await;
    assert_success_envelope(&health_body);
    assert_string_enum(
        &health_body["data"]["status"],
        HEALTH_STATUSES,
        "data.status",
    );
    assert!(health_body["data"]["db"].is_string());
    assert!(health_body["data"]["index"].is_string());
    assert!(health_body["data"]["embedding_provider"].is_string());
    assert!(health_body["data"]["version"].is_string());

    let ready_body = json_request(&app, Method::GET, "/ready", None).await;
    assert_success_envelope(&ready_body);
    assert_string_enum(
        &ready_body["data"]["status"],
        HEALTH_STATUSES,
        "data.status",
    );

    let live_body = json_request(&app, Method::GET, "/live", None).await;
    assert_success_envelope(&live_body);
    assert_eq!(live_body["data"]["status"], Value::String("ok".to_owned()));
    assert!(live_body["data"]["version"].is_string());

    let forget_body = json_request(
        &app,
        Method::POST,
        "/forget",
        Some(json!({
            "namespace": "default",
            "chunk_ids": chunk_ids,
            "mode": "soft"
        })),
    )
    .await;
    assert_success_envelope(&forget_body);
    assert!(forget_body["data"]["affected_chunks"].is_u64());
    assert_string_enum(
        &forget_body["data"]["index_cleanup_status"],
        FORGET_INDEX_CLEANUP_STATUSES,
        "data.index_cleanup_status",
    );

    let rebuild_body = json_request(
        &app,
        Method::POST,
        "/admin/rebuild",
        Some(json!({
            "namespace": "default",
            "scope": "all",
            "force": false
        })),
    )
    .await;
    assert_success_envelope(&rebuild_body);
    let job_id = rebuild_body["data"]["job_id"]
        .as_str()
        .expect("rebuild job_id should be string")
        .to_owned();
    assert_eq!(
        rebuild_body["data"]["status"],
        Value::String("queued".to_owned())
    );
    assert!(rebuild_body["data"]["submitted_at"].is_string());

    let job_body = poll_job_until_terminal(&app, &job_id).await;
    assert_success_envelope(&job_body);
    assert_eq!(job_body["data"]["job_id"], Value::String(job_id));
    assert_eq!(
        job_body["data"]["job_type"],
        Value::String("rebuild".to_owned())
    );
    assert_eq!(
        job_body["data"]["status"],
        Value::String("succeeded".to_owned())
    );
}

#[tokio::test]
async fn json_error_responses_match_formal_response_envelope_contract() {
    let app = build_test_app("api-contract-error-envelope");

    assert_invalid_input_error(
        &app,
        Method::POST,
        "/ingest",
        Some(json!({
            "namespace": "other",
            "content": "invalid namespace"
        })),
        StatusCode::BAD_REQUEST,
        "only namespace=default is supported",
    )
    .await;

    assert_invalid_input_error(
        &app,
        Method::POST,
        "/recall",
        Some(json!({
            "namespace": "default",
            "query": "contract alpha",
            "mode": "semantic"
        })),
        StatusCode::BAD_REQUEST,
        "mode must be one of basic, tagmemo; got semantic",
    )
    .await;

    let (status, body) = json_request_with_status(
        &app,
        Method::POST,
        "/recall",
        Some(json!({
            "namespace": "default",
            "query": "contract alpha",
            "mode": "basic",
            "timeout_ms": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_error_envelope(&body);
    assert_eq!(body["error"]["code"], Value::String("TIMEOUT".to_owned()));
    assert_eq!(body["error"]["retryable"], Value::Bool(true));

    assert_invalid_input_error(
        &app,
        Method::POST,
        "/forget",
        Some(json!({
            "namespace": "default",
            "chunk_ids": [1],
            "mode": "hard"
        })),
        StatusCode::BAD_REQUEST,
        "mode must be soft; got hard",
    )
    .await;

    assert_invalid_input_error(
        &app,
        Method::POST,
        "/admin/rebuild",
        Some(json!({
            "namespace": "default",
            "scope": "full"
        })),
        StatusCode::BAD_REQUEST,
        "scope must be all or missing_index; got full",
    )
    .await;

    assert_invalid_input_error(
        &app,
        Method::GET,
        "/admin/jobs/job-0",
        None,
        StatusCode::BAD_REQUEST,
        "job_id must be a positive integer; got job-0",
    )
    .await;
}

#[tokio::test]
async fn metrics_endpoint_matches_formal_text_contract() {
    let app = build_test_app("api-contract-metrics");

    let health_status = raw_request_status(&app, Method::GET, "/health", None).await;
    assert_eq!(health_status, StatusCode::OK);
    let ready_status = raw_request_status(&app, Method::GET, "/ready", None).await;
    assert_eq!(ready_status, StatusCode::OK);
    let live_status = raw_request_status(&app, Method::GET, "/live", None).await;
    assert_eq!(live_status, StatusCode::OK);

    let response = raw_request(&app, Method::GET, "/metrics", None).await;
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = read_text_body(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("text/plain; version=0.0.4"));
    assert!(body.contains("# HELP seahorse_http_requests_total"));
    assert!(body.contains("seahorse_http_requests_total{scope=\"total\"}"));
    assert!(body.contains("seahorse_chunk_count "));
    assert!(body.contains("seahorse_health_status{status=\"ok\"} 1"));
    assert!(body.contains("seahorse_repair_queue_tasks{status=\"pending\"}"));
    assert!(body.contains("seahorse_rebuild_jobs{status=\"queued\"}"));
    assert!(body.contains("seahorse_repair_oldest_task_age_seconds"));
    assert!(body.contains("seahorse_rebuild_oldest_active_job_age_seconds"));
}

async fn json_request(app: &Router, method: Method, uri: &str, body: Option<Value>) -> Value {
    let (_, value) = json_request_with_status(app, method, uri, body).await;
    value
}

async fn json_request_with_status(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = raw_request(app, method, uri, body).await;
    let status = response.status();
    let body = read_json_body(response).await;
    (status, body)
}

async fn raw_request_status(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> StatusCode {
    raw_request(app, method, uri, body).await.status()
}

async fn raw_request(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let mut request = Request::builder().method(method).uri(uri);
    let body = if let Some(body) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };

    app.clone()
        .oneshot(request.body(body).expect("build request"))
        .await
        .expect("execute request")
}

async fn poll_job_until_terminal(app: &Router, job_id: &str) -> Value {
    let mut last_status = String::new();
    for _ in 0..JOB_POLL_ATTEMPTS {
        let body = json_request(app, Method::GET, &format!("/admin/jobs/{job_id}"), None).await;

        let status = body["data"]["status"].as_str().unwrap_or_default();
        last_status = status.to_owned();
        if status == "succeeded" || status == "failed" || status == "cancelled" {
            return body;
        }

        thread::sleep(Duration::from_millis(JOB_POLL_SLEEP_MS));
    }

    panic!("job {job_id} did not reach terminal state (last_status={last_status})");
}

async fn read_json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&read_body_bytes(response).await).expect("parse json body")
}

async fn read_text_body(response: axum::response::Response) -> String {
    String::from_utf8(read_body_bytes(response).await).expect("parse utf-8 text body")
}

async fn read_body_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes()
        .to_vec()
}

async fn assert_invalid_input_error(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    expected_status: StatusCode,
    expected_message: &str,
) {
    let (status, body) = json_request_with_status(app, method, uri, body).await;

    assert_eq!(status, expected_status);
    assert_error_envelope(&body);
    assert_eq!(
        body["error"]["code"],
        Value::String("INVALID_INPUT".to_owned())
    );
    assert_eq!(
        body["error"]["message"],
        Value::String(expected_message.to_owned())
    );
    assert_eq!(body["error"]["retryable"], Value::Bool(false));
}

fn assert_success_envelope(body: &Value) {
    assert_envelope_keys(body);
    assert_eq!(body["success"], Value::Bool(true));
    assert!(body["data"].is_object());
    assert_eq!(body["error"], Value::Null);
    assert_request_id(body);
}

fn assert_error_envelope(body: &Value) {
    assert_envelope_keys(body);
    assert_eq!(body["success"], Value::Bool(false));
    assert_eq!(body["data"], Value::Null);
    assert_error_object_keys(
        body["error"]
            .as_object()
            .expect("error payload should be object"),
    );
    assert_request_id(body);
}

fn assert_envelope_keys(body: &Value) {
    let object = body
        .as_object()
        .expect("response envelope should be a JSON object");
    assert_keys(object, ["success", "data", "error", "request_id"]);
}

fn assert_error_object_keys(body: &Map<String, Value>) {
    assert_keys(body, ["code", "message", "retryable"]);
}

fn assert_request_id(body: &Value) {
    body["request_id"]
        .as_str()
        .expect("request_id should be string");
}

fn assert_string_enum(value: &Value, expected: &[&str], field: &str) {
    let actual = value
        .as_str()
        .unwrap_or_else(|| panic!("{field} should be string, got {value}"));
    assert!(
        expected.contains(&actual),
        "{field} should be one of {:?}, got {actual}",
        expected
    );
}

fn assert_keys<const N: usize>(object: &Map<String, Value>, expected: [&str; N]) {
    let actual = object.keys().cloned().collect::<BTreeSet<_>>();
    let expected = expected
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}
