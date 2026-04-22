use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, MutexGuard,
};
use std::time::{SystemTime, UNIX_EPOCH};

use seahorse_server::{
    config::{load_server_config, load_server_config_default, ApiConfig, ServerConfig},
    state::AppState,
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn loads_runtime_config_from_toml() {
    let config = load_server_config(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/seahorse.test.toml"),
    )
    .unwrap();

    assert_eq!(config.storage.db_path, "./tmp/runtime.db");
    assert_eq!(config.api.listen_addr(), "127.0.0.1:18080");
    assert_eq!(config.observability.log_level, "debug");
    assert_eq!(config.observability.metrics_path, "/internal/metrics");
    assert_eq!(config.jobs.repair_max_retries, 7);
    assert_eq!(config.jobs.repair_batch_size, 3);
    assert_eq!(config.embedding.dimension, 32);
}

#[test]
fn runtime_config_rejects_unsupported_documented_keys() {
    let path = write_temp_config(
        r#"
[storage]
db_path = "./tmp/runtime.db"
migrations_dir = "./migrations"
namespace = "default"
enable_wal = true
busy_timeout_ms = 5000

[embedding]
provider = "stub"
model_id = "local-dev-embedding-v1"
dimension = 48
timeout_ms = 15000
max_batch_size = 32

[index]
provider = "hnsw"
ef_search = 64
ef_construction = 200
m = 16
enable_visibility_filter = true

[pipeline]
default_top_k = 10
max_top_k = 20
max_content_bytes = 1048576
max_tag_count = 32
max_tag_length = 64
max_metadata_bytes = 16384
default_chunk_mode = "fixed"
default_dedup_mode = "reject"

[jobs]
repair_max_retries = 5
repair_batch_size = 2
rebuild_max_concurrency = 1
rebuild_batch_size = 128

[api]
host = "127.0.0.1"
port = 18080
request_timeout_ms = 30000
expose_admin_endpoints = true

[observability]
log_level = "debug"
enable_metrics = true
metrics_path = "/internal/metrics"
health_path = "/health"

[runtime]
environment = "local"
allow_public_bind = false
"#,
    );

    let error = load_server_config(&path).unwrap_err();
    assert!(
        error.contains("unsupported config field") || error.contains("unsupported config section"),
        "unexpected error: {error}"
    );

    cleanup_temp_config(&path);
}

#[test]
fn runtime_config_rejects_unknown_field_in_known_section() {
    let path = write_temp_config(
        r#"
[storage]
db_path = "./tmp/runtime.db"
unexpected = true
"#,
    );

    let error = load_server_config(&path).unwrap_err();

    assert!(
        error.contains("unknown field") && error.contains("unexpected"),
        "unexpected error: {error}"
    );

    cleanup_temp_config(&path);
}

#[test]
fn runtime_config_rejects_unknown_top_level_section() {
    let path = write_temp_config(
        r#"
[storage]
db_path = "./tmp/runtime.db"

[unexpected]
enabled = true
"#,
    );

    let error = load_server_config(&path).unwrap_err();

    assert!(
        error.contains("unknown field") && error.contains("unexpected"),
        "unexpected error: {error}"
    );

    cleanup_temp_config(&path);
}

#[test]
fn runtime_config_formats_ipv6_listen_addr() {
    let config = ApiConfig {
        host: "::1".to_owned(),
        port: 18080,
    };

    assert_eq!(config.listen_addr(), "[::1]:18080");
}

#[test]
fn runtime_config_applies_embedding_dimension_to_app_state() {
    let mut config = ServerConfig::default();
    config.storage.db_path = ":memory:".to_owned();
    config.embedding.dimension = 32;

    let state = AppState::new_with_config(&config).unwrap();
    let health = state.health_snapshot().unwrap();

    assert_eq!(health.index, "memory-32d");
}

#[test]
fn default_runtime_config_applies_legacy_db_path_env_override() {
    let _guard = LegacyEnvGuard::new(Some("./tmp/legacy-env.db"), None);

    let config = load_server_config_default().unwrap();

    assert_eq!(config.storage.db_path, "./tmp/legacy-env.db");
}

#[test]
fn default_runtime_config_applies_legacy_server_addr_env_override() {
    let _guard = LegacyEnvGuard::new(None, Some("127.0.0.1:19090"));

    let config = load_server_config_default().unwrap();

    assert_eq!(config.api.host, "127.0.0.1");
    assert_eq!(config.api.port, 19090);
    assert_eq!(config.api.listen_addr(), "127.0.0.1:19090");
}

#[test]
fn default_runtime_config_applies_bracketed_ipv6_server_addr_env_override() {
    let _guard = LegacyEnvGuard::new(None, Some("[::1]:19091"));

    let config = load_server_config_default().unwrap();

    assert_eq!(config.api.host, "::1");
    assert_eq!(config.api.port, 19091);
    assert_eq!(config.api.listen_addr(), "[::1]:19091");
}

fn write_temp_config(contents: &str) -> PathBuf {
    let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis();
    let path =
        std::env::temp_dir().join(format!("seahorse-runtime-config-{millis}-{counter}.toml"));
    fs::write(&path, contents).expect("write temp config");
    path
}

fn cleanup_temp_config(path: &Path) {
    let _ = fs::remove_file(path);
}

struct LegacyEnvGuard {
    old_db_path: Option<OsString>,
    old_server_addr: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl LegacyEnvGuard {
    fn new(db_path: Option<&str>, server_addr: Option<&str>) -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let old_db_path = std::env::var_os("SEAHORSE_DB_PATH");
        let old_server_addr = std::env::var_os("SEAHORSE_SERVER_ADDR");

        set_env_var("SEAHORSE_DB_PATH", db_path);
        set_env_var("SEAHORSE_SERVER_ADDR", server_addr);

        Self {
            old_db_path,
            old_server_addr,
            _lock: lock,
        }
    }
}

impl Drop for LegacyEnvGuard {
    fn drop(&mut self) {
        restore_env_var("SEAHORSE_DB_PATH", self.old_db_path.clone());
        restore_env_var("SEAHORSE_SERVER_ADDR", self.old_server_addr.clone());
    }
}

fn set_env_var(key: &str, value: Option<&str>) {
    match value {
        Some(value) => unsafe { std::env::set_var(key, value) },
        None => unsafe { std::env::remove_var(key) },
    }
}

fn restore_env_var(key: &str, value: Option<OsString>) {
    match value {
        Some(value) => unsafe { std::env::set_var(key, value) },
        None => unsafe { std::env::remove_var(key) },
    }
}
