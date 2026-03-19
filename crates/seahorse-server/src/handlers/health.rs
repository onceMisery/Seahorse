use axum::extract::State;

use crate::api::{self, HealthResponseData};
use crate::state::AppState;

pub async fn get_health(State(_state): State<AppState>) -> impl axum::response::IntoResponse {
    api::success(HealthResponseData {
        status: "degraded".to_owned(),
        db: "unconfigured".to_owned(),
        index: "unconfigured".to_owned(),
        embedding_provider: "unconfigured".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}
