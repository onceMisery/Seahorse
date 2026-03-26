use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::Json;
use seahorse_core::{ForgetError, ForgetMode, ForgetRequest as CoreForgetRequest};

use crate::api::{self, ForgetRequest, ForgetResponseData};
use crate::state::{AppState, AppStateError};

type ForgetResponse = (
    StatusCode,
    Json<api::ResponseEnvelope<ForgetResponseData>>,
);

pub async fn post_forget(
    State(state): State<AppState>,
    payload: Result<Json<ForgetRequest>, JsonRejection>,
) -> impl axum::response::IntoResponse {
    let Json(request) = match payload {
        Ok(json) => json,
        Err(error) => {
            return api::error::<ForgetResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                error.body_text(),
                false,
            );
        }
    };

    let core_request = match build_forget_request(request) {
        Ok(request) => request,
        Err(message) => {
            return api::error::<ForgetResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            );
        }
    };

    let result = match state.forget(core_request) {
        Ok(result) => result,
        Err(error) => return map_forget_error(error),
    };

    api::success(ForgetResponseData {
        affected_chunks: result.affected_chunks,
        index_cleanup_status: result.index_cleanup_status,
    })
}

fn build_forget_request(request: ForgetRequest) -> Result<CoreForgetRequest, String> {
    let ForgetRequest {
        namespace,
        chunk_ids,
        file_id,
        mode,
    } = request;

    if namespace != "default" {
        return Err("only namespace=default is supported".to_owned());
    }

    if file_id.is_some() && !chunk_ids.is_empty() {
        return Err("provide either file_id or chunk_ids, not both".to_owned());
    }

    if file_id.is_none() && chunk_ids.is_empty() {
        return Err("either file_id or chunk_ids is required".to_owned());
    }

    if let Some(file_id) = file_id {
        if file_id <= 0 {
            return Err("file_id must be positive".to_owned());
        }
    }

    if chunk_ids.iter().any(|chunk_id| *chunk_id <= 0) {
        return Err("chunk_ids must contain only positive integers".to_owned());
    }

    let mode = match mode.as_deref().unwrap_or("soft") {
        "soft" => ForgetMode::Soft,
        other => return Err(format!("mode must be soft; got {other}")),
    };

    Ok(CoreForgetRequest {
        namespace,
        chunk_ids,
        file_id,
        mode,
    })
}

fn map_forget_error(error: AppStateError) -> ForgetResponse {
    match error {
        AppStateError::Unavailable { message } => api::error::<ForgetResponseData>(
            StatusCode::SERVICE_UNAVAILABLE,
            "INDEX_UNAVAILABLE",
            message,
            true,
        ),
        AppStateError::Forget(error) => match error {
            ForgetError::InvalidInput { message } => api::error::<ForgetResponseData>(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                message,
                false,
            ),
            ForgetError::Storage(source) => api::error::<ForgetResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                source.to_string(),
                false,
            ),
            ForgetError::Index(source) => api::error::<ForgetResponseData>(
                StatusCode::SERVICE_UNAVAILABLE,
                "INDEX_UNAVAILABLE",
                source.to_string(),
                true,
            ),
        },
        AppStateError::Storage(source) => api::error::<ForgetResponseData>(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORAGE_ERROR",
            source.to_string(),
            false,
        ),
        AppStateError::NotFound { message } => api::error::<ForgetResponseData>(
            StatusCode::NOT_FOUND,
            "INVALID_INPUT",
            message,
            false,
        ),
        AppStateError::Ingest(_) | AppStateError::Recall(_) | AppStateError::Rebuild(_) => {
            api::error::<ForgetResponseData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                "unexpected pipeline error in forget handler",
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_forget_request;
    use crate::api::ForgetRequest;

    #[test]
    fn rejects_hard_mode_at_http_boundary() {
        let result = build_forget_request(ForgetRequest {
            namespace: "default".to_owned(),
            chunk_ids: vec![1],
            file_id: None,
            mode: Some("hard".to_owned()),
        });

        let error = result.expect_err("hard mode should be rejected before reaching core");
        assert_eq!(error, "mode must be soft; got hard");
    }
}
