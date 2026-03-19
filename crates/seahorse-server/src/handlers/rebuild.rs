use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::Json;
use seahorse_core::{RebuildError, RebuildRequest as CoreRebuildRequest, RebuildScope};

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
            return api::error::<AdminRebuildResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                error.body_text(),
                false,
            );
        }
    };

    let force = request.force;
    let core_request = match build_rebuild_request(request) {
        Ok(request) => request,
        Err(message) => {
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
        Err(error) => return map_rebuild_error(error),
    };

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
        AppStateError::Ingest(_)
        | AppStateError::Forget(_)
        | AppStateError::Recall(_) => api::error::<AdminRebuildResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            "unexpected pipeline error in rebuild handler",
            false,
        ),
    }
}

fn format_job_id(job_id: i64) -> String {
    format!("job-{job_id}")
}
