use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::Json;
use seahorse_core::{RebuildError, RebuildRequest as CoreRebuildRequest, RebuildScope};
use tracing::{info, warn};

use crate::api::{self, AdminRebuildRequest, AdminRebuildResponseData};
use crate::state::{AppState, AppStateError};

type RebuildResponse = (
    StatusCode,
    Json<api::ResponseEnvelope<AdminRebuildResponseData>>,
);

pub async fn post_rebuild(
    State(state): State<AppState>,
    payload: Result<Json<AdminRebuildRequest>, JsonRejection>,
) -> impl axum::response::IntoResponse {
    let Json(request) = match payload {
        Ok(json) => json,
        Err(error) => {
            warn!(
                event = "rebuild.request.invalid_json",
                error = %error,
                "rebuild request rejected"
            );
            return api::error::<AdminRebuildResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                error.body_text(),
                false,
            );
        }
    };

    info!(
        event = "rebuild.request.received",
        namespace = %request.namespace,
        scope = %request.scope.as_deref().unwrap_or("all"),
        force = request.force,
        "rebuild request received"
    );
    let force = request.force;
    let requested_scope = request.scope.clone().unwrap_or_else(|| "all".to_owned());
    let core_request = match build_rebuild_request(request) {
        Ok(request) => request,
        Err(message) => {
            warn!(
                event = "rebuild.request.invalid_input",
                reason = %message,
                "rebuild request validation failed"
            );
            return api::error::<AdminRebuildResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            );
        }
    };

    let job = match state.rebuild(core_request, force) {
        Ok(job) => job,
        Err(error) => {
            warn!(
                event = "rebuild.request.failed",
                error = %error,
                "rebuild request failed"
            );
            return map_rebuild_error(error);
        }
    };
    info!(
        event = "rebuild.request.succeeded",
        job_id = job.id,
        status = %job.status,
        scope = %requested_scope,
        force = force,
        "rebuild request completed"
    );

    api::success(AdminRebuildResponseData {
        job_id: format_job_id(job.id),
        status: job.status,
        submitted_at: job.created_at,
    })
}

fn build_rebuild_request(request: AdminRebuildRequest) -> Result<CoreRebuildRequest, String> {
    let AdminRebuildRequest {
        namespace,
        scope,
        force: _,
    } = request;

    if namespace != "default" {
        return Err("only namespace=default is supported".to_owned());
    }

    let scope = match scope.as_deref().unwrap_or("all") {
        "all" => RebuildScope::All,
        "missing_index" => RebuildScope::MissingIndex,
        other => return Err(format!("scope must be all or missing_index; got {other}")),
    };

    Ok(CoreRebuildRequest { namespace, scope })
}

fn map_rebuild_error(error: AppStateError) -> RebuildResponse {
    match error {
        AppStateError::Unavailable { message } => api::error::<AdminRebuildResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            message,
            true,
        ),
        AppStateError::Rebuild(error) => match error {
            RebuildError::InvalidInput { message } => api::error::<AdminRebuildResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            ),
            RebuildError::Embedding(source) => api::error::<AdminRebuildResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REBUILD_FAILED",
                source.to_string(),
                true,
            ),
            RebuildError::Storage(source) => api::error::<AdminRebuildResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REBUILD_FAILED",
                source.to_string(),
                false,
            ),
            RebuildError::Index(source) => api::error::<AdminRebuildResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REBUILD_FAILED",
                source.to_string(),
                true,
            ),
        },
        AppStateError::Storage(source) => api::error::<AdminRebuildResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            source.to_string(),
            false,
        ),
        AppStateError::NotFound { message } => api::error::<AdminRebuildResponseData>(
            StatusCode::NOT_FOUND,
            "INVALID_INPUT",
            message,
            false,
        ),
        AppStateError::Ingest(_) | AppStateError::Forget(_) | AppStateError::Recall(_) => {
            api::error::<AdminRebuildResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                "unexpected pipeline error in rebuild handler",
                false,
            )
        }
    }
}

fn format_job_id(job_id: i64) -> String {
    format!("job-{job_id}")
}
