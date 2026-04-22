use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use tracing::{info, warn};

use crate::api::{self, AdminJobResponseData};
use crate::state::{AppState, AppStateError};

type JobResponse = (
    StatusCode,
    Json<api::ResponseEnvelope<AdminJobResponseData>>,
);

pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> impl axum::response::IntoResponse {
    info!(
        event = "job.query.received",
        job_id = %job_id,
        "maintenance job query received"
    );
    let job_id = match parse_job_id(&job_id) {
        Ok(job_id) => job_id,
        Err(message) => {
            warn!(
                event = "job.query.invalid_input",
                reason = %message,
                "maintenance job query rejected"
            );
            return api::error::<AdminJobResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            );
        }
    };

    let job = match state.get_job(job_id) {
        Ok(job) => job,
        Err(error) => {
            warn!(
                event = "job.query.failed",
                job_id = job_id,
                error = %error,
                "maintenance job query failed"
            );
            return map_job_error(error);
        }
    };
    info!(
        event = "job.query.succeeded",
        job_id = job.id,
        job_type = %job.job_type,
        status = %job.status,
        has_error_message = job.error_message.is_some(),
        "maintenance job query completed"
    );

    api::success(AdminJobResponseData {
        job_id: format_job_id(job.id),
        job_type: job.job_type,
        status: job.status,
        progress: job.progress,
        error_message: job.error_message,
        started_at: job.started_at,
        finished_at: job.finished_at,
    })
}

fn parse_job_id(value: &str) -> Result<i64, String> {
    let normalized = value.strip_prefix("job-").unwrap_or(value);
    let job_id = normalized
        .parse::<i64>()
        .map_err(|_| format!("job_id must be a positive integer; got {value}"))?;
    if job_id <= 0 {
        return Err(format!("job_id must be a positive integer; got {value}"));
    }

    Ok(job_id)
}

fn map_job_error(error: AppStateError) -> JobResponse {
    match error {
        AppStateError::Unavailable { message } => api::error::<AdminJobResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            message,
            true,
        ),
        AppStateError::Storage(source) => api::error::<AdminJobResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            source.to_string(),
            false,
        ),
        AppStateError::NotFound { message } => api::error::<AdminJobResponseData>(
            StatusCode::NOT_FOUND,
            "INVALID_INPUT",
            message,
            false,
        ),
        AppStateError::Ingest(_)
        | AppStateError::Forget(_)
        | AppStateError::Recall(_)
        | AppStateError::Rebuild(_) => api::error::<AdminJobResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            "unexpected pipeline error in job handler",
            false,
        ),
    }
}

fn format_job_id(job_id: i64) -> String {
    format!("job-{job_id}")
}
