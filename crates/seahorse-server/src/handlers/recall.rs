use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::Json;
use seahorse_core::{EmbeddingError, RecallError, RecallMode, RecallRequest as CoreRecallRequest};
use serde_json::{Map, Value};
use tracing::{info, warn};

use crate::api::{
    self, RecallRequest, RecallResponseData, RecallResponseMetadata, RecallResultItem,
};
use crate::state::{AppState, AppStateError};

const MAX_TAG_COUNT: usize = 32;
const MAX_TAG_LENGTH: usize = 64;
type RecallResponse = (StatusCode, Json<api::ResponseEnvelope<RecallResponseData>>);

pub async fn post_recall(
    State(state): State<AppState>,
    payload: Result<Json<RecallRequest>, JsonRejection>,
) -> impl axum::response::IntoResponse {
    let Json(request) = match payload {
        Ok(json) => json,
        Err(error) => {
            warn!(
                event = "recall.request.invalid_json",
                error = %error,
                "recall request rejected"
            );
            return api::error::<RecallResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                error.body_text(),
                false,
            );
        }
    };
    info!(
        event = "recall.request.received",
        namespace = %request.namespace,
        query_bytes = request.query.len(),
        top_k = request.top_k,
        mode = %request.mode.as_deref().unwrap_or("basic"),
        has_filters = request.filters.is_some(),
        "recall request received"
    );

    let core_request = match build_recall_request(request) {
        Ok(request) => request,
        Err(message) => {
            warn!(
                event = "recall.request.invalid_input",
                reason = %message,
                "recall request validation failed"
            );
            return api::error::<RecallResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            );
        }
    };

    let result = match state.recall(core_request) {
        Ok(result) => result,
        Err(error) => {
            warn!(
                event = "recall.request.failed",
                error = %error,
                "recall pipeline failed"
            );
            return map_recall_error(error);
        }
    };

    let mut items = Vec::with_capacity(result.results.len());
    for item in result.results {
        let metadata = match parse_metadata_json(item.metadata_json.as_deref()) {
            Ok(metadata) => metadata,
            Err(message) => {
                warn!(
                    event = "recall.response.invalid_stored_metadata",
                    reason = %message,
                    "recall response build failed"
                );
                return api::error::<RecallResponseData>(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "STORAGE_ERROR",
                    message,
                    false,
                );
            }
        };

        items.push(RecallResultItem {
            chunk_id: item.chunk_id,
            chunk_text: item.chunk_text,
            source_file: item.source_file,
            tags: item.tags,
            score: item.score,
            source_type: item.source_type,
            metadata,
        });
    }
    info!(
        event = "recall.request.succeeded",
        result_count = items.len(),
        top_k = result.metadata.top_k,
        latency_ms = result.metadata.latency_ms,
        degraded = result.metadata.degraded,
        index_state = %result.metadata.index_state,
        worldview = %result.metadata.worldview.as_deref().unwrap_or("unknown"),
        entropy = result.metadata.entropy.unwrap_or_default(),
        "recall request completed"
    );

    api::success(RecallResponseData {
        results: items,
        metadata: RecallResponseMetadata {
            top_k: result.metadata.top_k as u32,
            latency_ms: result.metadata.latency_ms,
            degraded: result.metadata.degraded,
            degraded_reason: result.metadata.degraded_reason,
            result_count: result.metadata.result_count,
            index_state: result.metadata.index_state,
            worldview: result.metadata.worldview,
            entropy: result.metadata.entropy.map(f64::from),
            focus_terms: result.metadata.focus_terms,
            weak_signal_allowed: result.metadata.weak_signal_allowed,
            weak_signal_reason: result.metadata.weak_signal_reason,
            association_allowed: result.metadata.association_allowed,
            association_reason: result.metadata.association_reason,
            vector_result_count: result.metadata.vector_result_count,
            association_result_count: result.metadata.association_result_count,
        },
    })
}

fn build_recall_request(request: RecallRequest) -> Result<CoreRecallRequest, String> {
    let RecallRequest {
        namespace,
        query,
        top_k,
        filters,
        mode,
        timeout_ms,
    } = request;

    if namespace != "default" {
        return Err("only namespace=default is supported".to_owned());
    }

    if query.trim().is_empty() {
        return Err("query is empty".to_owned());
    }

    if top_k == 0 || top_k > 20 {
        return Err("top_k must be between 1 and 20".to_owned());
    }

    let recall_mode = match mode.as_deref().unwrap_or("basic") {
        "basic" => RecallMode::Basic,
        "tagmemo" => RecallMode::TagMemo,
        other => return Err(format!("mode must be one of basic, tagmemo; got {other}")),
    };

    let filters = filters.unwrap_or(crate::api::RecallFilters {
        file_id: None,
        tags: Vec::new(),
    });
    validate_tags(&filters.tags)?;

    let mut core_request = CoreRecallRequest::new(query);
    core_request.namespace = namespace;
    core_request.top_k = top_k as usize;
    core_request.mode = recall_mode;
    core_request.timeout_ms = timeout_ms;
    core_request.filters.file_id = filters.file_id;
    core_request.filters.tags = filters.tags;

    Ok(core_request)
}

fn validate_tags(tags: &[String]) -> Result<(), String> {
    if tags.len() > MAX_TAG_COUNT {
        return Err(format!(
            "filters.tags exceeds maximum count {}",
            MAX_TAG_COUNT
        ));
    }

    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            return Err("filters.tags contains empty tag".to_owned());
        }

        if trimmed.len() > MAX_TAG_LENGTH {
            return Err(format!(
                "filters.tags contains tag exceeding {} characters",
                MAX_TAG_LENGTH
            ));
        }
    }

    Ok(())
}

fn parse_metadata_json(metadata_json: Option<&str>) -> Result<Value, String> {
    let Some(metadata_json) = metadata_json else {
        return Ok(Value::Object(Map::new()));
    };

    let value = serde_json::from_str::<Value>(metadata_json)
        .map_err(|error| format!("stored metadata_json is invalid JSON: {error}"))?;
    if !value.is_object() {
        return Err("stored metadata_json must be a JSON object".to_owned());
    }

    Ok(value)
}

fn map_recall_error(error: AppStateError) -> RecallResponse {
    match error {
        AppStateError::Unavailable { message } => api::error::<RecallResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            message,
            true,
        ),
        AppStateError::Recall(error) => match error {
            RecallError::InvalidInput { message } => api::error::<RecallResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            ),
            RecallError::Timeout { .. } => api::error::<RecallResponseData>(
                StatusCode::GATEWAY_TIMEOUT,
                "TIMEOUT",
                error.to_string(),
                true,
            ),
            RecallError::Embedding(source) => map_embedding_error(source),
            RecallError::Storage(source) => api::error::<RecallResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                source.to_string(),
                false,
            ),
            RecallError::Index(source) => api::error::<RecallResponseData>(
                StatusCode::SERVICE_UNAVAILABLE,
                "INDEX_UNAVAILABLE",
                source.to_string(),
                true,
            ),
        },
        AppStateError::Storage(source) => api::error::<RecallResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            source.to_string(),
            false,
        ),
        AppStateError::NotFound { message } => {
            api::error::<RecallResponseData>(StatusCode::NOT_FOUND, "INVALID_INPUT", message, false)
        }
        AppStateError::Ingest(_) | AppStateError::Forget(_) | AppStateError::Rebuild(_) => {
            api::error::<RecallResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                "unexpected pipeline error in recall handler",
                false,
            )
        }
    }
}

fn map_embedding_error(error: EmbeddingError) -> RecallResponse {
    match error {
        EmbeddingError::ProviderTimeout { .. } => api::error::<RecallResponseData>(
            StatusCode::GATEWAY_TIMEOUT,
            "TIMEOUT",
            error.to_string(),
            true,
        ),
        EmbeddingError::ProviderFailure { .. } | EmbeddingError::DimensionMismatch { .. } => {
            api::error::<RecallResponseData>(
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
    use super::build_recall_request;
    use crate::api::RecallRequest;
    use seahorse_core::RecallMode;

    #[test]
    fn forwards_timeout_ms_into_core_request() {
        let request = build_recall_request(RecallRequest {
            namespace: "default".to_owned(),
            query: "alpha".to_owned(),
            top_k: 5,
            filters: None,
            mode: Some("basic".to_owned()),
            timeout_ms: Some(25),
        })
        .expect("recall request should build");

        assert_eq!(request.timeout_ms, Some(25));
    }

    #[test]
    fn accepts_tagmemo_mode() {
        let request = build_recall_request(RecallRequest {
            namespace: "default".to_owned(),
            query: "alpha".to_owned(),
            top_k: 5,
            filters: None,
            mode: Some("tagmemo".to_owned()),
            timeout_ms: None,
        })
        .expect("tagmemo recall request should build");

        assert_eq!(request.mode, RecallMode::TagMemo);
    }
}
