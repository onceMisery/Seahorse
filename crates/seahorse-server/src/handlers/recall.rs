use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::Json;

use crate::api::{self, RecallRequest, RecallResponseData, RecallResponseMetadata};
use crate::state::AppState;

pub async fn post_recall(
    State(_state): State<AppState>,
    payload: Result<Json<RecallRequest>, JsonRejection>,
) -> impl axum::response::IntoResponse {
    let Json(request) = match payload {
        Ok(json) => json,
        Err(error) => {
            return api::error::<RecallResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                error.body_text(),
                false,
            );
        }
    };

    if request.query.trim().is_empty() {
        return api::error::<RecallResponseData>(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "query is empty",
            false,
        );
    }

    if request.top_k == 0 || request.top_k > 20 {
        return api::error::<RecallResponseData>(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "top_k must be between 1 and 20",
            false,
        );
    }

    api::success(RecallResponseData {
        results: Vec::new(),
        metadata: RecallResponseMetadata {
            top_k: request.top_k,
            latency_ms: 0,
            degraded: true,
            degraded_reason: Some("recall pipeline not wired yet".to_owned()),
            result_count: 0,
            index_state: "degraded".to_owned(),
        },
    })
}
