use crate::api::{self, LiveResponseData};

pub async fn get_live() -> impl axum::response::IntoResponse {
    api::success(LiveResponseData {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}
