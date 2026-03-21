# Metrics Config Switch Design

**Date:** 2026-03-20

## Goal
Enable `/metrics` via a fixed config file (`./config/seahorse.toml`) using an `observability.enable_metrics` switch and a configurable `observability.metrics_path`, while keeping the current HTTP response envelope unchanged.

## Scope
- Read `./config/seahorse.toml` on startup.
- Parse only `[observability]`:
  - `enable_metrics` (bool)
  - `metrics_path` (string)
- If file is missing: use defaults (`enable_metrics = true`, `metrics_path = "/metrics"`).
- If file exists but is invalid: fail startup with a clear error.
- When disabled, do not register the metrics route.

## Non-Goals
- No full configuration system.
- No environment variable overrides.
- No external metrics backend integration.
- No authentication/authorization for `/metrics`.

## Config Format
Example (`./config/seahorse.toml`):

```toml
[observability]
enable_metrics = true
metrics_path = "/metrics"
```

## Behavior
- Load config at process start before router construction.
- If `metrics_path` is empty: fall back to default `/metrics`.
- If `metrics_path` does not start with `/`: prefix `/` to keep a valid route.
- If `enable_metrics` is `false`: metrics route is not mounted.

## Error Handling
- Missing config file: proceed with defaults.
- Parse error: panic with message including file path and parse failure.

## Compatibility
- Keep existing JSON response envelope unchanged.
- `/health`, `/stats`, and other routes remain unchanged.

## Rollout Notes
- Deploy with no config file to keep current `/metrics` behavior.
- Add `./config/seahorse.toml` in environments that need a toggle or path change.
