use seahorse_server::{
    app::build_app_with_observability, config::load_server_config_default, state::AppState,
};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let config = load_server_config_default().expect("failed to load seahorse server config");
    init_tracing(&config.observability.log_level);

    let addr = config.api.listen_addr();
    let state = AppState::new_with_config(&config)
        .expect("failed to initialize seahorse application state");
    let app = build_app_with_observability(state, &config.observability);

    if config.observability.enable_metrics {
        info!(
            metrics_path = %config.observability.metrics_path,
            "metrics endpoint enabled"
        );
    } else {
        info!("metrics endpoint disabled by configuration");
    }

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind seahorse server listener");
    info!(bind = %addr, "seahorse server listening");

    axum::serve(listener, app)
        .await
        .expect("seahorse server failed");
}

fn init_tracing(default_log_level: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_level));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .try_init();
}
