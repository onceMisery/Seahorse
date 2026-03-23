use std::fs;
use std::path::Path;

use serde::Deserialize;

const CONFIG_PATH: &str = "./config/seahorse.toml";
const DEFAULT_METRICS_PATH: &str = "/metrics";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityConfig {
    pub enable_metrics: bool,
    pub metrics_path: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enable_metrics: true,
            metrics_path: DEFAULT_METRICS_PATH.to_owned(),
        }
    }
}

pub fn load_observability_config() -> ObservabilityConfig {
    let path = Path::new(CONFIG_PATH);
    if !path.exists() {
        return ObservabilityConfig::default();
    }

    let content = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("failed to read config file {}: {error}", path.display())
    });
    let parsed: SeahorseConfigFile = toml::from_str(&content).unwrap_or_else(|error| {
        panic!("failed to parse config file {}: {error}", path.display())
    });

    let mut config = ObservabilityConfig::default();
    if let Some(observability) = parsed.observability {
        if let Some(enable_metrics) = observability.enable_metrics {
            config.enable_metrics = enable_metrics;
        }
        if let Some(metrics_path) = observability.metrics_path {
            config.metrics_path = normalize_metrics_path(&metrics_path);
        }
    }

    config
}

fn normalize_metrics_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return DEFAULT_METRICS_PATH.to_owned();
    }

    if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    }
}

#[derive(Debug, Deserialize, Default)]
struct SeahorseConfigFile {
    observability: Option<RawObservabilityConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct RawObservabilityConfig {
    enable_metrics: Option<bool>,
    metrics_path: Option<String>,
}
