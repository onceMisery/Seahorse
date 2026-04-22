use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::MatchedPath;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use tokio::task_local;
use tracing::{error, info, info_span, warn, Instrument};

task_local! {
    static REQUEST_ID: String;
}

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);
static REQUEST_METRICS: OnceLock<Mutex<RequestMetricsStore>> = OnceLock::new();

#[derive(Debug, Clone, Default)]
struct RequestMetrics {
    request_total: u64,
    request_error_total: u64,
    request_latency_ms_sum: u64,
    request_latency_ms_max: u64,
}

#[derive(Debug, Default)]
struct RequestMetricsStore {
    total: RequestMetrics,
    by_route: HashMap<String, RequestMetrics>,
}

#[derive(Debug, Clone, Default)]
pub struct RequestMetricsSnapshot {
    pub total: RequestMetricsValues,
    pub by_route: Vec<RouteRequestMetricsSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct RequestMetricsValues {
    pub request_total: u64,
    pub request_error_total: u64,
    pub request_latency_ms_sum: u64,
    pub request_latency_ms_max: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RouteRequestMetricsSnapshot {
    pub method: String,
    pub route: String,
    pub values: RequestMetricsValues,
}

#[derive(Debug, Clone)]
pub struct RequestId;

pub fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(Clone::clone).ok()
}

pub fn request_metrics_snapshot() -> RequestMetricsSnapshot {
    let store = metrics_store()
        .lock()
        .expect("request metrics mutex poisoned");
    let mut by_route = store
        .by_route
        .iter()
        .map(|(route_key, metrics)| {
            let (method, route) = route_key
                .split_once(' ')
                .map(|(method, route)| (method.to_owned(), route.to_owned()))
                .unwrap_or_else(|| ("UNKNOWN".to_owned(), route_key.clone()));
            RouteRequestMetricsSnapshot {
                method,
                route,
                values: metrics.into(),
            }
        })
        .collect::<Vec<_>>();

    by_route.sort_by(|left, right| {
        left.method
            .cmp(&right.method)
            .then_with(|| left.route.cmp(&right.route))
    });

    RequestMetricsSnapshot {
        total: (&store.total).into(),
        by_route,
    }
}

pub async fn request_context_middleware(mut request: Request, next: Next) -> Response {
    let request_id = next_request_id();
    request.extensions_mut().insert(RequestId);
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned())
        .unwrap_or_else(|| path.clone());
    let started = Instant::now();
    let request_span = info_span!(
        "http.request",
        request_id = %request_id,
        method = %method,
        route = %route,
        path = %path,
    );

    REQUEST_ID
        .scope(
            request_id.clone(),
            async move {
                info!(event = "request.start", "http request started");
                let response = next.run(request).await;
                let status = response.status().as_u16();
                let latency_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                record_request_metrics(&method, &route, status, latency_ms);

                if status >= 500 {
                    error!(
                        event = "request.end",
                        status = status,
                        latency_ms = latency_ms,
                        "http request completed with server error"
                    );
                } else if status >= 400 {
                    warn!(
                        event = "request.end",
                        status = status,
                        latency_ms = latency_ms,
                        "http request completed with client error"
                    );
                } else {
                    info!(
                        event = "request.end",
                        status = status,
                        latency_ms = latency_ms,
                        "http request completed"
                    );
                }

                response
            }
            .instrument(request_span),
        )
        .await
}

fn next_request_id() -> String {
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("req-{millis}-{counter}")
}

fn record_request_metrics(method: &str, route: &str, status: u16, latency_ms: u64) {
    let route_key = format!("{method} {route}");
    let mut store = metrics_store()
        .lock()
        .expect("request metrics mutex poisoned");

    update_metrics_entry(&mut store.total, status, latency_ms);
    let route_metrics = store.by_route.entry(route_key).or_default();
    update_metrics_entry(route_metrics, status, latency_ms);
}

fn update_metrics_entry(metrics: &mut RequestMetrics, status: u16, latency_ms: u64) {
    metrics.request_total = metrics.request_total.saturating_add(1);
    if status >= 400 {
        metrics.request_error_total = metrics.request_error_total.saturating_add(1);
    }
    metrics.request_latency_ms_sum = metrics.request_latency_ms_sum.saturating_add(latency_ms);
    metrics.request_latency_ms_max = metrics.request_latency_ms_max.max(latency_ms);
}

fn metrics_store() -> &'static Mutex<RequestMetricsStore> {
    REQUEST_METRICS.get_or_init(|| Mutex::new(RequestMetricsStore::default()))
}

impl From<&RequestMetrics> for RequestMetricsValues {
    fn from(value: &RequestMetrics) -> Self {
        Self {
            request_total: value.request_total,
            request_error_total: value.request_error_total,
            request_latency_ms_sum: value.request_latency_ms_sum,
            request_latency_ms_max: value.request_latency_ms_max,
        }
    }
}
