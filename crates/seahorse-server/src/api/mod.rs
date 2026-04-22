use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod observability;

#[derive(Debug, Serialize)]
pub struct ResponseEnvelope<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ErrorObject>,
    pub request_id: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorObject {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub content: String,
    #[serde(default)]
    pub source: Option<SourceInput>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub options: Option<IngestOptions>,
}

#[derive(Debug, Deserialize)]
pub struct SourceInput {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IngestOptions {
    #[serde(default)]
    pub chunk_mode: Option<String>,
    #[serde(default)]
    pub auto_tag: Option<bool>,
    #[serde(default)]
    pub dedup_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecallRequest {
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: u32,
    #[serde(default)]
    pub filters: Option<RecallFilters>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ForgetRequest {
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default)]
    pub chunk_ids: Vec<i64>,
    #[serde(default)]
    pub file_id: Option<i64>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminRebuildRequest {
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct RecallFilters {
    #[serde(default)]
    pub file_id: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct IngestResponseData {
    pub file_id: i64,
    pub chunk_ids: Vec<i64>,
    pub ingest_status: String,
    pub index_status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RecallResponseData {
    pub results: Vec<RecallResultItem>,
    pub metadata: RecallResponseMetadata,
}

#[derive(Debug, Serialize)]
pub struct ForgetResponseData {
    pub affected_chunks: usize,
    pub index_cleanup_status: String,
}

#[derive(Debug, Serialize)]
pub struct AdminRebuildResponseData {
    pub job_id: String,
    pub status: String,
    pub submitted_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminJobResponseData {
    pub job_id: String,
    pub job_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RecallResultItem {
    pub chunk_id: i64,
    pub chunk_text: String,
    pub source_file: String,
    pub tags: Vec<String>,
    pub score: f32,
    pub source_type: String,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct RecallResponseMetadata {
    pub top_k: u32,
    pub latency_ms: u64,
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub result_count: usize,
    pub index_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worldview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entropy: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_terms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub association_allowed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub association_reason: Option<String>,
    pub vector_result_count: usize,
    pub association_result_count: usize,
}

#[derive(Debug, Serialize)]
pub struct HealthResponseData {
    pub status: String,
    pub db: String,
    pub index: String,
    pub embedding_provider: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
pub struct LiveResponseData {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
pub struct StatsResponseData {
    pub chunk_count: usize,
    pub tag_count: usize,
    pub deleted_chunk_count: usize,
    pub repair_queue_size: usize,
    pub index_status: String,
}

pub fn success<T>(data: T) -> (StatusCode, Json<ResponseEnvelope<T>>)
where
    T: Serialize,
{
    (
        StatusCode::OK,
        Json(ResponseEnvelope {
            success: true,
            data: Some(data),
            error: None,
            request_id: current_request_id_or_fallback(),
        }),
    )
}

pub fn error<T>(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
    retryable: bool,
) -> (StatusCode, Json<ResponseEnvelope<T>>)
where
    T: Serialize,
{
    (
        status,
        Json(ResponseEnvelope {
            success: false,
            data: None,
            error: Some(ErrorObject {
                code,
                message: message.into(),
                retryable,
            }),
            request_id: current_request_id_or_fallback(),
        }),
    )
}

pub fn default_namespace() -> String {
    "default".to_owned()
}

pub fn default_top_k() -> u32 {
    10
}

fn current_request_id_or_fallback() -> String {
    observability::current_request_id().unwrap_or_else(current_timestamp_request_id)
}

fn current_timestamp_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("req-{millis}-{counter}")
}
