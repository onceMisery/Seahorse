use std::fs;
use std::net::Ipv6Addr;
use std::path::Path;

use seahorse_core::RepairWorkerConfig;
use serde::Deserialize;

const CONFIG_PATH: &str = "./config/seahorse.toml";
const DEFAULT_DB_PATH: &str = "./data/seahorse.db";
const DEFAULT_API_HOST: &str = "127.0.0.1";
const DEFAULT_API_PORT: u16 = 8080;
const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_METRICS_PATH: &str = "/metrics";
const DEFAULT_EMBEDDING_DIMENSION: usize = 1024;
const DEFAULT_REPAIR_MAX_RETRIES: u32 = 3;
const DEFAULT_REPAIR_BATCH_SIZE: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub storage: StorageConfig,
    pub api: ApiConfig,
    pub embedding: EmbeddingConfig,
    pub observability: ObservabilityConfig,
    pub jobs: JobsConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            api: ApiConfig::default(),
            embedding: EmbeddingConfig::default(),
            observability: ObservabilityConfig::default(),
            jobs: JobsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageConfig {
    pub db_path: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_path: DEFAULT_DB_PATH.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
}

impl ApiConfig {
    pub fn listen_addr(&self) -> String {
        if self.host.parse::<Ipv6Addr>().is_ok() {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_API_HOST.to_owned(),
            port: DEFAULT_API_PORT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingConfig {
    pub dimension: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            dimension: DEFAULT_EMBEDDING_DIMENSION,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityConfig {
    pub log_level: String,
    pub enable_metrics: bool,
    pub metrics_path: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            log_level: DEFAULT_LOG_LEVEL.to_owned(),
            enable_metrics: true,
            metrics_path: DEFAULT_METRICS_PATH.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobsConfig {
    pub repair_max_retries: u32,
    pub repair_batch_size: usize,
}

impl JobsConfig {
    pub fn repair_worker_config(&self) -> RepairWorkerConfig {
        RepairWorkerConfig {
            max_retries: self.repair_max_retries,
            batch_size: self.repair_batch_size,
        }
    }
}

impl Default for JobsConfig {
    fn default() -> Self {
        Self {
            repair_max_retries: DEFAULT_REPAIR_MAX_RETRIES,
            repair_batch_size: DEFAULT_REPAIR_BATCH_SIZE,
        }
    }
}

pub fn load_server_config(path: impl AsRef<Path>) -> Result<ServerConfig, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|error| format!("failed to read config file {}: {error}", path.display()))?;
    let config = parse_server_config(&content)
        .map_err(|error| format!("failed to parse config file {}: {error}", path.display()))?;
    apply_legacy_env_overrides(config)
}

pub fn load_server_config_default() -> Result<ServerConfig, String> {
    let path = Path::new(CONFIG_PATH);
    if !path.exists() {
        return apply_legacy_env_overrides(ServerConfig::default());
    }

    load_server_config(path)
}

pub fn load_observability_config() -> ObservabilityConfig {
    load_server_config_default()
        .map(|config| config.observability)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn parse_server_config(content: &str) -> Result<ServerConfig, toml::de::Error> {
    let parsed: SeahorseConfigFile = toml::from_str(content)?;
    let mut config = ServerConfig::default();

    if let Some(storage) = parsed.storage {
        if let Some(db_path) = storage.db_path {
            config.storage.db_path = db_path;
        }
    }

    if let Some(api) = parsed.api {
        if let Some(host) = api.host {
            config.api.host = host;
        }
        if let Some(port) = api.port {
            config.api.port = port;
        }
    }

    if let Some(embedding) = parsed.embedding {
        if let Some(dimension) = embedding.dimension {
            config.embedding.dimension = dimension;
        }
    }

    if let Some(observability) = parsed.observability {
        if let Some(log_level) = observability.log_level {
            config.observability.log_level = log_level;
        }
        if let Some(enable_metrics) = observability.enable_metrics {
            config.observability.enable_metrics = enable_metrics;
        }
        if let Some(metrics_path) = observability.metrics_path {
            config.observability.metrics_path = normalize_metrics_path(&metrics_path);
        }
    }

    if let Some(jobs) = parsed.jobs {
        if let Some(repair_max_retries) = jobs.repair_max_retries {
            config.jobs.repair_max_retries = repair_max_retries;
        }
        if let Some(repair_batch_size) = jobs.repair_batch_size {
            config.jobs.repair_batch_size = repair_batch_size;
        }
    }

    Ok(config)
}

fn apply_legacy_env_overrides(mut config: ServerConfig) -> Result<ServerConfig, String> {
    if let Ok(db_path) = std::env::var("SEAHORSE_DB_PATH") {
        config.storage.db_path = db_path;
    }

    if let Ok(server_addr) = std::env::var("SEAHORSE_SERVER_ADDR") {
        let (host, port) = parse_legacy_server_addr(&server_addr)?;
        config.api.host = host;
        config.api.port = port;
    }

    Ok(config)
}

fn parse_legacy_server_addr(addr: &str) -> Result<(String, u16), String> {
    let trimmed = addr.trim();
    if trimmed.is_empty() {
        return Err("SEAHORSE_SERVER_ADDR must not be empty".to_owned());
    }

    if let Some(remainder) = trimmed.strip_prefix('[') {
        let (host, port_fragment) = remainder.split_once(']').ok_or_else(|| {
            format!("SEAHORSE_SERVER_ADDR must use [host]:port format, got {trimmed}")
        })?;
        let port = port_fragment.strip_prefix(':').ok_or_else(|| {
            format!("SEAHORSE_SERVER_ADDR must use [host]:port format, got {trimmed}")
        })?;
        return Ok((host.to_owned(), parse_legacy_port(trimmed, port)?));
    }

    let (host, port) = trimmed.rsplit_once(':').ok_or_else(|| {
        format!("SEAHORSE_SERVER_ADDR must use host:port format, got {trimmed}")
    })?;
    if host.is_empty() {
        return Err(format!(
            "SEAHORSE_SERVER_ADDR must include a host before the port, got {trimmed}"
        ));
    }

    Ok((host.to_owned(), parse_legacy_port(trimmed, port)?))
}

fn parse_legacy_port(addr: &str, port: &str) -> Result<u16, String> {
    port.parse::<u16>()
        .map_err(|error| format!("SEAHORSE_SERVER_ADDR has invalid port in {addr}: {error}"))
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

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SeahorseConfigFile {
    storage: Option<RawStorageConfig>,
    api: Option<RawApiConfig>,
    embedding: Option<RawEmbeddingConfig>,
    index: Option<RawIndexConfig>,
    pipeline: Option<RawPipelineConfig>,
    observability: Option<RawObservabilityConfig>,
    jobs: Option<RawJobsConfig>,
    runtime: Option<RawRuntimeConfig>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawStorageConfig {
    db_path: Option<String>,
    migrations_dir: Option<String>,
    namespace: Option<String>,
    enable_wal: Option<bool>,
    busy_timeout_ms: Option<u64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawApiConfig {
    host: Option<String>,
    port: Option<u16>,
    request_timeout_ms: Option<u64>,
    expose_admin_endpoints: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawEmbeddingConfig {
    provider: Option<String>,
    model_id: Option<String>,
    dimension: Option<usize>,
    timeout_ms: Option<u64>,
    max_batch_size: Option<usize>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawIndexConfig {
    provider: Option<String>,
    ef_search: Option<usize>,
    ef_construction: Option<usize>,
    m: Option<usize>,
    enable_visibility_filter: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawPipelineConfig {
    default_top_k: Option<usize>,
    max_top_k: Option<usize>,
    max_content_bytes: Option<usize>,
    max_tag_count: Option<usize>,
    max_tag_length: Option<usize>,
    max_metadata_bytes: Option<usize>,
    default_chunk_mode: Option<String>,
    default_dedup_mode: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawObservabilityConfig {
    log_level: Option<String>,
    enable_metrics: Option<bool>,
    metrics_path: Option<String>,
    health_path: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawJobsConfig {
    repair_max_retries: Option<u32>,
    repair_batch_size: Option<usize>,
    rebuild_max_concurrency: Option<usize>,
    rebuild_batch_size: Option<usize>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawRuntimeConfig {
    environment: Option<String>,
    allow_public_bind: Option<bool>,
}
