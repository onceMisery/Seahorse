use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::api::{self, HealthResponseData};
use crate::state::{AppState, AppStateError, HealthSnapshot};

type ReadyResponse = (StatusCode, Json<api::ResponseEnvelope<HealthResponseData>>);

pub async fn get_ready(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    match state.health_snapshot() {
        Ok(snapshot) if snapshot.status == "failed" => api::error::<HealthResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            "service is not ready",
            true,
        ),
        Ok(snapshot) => api::success(build_health_response(snapshot)),
        Err(error) => map_ready_error(error),
    }
}

fn build_health_response(snapshot: HealthSnapshot) -> HealthResponseData {
    HealthResponseData {
        status: snapshot.status,
        db: snapshot.db,
        index: snapshot.index,
        embedding_provider: snapshot.embedding_provider,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

fn map_ready_error(error: AppStateError) -> ReadyResponse {
    match error {
        AppStateError::Unavailable { message } => api::error::<HealthResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            message,
            true,
        ),
        AppStateError::Storage(source) => api::error::<HealthResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "STORAGE_ERROR",
            source.to_string(),
            true,
        ),
        AppStateError::NotFound { message } => api::error::<HealthResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INVALID_INPUT",
            message,
            false,
        ),
        AppStateError::Ingest(_)
        | AppStateError::Forget(_)
        | AppStateError::Recall(_)
        | AppStateError::Rebuild(_) => api::error::<HealthResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            "unexpected pipeline error in readiness handler",
            true,
        ),
    }
}
