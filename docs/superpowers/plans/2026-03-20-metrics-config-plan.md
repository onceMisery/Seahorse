# Metrics Config Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a fixed config file (`./config/seahorse.toml`) to toggle `/metrics` and configure its path without changing existing API envelopes.

**Architecture:** Add a minimal config loader in `seahorse-server` that reads only `[observability]`. `main.rs` loads config before building the router and conditionally registers the metrics route.

**Tech Stack:** Rust, Axum, `toml` crate, serde.

---

### Task 1: Add Minimal Config Loader

**Files:**
- Create: `crates/seahorse-server/src/config.rs`
- Modify: `crates/seahorse-server/Cargo.toml`

- [ ] **Step 1: Define config structs and defaults**

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_enable_metrics")]
    pub enable_metrics: bool,
    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,
}

fn default_enable_metrics() -> bool { true }
fn default_metrics_path() -> String { "/metrics".to_owned() }

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enable_metrics: default_enable_metrics(),
            metrics_path: default_metrics_path(),
        }
    }
}
```

- [ ] **Step 2: Implement a loader for `./config/seahorse.toml`**

```rust
use std::fs;
use std::path::Path;

pub fn load_config() -> Result<ServerConfig, String> {
    let path = Path::new("./config/seahorse.toml");
    let contents = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ServerConfig::default());
        }
        Err(err) => return Err(format!("failed to read config {}: {err}", path.display())),
    };

    let mut config: ServerConfig = toml::from_str(&contents)
        .map_err(|err| format!("failed to parse config {}: {err}", path.display()))?;

    if config.observability.metrics_path.trim().is_empty() {
        config.observability.metrics_path = default_metrics_path();
    } else if !config.observability.metrics_path.starts_with('/') {
        config.observability.metrics_path = format!("/{}", config.observability.metrics_path);
    }

    Ok(config)
}
```

- [ ] **Step 3: Add `Default` impl**

```rust
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            observability: ObservabilityConfig::default(),
        }
    }
}
```

- [ ] **Step 4: Add `toml` dependency**

`crates/seahorse-server/Cargo.toml`:
```
[dependencies]
# ...
toml = "0.8"
```

- [ ] **Step 5: Tests**
Skipped per user request (no new tests for MVP).

### Task 2: Wire Config Into Router

**Files:**
- Modify: `crates/seahorse-server/src/main.rs`
- Modify: `crates/seahorse-server/src/handlers/mod.rs` (if needed)

- [ ] **Step 1: Load config before building the router**

```rust
mod config;

fn main() {
    // ...
    let config = config::load_config().expect("failed to load config");
    let app = build_app_with_config(state, &config);
}
```

- [ ] **Step 2: Add `build_app_with_config` and keep `build_app` for tests**

```rust
fn build_app(state: AppState) -> Router {
    build_app_with_config(state, &config::ServerConfig::default())
}

fn build_app_with_config(state: AppState, config: &config::ServerConfig) -> Router {
    let mut router = Router::new()
        .route("/ingest", post(...))
        // ...
        .route("/health", get(...))
        .with_state(state)
        .route_layer(axum::middleware::from_fn(
            api::observability::request_context_middleware,
        ));

    if config.observability.enable_metrics {
        router = router.route(&config.observability.metrics_path, get(handlers::metrics::get_metrics));
    }

    router
}
```

- [ ] **Step 3: Tests**
Skipped per user request.

### Task 3: Documentation Note

**Files:**
- Optional: `docs/mvp-config.example.toml`

- [ ] **Step 1: Ensure example matches behavior**
No change required unless you want explicit comments. If adding, keep ASCII only.

### Verification
- [ ] **Step 1: Static check**
Run: `git diff --check -- crates/seahorse-server/src/config.rs crates/seahorse-server/src/main.rs crates/seahorse-server/Cargo.toml`
Expected: no errors (CRLF warnings are acceptable).

- [ ] **Step 2: Build check**
Skipped (Rust toolchain not configured in this environment).

### Commit
- [ ] **Step: Commit changes**
Skip unless requested.
