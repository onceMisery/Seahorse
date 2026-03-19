use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::api::{self, StatsResponseData};
use crate::state::{AppState, AppStateError};

type StatsResponse = (
    StatusCode,
    Json<api::ResponseEnvelope<StatsResponseData>>,
);

pub async fn get_stats(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    match state.stats_snapshot() {
        Ok(snapshot) => api::success(StatsResponseData {
            chunk_count: snapshot.chunk_count,
            tag_count: snapshot.tag_count,
            deleted_chunk_count: snapshot.deleted_chunk_count,
            repair_queue_size: snapshot.repair_queue_size,
            index_status: snapshot.index_status,
        }),
        Err(error) => map_stats_error(error),
    }
}

fn map_stats_error(error: AppStateError) -> StatsResponse {
    match error {
        AppStateError::Unavailable { message } => api::error::<StatsResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            message,
            true,
        ),
        AppStateError::Storage(source) => api::error::<StatsResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            source.to_string(),
            false,
        ),
        AppStateError::NotFound { message } => api::error::<StatsResponseData>(
            StatusCode::NOT_FOUND,
            "INVALID_INPUT",
            message,
            false,
        ),
        AppStateError::Ingest(_)
        | AppStateError::Forget(_)
        | AppStateError::Recall(_)
        | AppStateError::Rebuild(_) => api::error::<StatsResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            "unexpected pipeline error in stats handler",
            false,
        ),
    }
}
