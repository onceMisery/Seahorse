use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::Json;
use seahorse_core::{DedupMode, EmbeddingError, IngestError, IngestRequest as CoreIngestRequest};
use serde_json::Value;
use tracing::{info, warn};

use crate::api::{self, IngestRequest, IngestResponseData};
use crate::state::{AppState, AppStateError};

const MAX_CONTENT_BYTES: usize = 1024 * 1024;
const MAX_FILENAME_LENGTH: usize = 255;
const MAX_TAG_COUNT: usize = 32;
const MAX_TAG_LENGTH: usize = 64;
const MAX_METADATA_BYTES: usize = 16 * 1024;
type IngestResponse = (StatusCode, Json<api::ResponseEnvelope<IngestResponseData>>);

pub async fn post_ingest(
    State(state): State<AppState>,
    payload: Result<Json<IngestRequest>, JsonRejection>,
) -> impl axum::response::IntoResponse {
    let Json(request) = match payload {
        Ok(json) => json,
        Err(error) => {
            warn!(
                event = "ingest.request.invalid_json",
                error = %error,
                "ingest request rejected"
            );
            return api::error::<IngestResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                error.body_text(),
                false,
            );
        }
    };
    info!(
        event = "ingest.request.received",
        namespace = %request.namespace,
        content_bytes = request.content.len(),
        tag_count = request.tags.len(),
        has_source = request.source.is_some(),
        has_metadata = request.metadata.is_some(),
        "ingest request received"
    );

    let core_request = match build_ingest_request(request) {
        Ok(request) => request,
        Err(message) => {
            warn!(
                event = "ingest.request.invalid_input",
                reason = %message,
                "ingest request validation failed"
            );
            return api::error::<IngestResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            );
        }
    };

    let result = match state.ingest(core_request) {
        Ok(result) => result,
        Err(error) => {
            warn!(
                event = "ingest.request.failed",
                error = %error,
                "ingest pipeline failed"
            );
            return map_ingest_error(error);
        }
    };
    let chunk_count = result.chunk_ids.len();
    let warning_count = result.warnings.len();
    info!(
        event = "ingest.request.succeeded",
        file_id = result.file_id,
        chunk_count = chunk_count,
        ingest_status = %result.ingest_status,
        index_status = %result.index_status,
        warning_count = warning_count,
        "ingest request completed"
    );

    api::success(IngestResponseData {
        file_id: result.file_id,
        chunk_ids: result.chunk_ids,
        ingest_status: result.ingest_status,
        index_status: result.index_status,
        warnings: result.warnings,
    })
}

fn build_ingest_request(request: IngestRequest) -> Result<CoreIngestRequest, String> {
    let IngestRequest {
        namespace,
        content,
        source,
        tags,
        metadata,
        options,
    } = request;

    if namespace != "default" {
        return Err("only namespace=default is supported".to_owned());
    }

    if content.trim().is_empty() {
        return Err("content is empty".to_owned());
    }

    if content.len() > MAX_CONTENT_BYTES {
        return Err(format!("content exceeds {} bytes", MAX_CONTENT_BYTES));
    }

    validate_tags(&tags)?;

    let filename = resolve_filename(source.as_ref())?;
    let source_type = source.as_ref().and_then(|value| value.r#type.clone());
    let metadata_json = serialize_metadata(metadata.as_ref())?;
    let options = options.as_ref();

    let mut core_request = CoreIngestRequest::new(content);
    core_request.namespace = namespace;
    core_request.filename = filename;
    core_request.source_type = source_type;
    core_request.tags = tags;
    core_request.metadata_json = metadata_json;
    parse_chunk_mode(options.and_then(|value| value.chunk_mode.as_deref()))?;
    core_request.options.auto_tag = options.and_then(|value| value.auto_tag).unwrap_or(false);
    core_request.options.dedup_mode =
        parse_dedup_mode(options.and_then(|value| value.dedup_mode.as_deref()))?;

    Ok(core_request)
}

fn resolve_filename(source: Option<&crate::api::SourceInput>) -> Result<String, String> {
    let Some(filename) = source.and_then(|source| source.filename.as_deref()) else {
        return Ok("inline.txt".to_owned());
    };

    let trimmed = filename.trim();
    if trimmed.is_empty() {
        return Err("source.filename must not be empty".to_owned());
    }

    if trimmed.len() > MAX_FILENAME_LENGTH {
        return Err(format!(
            "source.filename exceeds {} characters",
            MAX_FILENAME_LENGTH
        ));
    }

    Ok(trimmed.to_owned())
}

fn validate_tags(tags: &[String]) -> Result<(), String> {
    if tags.len() > MAX_TAG_COUNT {
        return Err(format!("tags exceeds maximum count {}", MAX_TAG_COUNT));
    }

    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            return Err("tag must not be empty".to_owned());
        }

        if trimmed.len() > MAX_TAG_LENGTH {
            return Err(format!("tag exceeds {} characters", MAX_TAG_LENGTH));
        }
    }

    Ok(())
}

fn serialize_metadata(metadata: Option<&Value>) -> Result<Option<String>, String> {
    let Some(metadata) = metadata else {
        return Ok(None);
    };

    if !metadata.is_object() {
        return Err("metadata must be a JSON object".to_owned());
    }

    let serialized = serde_json::to_string(metadata)
        .map_err(|error| format!("failed to serialize metadata: {error}"))?;
    if serialized.len() > MAX_METADATA_BYTES {
        return Err(format!(
            "metadata exceeds {} bytes after serialization",
            MAX_METADATA_BYTES
        ));
    }

    Ok(Some(serialized))
}

fn parse_dedup_mode(value: Option<&str>) -> Result<DedupMode, String> {
    match value.unwrap_or("reject") {
        "reject" => Ok(DedupMode::Reject),
        "upsert" => Ok(DedupMode::Upsert),
        "allow" => Ok(DedupMode::Allow),
        other => Err(format!(
            "options.dedup_mode must be one of reject, upsert, allow; got {other}"
        )),
    }
}

fn parse_chunk_mode(value: Option<&str>) -> Result<(), String> {
    match value.unwrap_or("fixed") {
        "fixed" => Ok(()),
        other => Err(format!(
            "options.chunk_mode must be fixed in current build; got {other}"
        )),
    }
}

fn map_ingest_error(error: AppStateError) -> IngestResponse {
    match error {
        AppStateError::Unavailable { message } => api::error::<IngestResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            message,
            true,
        ),
        AppStateError::Ingest(error) => match error {
            IngestError::InvalidInput { message } => api::error::<IngestResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            ),
            IngestError::UnsupportedDedupMode { mode, reason } => api::error::<IngestResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                format!(
                    "dedup_mode {} is not available in current build: {reason}",
                    mode.as_str()
                ),
                false,
            ),
            IngestError::Embedding(source) => map_embedding_error(source),
            IngestError::Storage(source) => api::error::<IngestResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                source.to_string(),
                false,
            ),
            IngestError::Index(source) => api::error::<IngestResponseData>(
                StatusCode::SERVICE_UNAVAILABLE,
                "INDEX_UNAVAILABLE",
                source.to_string(),
                true,
            ),
        },
        AppStateError::Storage(source) => api::error::<IngestResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            source.to_string(),
            false,
        ),
        AppStateError::NotFound { message } => {
            api::error::<IngestResponseData>(StatusCode::NOT_FOUND, "INVALID_INPUT", message, false)
        }
        AppStateError::Recall(_) | AppStateError::Forget(_) | AppStateError::Rebuild(_) => {
            api::error::<IngestResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                "unexpected pipeline error in ingest handler",
                false,
            )
        }
    }
}

fn map_embedding_error(error: EmbeddingError) -> IngestResponse {
    match error {
        EmbeddingError::ProviderTimeout { .. } => api::error::<IngestResponseData>(
            StatusCode::GATEWAY_TIMEOUT,
            "TIMEOUT",
            error.to_string(),
            true,
        ),
        EmbeddingError::ProviderFailure { .. } | EmbeddingError::DimensionMismatch { .. } => {
            api::error::<IngestResponseData>(
                StatusCode::SERVICE_UNAVAILABLE,
                "EMBEDDING_FAILED",
                error.to_string(),
                true,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_chunk_mode;

    #[test]
    fn rejects_non_fixed_chunk_mode_at_http_boundary() {
        let error = parse_chunk_mode(Some("semantic")).expect_err("chunk_mode should be rejected");
        assert_eq!(
            error,
            "options.chunk_mode must be fixed in current build; got semantic"
        );
    }
}
