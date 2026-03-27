# Seahorse

Seahorse 是一个基于 SQLite 的单 namespace MVP 记忆服务，提供 `ingest`、`recall`、`forget`、`rebuild`、`jobs`、`stats`、`health`、`metrics` 等 REST API。

## Crates

- `crates/seahorse-core`
  核心存储、向量索引、pipeline、repair job、rebuild 逻辑。
- `crates/seahorse-server`
  runtime wiring、HTTP handlers、配置加载、observability 与集成测试入口。

## 本地启动

```bash
cargo run -p seahorse-server
```

如需显式配置，可准备 `./config/seahorse.toml`；不提供时会使用默认配置。数据库默认位于 `./data/seahorse.db`。

## 文档入口

- [docs/README.md](docs/README.md)
- [docs/mvp-openapi.yaml](docs/mvp-openapi.yaml)
- [docs/mvp-release-checklist.md](docs/mvp-release-checklist.md)
- [docs/runbooks/mvp-deploy-backup-rollback.md](docs/runbooks/mvp-deploy-backup-rollback.md)
- [docs/reports/2026-03-26-mvp-release-readiness.md](docs/reports/2026-03-26-mvp-release-readiness.md)