# Seahorse MVP 发布交接（2026-04-22）

## 1. 当前状态

- 当前代码可定义为“发布候选 MVP”
- 代码侧主链路、恢复链路、可观测性与运行文档已收口
- 当前剩余 blocker 只剩外部环境相关项，不再是仓库内缺功能

## 2. 已完成能力

- 主链路接口：`POST /ingest`、`POST /recall`、`POST /forget`
- 后台任务：`POST /admin/rebuild`、`GET /admin/jobs/{job_id}`
- 巡检接口：`GET /health`、`GET /stats`
- 平台探针：`GET /ready`、`GET /live`
- 运维指标：`GET /metrics`
  - HTTP 请求量、错误量、延迟
  - repair queue 状态分布
  - rebuild job 状态分布
  - repair oldest task age
  - rebuild oldest active job age
  - index_state / health_status
- 结构化日志：
  - request span / request_id
  - request.start / request.end
  - ingest / recall / forget / rebuild / jobs 关键链路事件
  - repair / rebuild 后台状态事件
- 契约边界已收紧：
  - `ingest.options.chunk_mode` 仅支持 `fixed`
  - `ingest.options.auto_tag` 已实现规则型自动打标
  - `recall.timeout_ms` 已实现并返回 `504 TIMEOUT`
  - `forget.mode` 仅支持 `soft`

## 3. 已验证证据

- `cargo fmt`
- `cargo test -p seahorse-core --lib -- --nocapture`
- `cargo test -p seahorse-server -- --nocapture`
- `powershell -File scripts/check-mvp-docs.ps1`

## 4. 当前文档基线

- API 契约：
  - `docs/mvp-openapi.yaml`
- 设计与范围：
  - `docs/mvp-design-and-roadmap.md`
- 发布检查：
  - `docs/mvp-release-checklist.md`
- 运行手册：
  - `docs/runbooks/mvp-deploy-backup-rollback.md`
  - `docs/runbooks/mvp-release-execution-checklist.md`
  - `docs/runbooks/mvp-logging-validation-record-template.md`
- 告警样例：
  - `docs/runbooks/mvp-alert-rules.example.yaml`
- readiness 证据：
  - `docs/reports/2026-03-26-mvp-release-readiness.md`

## 5. 剩余 blocker

- 监控平台中正式导入并启用告警规则
- 验证日志采集、落库、检索链路，而不只是 stdout JSON 输出
- 在 release 机器上复测并通过 `10k chunk` hard gate

## 6. 建议的发布前最后步骤

1. 以 release 配置启动服务
2. 运行 `powershell -File scripts/run-mvp-release-validation.ps1`
3. 导入 `docs/runbooks/mvp-alert-rules.example.yaml` 到监控平台
4. 按 `docs/runbooks/mvp-logging-validation-record-template.md` 补齐日志链路验证记录
5. 在 release 机器执行 `cargo test -p seahorse-server perf_baseline_10k -- --ignored --nocapture`
6. 归档最终命令输出、监控截图和告警验证记录
