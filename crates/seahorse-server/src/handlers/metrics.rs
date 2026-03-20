use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::IntoResponse;

use crate::api::observability;
use crate::state::{AppState, AppStateError, HealthSnapshot, StatsSnapshot};

const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";
const INDEX_STATES: &[&str] = &["ready", "rebuilding", "degraded", "unavailable"];
const HEALTH_STATUSES: &[&str] = &["ok", "degraded", "failed"];

pub async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    match render_metrics(&state) {
        Ok(body) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, HeaderValue::from_static(PROMETHEUS_CONTENT_TYPE))],
            body,
        )
            .into_response(),
        Err(error) => map_metrics_error(error).into_response(),
    }
}

fn render_metrics(state: &AppState) -> Result<String, AppStateError> {
    let request_metrics = observability::request_metrics_snapshot();
    let stats = state.stats_snapshot()?;
    let health = state.health_snapshot()?;
    let mut lines = Vec::new();

    append_metric_help(
        &mut lines,
        "seahorse_http_requests_total",
        "Total HTTP requests observed.",
        "counter",
    );
    lines.push(format!(
        "seahorse_http_requests_total{{scope=\"total\"}} {}",
        request_metrics.total.request_total
    ));
    for route_metrics in &request_metrics.by_route {
        lines.push(format!(
            "seahorse_http_requests_total{{scope=\"route\",method=\"{}\",route=\"{}\"}} {}",
            escape_label_value(&route_metrics.method),
            escape_label_value(&route_metrics.route),
            route_metrics.values.request_total
        ));
    }

    append_metric_help(
        &mut lines,
        "seahorse_http_request_errors_total",
        "Total HTTP requests with status >= 400.",
        "counter",
    );
    lines.push(format!(
        "seahorse_http_request_errors_total{{scope=\"total\"}} {}",
        request_metrics.total.request_error_total
    ));
    for route_metrics in &request_metrics.by_route {
        lines.push(format!(
            "seahorse_http_request_errors_total{{scope=\"route\",method=\"{}\",route=\"{}\"}} {}",
            escape_label_value(&route_metrics.method),
            escape_label_value(&route_metrics.route),
            route_metrics.values.request_error_total
        ));
    }

    append_metric_help(
        &mut lines,
        "seahorse_http_request_latency_ms_sum",
        "Sum of HTTP request latency in milliseconds.",
        "counter",
    );
    lines.push(format!(
        "seahorse_http_request_latency_ms_sum{{scope=\"total\"}} {}",
        request_metrics.total.request_latency_ms_sum
    ));
    for route_metrics in &request_metrics.by_route {
        lines.push(format!(
            "seahorse_http_request_latency_ms_sum{{scope=\"route\",method=\"{}\",route=\"{}\"}} {}",
            escape_label_value(&route_metrics.method),
            escape_label_value(&route_metrics.route),
            route_metrics.values.request_latency_ms_sum
        ));
    }

    append_metric_help(
        &mut lines,
        "seahorse_http_request_latency_ms_max",
        "Max HTTP request latency in milliseconds.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_http_request_latency_ms_max{{scope=\"total\"}} {}",
        request_metrics.total.request_latency_ms_max
    ));
    for route_metrics in &request_metrics.by_route {
        lines.push(format!(
            "seahorse_http_request_latency_ms_max{{scope=\"route\",method=\"{}\",route=\"{}\"}} {}",
            escape_label_value(&route_metrics.method),
            escape_label_value(&route_metrics.route),
            route_metrics.values.request_latency_ms_max
        ));
    }

    append_runtime_metrics(&mut lines, &stats, &health);

    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn append_runtime_metrics(lines: &mut Vec<String>, stats: &StatsSnapshot, health: &HealthSnapshot) {
    append_metric_help(lines, "seahorse_chunk_count", "Active chunk count.", "gauge");
    lines.push(format!("seahorse_chunk_count {}", stats.chunk_count));

    append_metric_help(lines, "seahorse_tag_count", "Tag count.", "gauge");
    lines.push(format!("seahorse_tag_count {}", stats.tag_count));

    append_metric_help(
        lines,
        "seahorse_deleted_chunk_count",
        "Deleted chunk count.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_deleted_chunk_count {}",
        stats.deleted_chunk_count
    ));

    append_metric_help(
        lines,
        "seahorse_repair_queue_backlog",
        "Repair queue backlog size.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_repair_queue_backlog{{namespace=\"default\"}} {}",
        stats.repair_queue_size
    ));

    append_metric_help(
        lines,
        "seahorse_index_state",
        "Current index state as a one-hot gauge.",
        "gauge",
    );
    for state in INDEX_STATES {
        let value = if stats.index_status == *state { 1 } else { 0 };
        lines.push(format!("seahorse_index_state{{state=\"{state}\"}} {value}"));
    }

    append_metric_help(
        lines,
        "seahorse_health_status",
        "Current health status as a one-hot gauge.",
        "gauge",
    );
    for status in HEALTH_STATUSES {
        let value = if health.status == *status { 1 } else { 0 };
        lines.push(format!("seahorse_health_status{{status=\"{status}\"}} {value}"));
    }
}

fn append_metric_help(lines: &mut Vec<String>, name: &str, help: &str, metric_type: &str) {
    lines.push(format!("# HELP {name} {help}"));
    lines.push(format!("# TYPE {name} {metric_type}"));
}

fn escape_label_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn map_metrics_error(error: AppStateError) -> (StatusCode, [(axum::http::header::HeaderName, HeaderValue); 1], String) {
    let (status, body) = match error {
        AppStateError::Unavailable { message } => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("# metrics unavailable: {message}\n"),
        ),
        AppStateError::Storage(source) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("# storage error: {source}\n"),
        ),
        AppStateError::NotFound { message } => (
            StatusCode::NOT_FOUND,
            format!("# metrics not found: {message}\n"),
        ),
        AppStateError::Ingest(_)
        | AppStateError::Forget(_)
        | AppStateError::Recall(_)
        | AppStateError::Rebuild(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "# unexpected pipeline error in metrics handler\n".to_owned(),
        ),
    };

    (
        status,
        [(header::CONTENT_TYPE, HeaderValue::from_static(PROMETHEUS_CONTENT_TYPE))],
        body,
    )
}
