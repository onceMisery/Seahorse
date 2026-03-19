use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::Json;

use crate::api::{self, IngestRequest, IngestResponseData};
use crate::state::AppState;

pub async fn post_ingest(
    State(_state): State<AppState>,
    payload: Result<Json<IngestRequest>, JsonRejection>,
) -> impl axum::response::IntoResponse {
    let Json(request) = match payload {
        Ok(json) => json,
        Err(error) => {
            return api::error::<IngestResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                error.body_text(),
                false,
            );
        }
    };

    if request.content.trim().is_empty() {
        return api::error::<IngestResponseData>(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "content is empty",
            false,
        );
    }

    api::success(IngestResponseData {
        file_id: 0,
        chunk_ids: Vec::new(),
        ingest_status: "pending_index".to_owned(),
        index_status: "pending_repair".to_owned(),
        warnings: vec!["ingest handler is a placeholder".to_owned()],
    })
}
