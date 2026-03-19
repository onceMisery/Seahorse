use axum::extract::State;

use crate::api::{self, HealthResponseData};
use crate::state::AppState;

pub async fn get_health(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    match state.health_snapshot() {
        Ok(snapshot) => api::success(HealthResponseData {
            status: snapshot.status,
            db: snapshot.db,
            index: snapshot.index,
            embedding_provider: snapshot.embedding_provider,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
        Err(_) => api::success(HealthResponseData {
            status: "failed".to_owned(),
            db: "unavailable".to_owned(),
            index: "unavailable".to_owned(),
            embedding_provider: "unavailable".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
    }
}
