use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::IntoResponse;

use crate::api::observability;
use crate::state::{AppState, AppStateError, MetricsSnapshot, StatusCountSnapshot};

const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";
const INDEX_STATES: &[&str] = &["ready", "rebuilding", "degraded", "unavailable"];
const HEALTH_STATUSES: &[&str] = &["ok", "degraded", "failed"];
const REPAIR_QUEUE_STATUSES: &[&str] = &["pending", "running", "failed", "deadletter", "succeeded"];
const REBUILD_JOB_STATUSES: &[&str] = &["queued", "running", "succeeded", "failed", "cancelled"];
const RECALL_WORLDVIEWS: &[&str] = &["default", "technical", "creative", "emotional"];
const RECALL_RESULT_SOURCES: &[&str] = &["vector", "weak_signal", "spike_association"];

pub async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    match render_metrics(&state) {
        Ok(body) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static(PROMETHEUS_CONTENT_TYPE),
            )],
            body,
        )
            .into_response(),
        Err(error) => map_metrics_error(error).into_response(),
    }
}

fn render_metrics(state: &AppState) -> Result<String, AppStateError> {
    let request_metrics = observability::request_metrics_snapshot();
    let runtime = state.metrics_snapshot()?;
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

    append_runtime_metrics(&mut lines, &runtime);

    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn append_runtime_metrics(lines: &mut Vec<String>, runtime: &MetricsSnapshot) {
    append_metric_help(
        lines,
        "seahorse_chunk_count",
        "Active chunk count.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_chunk_count {}",
        runtime.stats.chunk_count
    ));

    append_metric_help(lines, "seahorse_tag_count", "Tag count.", "gauge");
    lines.push(format!("seahorse_tag_count {}", runtime.stats.tag_count));

    append_metric_help(
        lines,
        "seahorse_connectome_edge_count",
        "Connectome edge count.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_connectome_edge_count {}",
        runtime.connectome_edge_count
    ));

    append_metric_help(
        lines,
        "seahorse_connectome_density",
        "Connectome density across active tags.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_connectome_density {}",
        runtime.connectome_density
    ));

    append_metric_help(
        lines,
        "seahorse_deleted_chunk_count",
        "Deleted chunk count.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_deleted_chunk_count {}",
        runtime.stats.deleted_chunk_count
    ));

    append_metric_help(
        lines,
        "seahorse_repair_queue_backlog",
        "Repair queue backlog size.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_repair_queue_backlog{{namespace=\"default\"}} {}",
        runtime.stats.repair_queue_size
    ));

    append_status_count_metric(
        lines,
        "seahorse_repair_queue_tasks",
        "Repair queue task count by status.",
        REPAIR_QUEUE_STATUSES,
        &runtime.repair_queue_statuses,
    );

    append_status_count_metric(
        lines,
        "seahorse_rebuild_jobs",
        "Rebuild maintenance job count by status.",
        REBUILD_JOB_STATUSES,
        &runtime.rebuild_job_statuses,
    );

    append_metric_help(
        lines,
        "seahorse_repair_oldest_task_age_seconds",
        "Age in seconds of the oldest repair queue task that still requires attention.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_repair_oldest_task_age_seconds {}",
        runtime.repair_oldest_task_age_seconds.unwrap_or(0.0)
    ));

    append_metric_help(
        lines,
        "seahorse_rebuild_oldest_active_job_age_seconds",
        "Age in seconds of the oldest active rebuild job.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_rebuild_oldest_active_job_age_seconds {}",
        runtime.rebuild_oldest_active_job_age_seconds.unwrap_or(0.0)
    ));

    append_metric_help(
        lines,
        "seahorse_index_state",
        "Current index state as a one-hot gauge.",
        "gauge",
    );
    let mut index_state_known = false;
    for state in INDEX_STATES {
        let value = if runtime.stats.index_status == *state {
            index_state_known = true;
            1
        } else {
            0
        };
        lines.push(format!("seahorse_index_state{{state=\"{state}\"}} {value}"));
    }
    if !index_state_known {
        lines.push("seahorse_index_state{state=\"unknown\"} 1".to_owned());
    }

    append_metric_help(
        lines,
        "seahorse_health_status",
        "Current health status as a one-hot gauge.",
        "gauge",
    );
    let mut health_status_known = false;
    for status in HEALTH_STATUSES {
        let value = if runtime.health.status == *status {
            health_status_known = true;
            1
        } else {
            0
        };
        lines.push(format!(
            "seahorse_health_status{{status=\"{status}\"}} {value}"
        ));
    }
    if !health_status_known {
        lines.push("seahorse_health_status{status=\"unknown\"} 1".to_owned());
    }

    append_metric_help(
        lines,
        "seahorse_recall_recent_total",
        "Recent retrieval_log sample size used for recall telemetry.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_recall_recent_total {}",
        runtime.recall_telemetry.sample_count
    ));

    append_status_count_metric(
        lines,
        "seahorse_recall_recent_worldviews",
        "Recent recall worldview distribution.",
        RECALL_WORLDVIEWS,
        &runtime.recall_telemetry.worldview_counts,
    );

    append_metric_help(
        lines,
        "seahorse_recall_recent_entropy_avg",
        "Average entropy across recent recalls.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_recall_recent_entropy_avg {}",
        runtime.recall_telemetry.average_entropy.unwrap_or(0.0)
    ));

    append_metric_help(
        lines,
        "seahorse_recall_recent_spike_depth_avg",
        "Average spike depth across recent recalls.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_recall_recent_spike_depth_avg {}",
        runtime.recall_telemetry.average_spike_depth.unwrap_or(0.0)
    ));

    append_metric_help(
        lines,
        "seahorse_recall_recent_emergent_total",
        "Total emergent recall count across recent recalls.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_recall_recent_emergent_total {}",
        runtime.recall_telemetry.emergent_total
    ));

    append_metric_help(
        lines,
        "seahorse_recall_recent_results_total",
        "Total recent recall results grouped by source.",
        "gauge",
    );
    for source in RECALL_RESULT_SOURCES {
        let value = match *source {
            "vector" => runtime.recall_telemetry.vector_result_total,
            "weak_signal" => 0,
            "spike_association" => runtime.recall_telemetry.association_result_total,
            _ => 0,
        };
        lines.push(format!(
            "seahorse_recall_recent_results_total{{source=\"{source}\"}} {value}"
        ));
    }

    append_metric_help(
        lines,
        "seahorse_recall_recent_association_gate_total",
        "Recent tagmemo association gate decisions.",
        "gauge",
    );
    lines.push(format!(
        "seahorse_recall_recent_association_gate_total{{decision=\"allowed\"}} {}",
        runtime.recall_telemetry.association_allowed_total
    ));
    lines.push(format!(
        "seahorse_recall_recent_association_gate_total{{decision=\"blocked\"}} {}",
        runtime.recall_telemetry.association_blocked_total
    ));
}

fn append_status_count_metric(
    lines: &mut Vec<String>,
    name: &str,
    help: &str,
    known_statuses: &[&str],
    values: &[StatusCountSnapshot],
) {
    append_metric_help(lines, name, help, "gauge");
    let mut seen_unknown = false;
    for status in known_statuses {
        let value = values
            .iter()
            .find(|entry| entry.status == *status)
            .map(|entry| entry.count)
            .unwrap_or(0);
        lines.push(format!("{name}{{status=\"{status}\"}} {value}"));
    }

    for value in values {
        if known_statuses.contains(&value.status.as_str()) {
            continue;
        }
        seen_unknown = true;
        lines.push(format!(
            "{name}{{status=\"{}\"}} {}",
            escape_label_value(&value.status),
            value.count
        ));
    }

    if !seen_unknown && values.is_empty() {
        lines.push(format!("{name}{{status=\"unknown\"}} 0"));
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

fn map_metrics_error(
    error: AppStateError,
) -> (
    StatusCode,
    [(axum::http::header::HeaderName, HeaderValue); 1],
    String,
) {
    let (status, body) = match error {
        AppStateError::Unavailable { message } => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("# metrics unavailable: {}\n", escape_label_value(&message)),
        ),
        AppStateError::Storage(source) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "# storage error: {}\n",
                escape_label_value(&source.to_string())
            ),
        ),
        AppStateError::NotFound { message } => (
            StatusCode::NOT_FOUND,
            format!("# metrics not found: {}\n", escape_label_value(&message)),
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
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static(PROMETHEUS_CONTENT_TYPE),
        )],
        body,
    )
}
