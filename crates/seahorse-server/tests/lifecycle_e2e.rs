use std::thread;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt as _;
use serde_json::{json, Value};
use seahorse_server::app::build_test_app;
use tower::util::ServiceExt;

const JOB_POLL_ATTEMPTS: usize = 500;
const JOB_POLL_SLEEP_MS: u64 = 20;

#[tokio::test]
async fn ingest_recall_forget_rebuild_recall_roundtrip_preserves_soft_delete() {
    let app = build_test_app("lifecycle-e2e");

    let ingest_body = json_request(
        &app,
        Method::POST,
        "/ingest",
        Some(json!({
            "namespace": "default",
            "content": "lifecycle-orbit-sapphire signal retained before forget and absent after rebuild",
            "source": {
                "type": "inline",
                "filename": "lifecycle.txt"
            },
            "tags": ["lifecycle", "roundtrip"],
            "metadata": {
                "case": "lifecycle_e2e",
                "marker": "orbit-sapphire"
            }
        })),
    )
    .await;

    assert_eq!(ingest_body["success"], Value::Bool(true));
    let chunk_ids = ingest_body["data"]["chunk_ids"]
        .as_array()
        .expect("chunk_ids should be array")
        .iter()
        .map(|value| value.as_i64().expect("chunk_id should be integer"))
        .collect::<Vec<_>>();
    assert!(!chunk_ids.is_empty(), "ingest should create chunks");

    let recall_before_forget = json_request(
        &app,
        Method::POST,
        "/recall",
        Some(json!({
            "namespace": "default",
            "query": "orbit-sapphire",
            "mode": "basic",
            "top_k": 5
        })),
    )
    .await;

    assert_eq!(recall_before_forget["success"], Value::Bool(true));
    let results_before_forget = recall_before_forget["data"]["results"]
        .as_array()
        .expect("recall results should be array");
    assert!(
        !results_before_forget.is_empty(),
        "recall before forget should return seeded chunks"
    );
    assert!(results_before_forget.iter().any(|item| {
        item["tags"]
            .as_array()
            .map(|tags| tags.iter().any(|tag| tag == "lifecycle"))
            .unwrap_or(false)
    }));
    assert!(results_before_forget.iter().any(|item| {
        item["metadata"]["marker"] == Value::String("orbit-sapphire".to_owned())
    }));

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

    assert_eq!(forget_body["success"], Value::Bool(true));
    assert_eq!(
        forget_body["data"]["affected_chunks"],
        Value::from(chunk_ids.len() as u64)
    );

    let recall_after_forget = json_request(
        &app,
        Method::POST,
        "/recall",
        Some(json!({
            "namespace": "default",
            "query": "orbit-sapphire",
            "mode": "basic",
            "top_k": 5
        })),
    )
    .await;

    assert_eq!(recall_after_forget["success"], Value::Bool(true));
    assert_eq!(recall_after_forget["data"]["results"], Value::Array(Vec::new()));
    assert_eq!(recall_after_forget["data"]["metadata"]["result_count"], Value::from(0));

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

    assert_eq!(rebuild_body["success"], Value::Bool(true));
    let job_id = rebuild_body["data"]["job_id"]
        .as_str()
        .expect("job_id should be string")
        .to_owned();
    let job_body = poll_job_until_terminal(&app, &job_id).await;
    assert_eq!(job_body["success"], Value::Bool(true));
    assert_eq!(
        job_body["data"]["status"],
        Value::String("succeeded".to_owned())
    );

    let recall_after_rebuild = json_request(
        &app,
        Method::POST,
        "/recall",
        Some(json!({
            "namespace": "default",
            "query": "orbit-sapphire",
            "mode": "basic",
            "top_k": 5
        })),
    )
    .await;

    assert_eq!(recall_after_rebuild["success"], Value::Bool(true));
    assert_eq!(recall_after_rebuild["data"]["results"], Value::Array(Vec::new()));
    assert_eq!(
        recall_after_rebuild["data"]["metadata"]["index_state"],
        Value::String("ready".to_owned())
    );
    assert_eq!(
        recall_after_rebuild["data"]["metadata"]["result_count"],
        Value::from(0)
    );

    let stats_body = json_request(&app, Method::GET, "/stats", None).await;
    assert_eq!(stats_body["success"], Value::Bool(true));
    assert_eq!(
        stats_body["data"]["deleted_chunk_count"].as_u64(),
        Some(ingest_body["data"]["chunk_ids"].as_array().unwrap().len() as u64)
    );
    assert_eq!(
        stats_body["data"]["index_status"],
        Value::String("ready".to_owned())
    );

    let health_body = json_request(&app, Method::GET, "/health", None).await;
    assert_eq!(health_body["success"], Value::Bool(true));
    assert_eq!(health_body["data"]["status"], Value::String("ok".to_owned()));

}

async fn json_request(app: &Router, method: Method, uri: &str, body: Option<Value>) -> Value {
    let response = raw_request(app, method, uri, body).await;
    let status = response.status();
    let body = read_json_body(response).await;
    assert_eq!(status, StatusCode::OK);
    body
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
        let body = json_request(
            app,
            Method::GET,
            &format!("/admin/jobs/{job_id}"),
            None,
        )
        .await;

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
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json body")
}
