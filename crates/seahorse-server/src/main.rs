mod api;
mod handlers;
mod state;

use axum::{routing::{get, post}, Router};

use state::AppState;

#[tokio::main]
async fn main() {
    let state = AppState::new().expect("failed to initialize seahorse application state");
    let app = Router::new()
        .route("/ingest", post(handlers::ingest::post_ingest))
        .route("/recall", post(handlers::recall::post_recall))
        .route("/forget", post(handlers::forget::post_forget))
        .route("/admin/rebuild", post(handlers::rebuild::post_rebuild))
        .route("/admin/jobs/{job_id}", get(handlers::jobs::get_job))
        .route("/stats", get(handlers::stats::get_stats))
        .route("/health", get(handlers::health::get_health))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen_addr())
        .await
        .expect("failed to bind seahorse server listener");

    axum::serve(listener, app)
        .await
        .expect("seahorse server failed");
}

fn listen_addr() -> String {
    std::env::var("SEAHORSE_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
}
