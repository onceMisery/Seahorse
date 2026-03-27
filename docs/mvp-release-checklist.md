# Seahorse MVP 发布检查清单

## 1. 范围确认

- 只发布 MVP 必需能力
- 不包含 Tide、LIF、connectome、MCP、SDK、多租户增强
- 当前版本仍以 SQLite + vector recall + REST API 为边界

## 2. 代码与契约

- `docs/mvp-design-and-roadmap.md` 仍可作为当前实现基线
- `docs/mvp-openapi.yaml` 已覆盖当前对外 API
- 请求/响应结构继续使用统一包络：`success/data/error/request_id`
- 未新增设计文档中未定义的状态值或外部 API

## 3. 配置与运行前置

- 已确认数据库路径
- 已确认数据目录可写
- 已确认服务仅绑定本地或受信内网地址
- 已确认当前 embedding 配置与实现一致
- 已确认 migration 文件齐全
- 已确认 `./config/seahorse.toml` 可被读取，不存在时走默认配置
- 已确认 `[observability]` 中 `enable_metrics` / `metrics_path` 与预期一致
- 已确认 `metrics_path` 为空时回退 `/metrics`，缺少前导 `/` 时会自动补齐

## 4. 存储与恢复

- 已执行 SQLite 备份
- 已记录备份文件位置和时间
- 已确认回滚时使用的稳定版本
- 已准备 rebuild 操作步骤

## 5. 主链路验收

- `POST /ingest` 可调用
- `POST /recall` 可调用
- `POST /forget` 可调用
- `POST /admin/rebuild` 可调用
- `GET /admin/jobs/{job_id}` 可调用
- `GET /health` 可调用
- `GET /stats` 可调用
- 若开启 metrics，`GET /metrics` 或配置指定路径可抓取 Prometheus 文本格式

建议至少完成一次人工链路：

1. ingest 一条文本
2. recall 命中该文本
3. forget 后 recall 不再返回
4. rebuild 成功完成
5. rebuild 后 health / stats 正常
6. 若开启 metrics，确认可抓到核心指标：
   - `seahorse_http_requests_total`
   - `seahorse_http_request_errors_total`
   - `seahorse_http_request_latency_ms_max`
   - `seahorse_index_state`
   - `seahorse_health_status`

## 6. 状态与可恢复性

- `files.ingest_status` 状态流转符合设计
- `chunks.index_status` 状态流转符合设计
- `schema_meta.index_state` 可反映 `ready / rebuilding / degraded`
- 重启后 active rebuild job 可恢复
- 多个 active rebuild job 启动恢复时仅保留最新一条

## 7. 当前代码已实现（含已关闭的 release blocker）

- HTTP 主链路接口已实现：`POST /ingest`、`POST /recall`、`POST /forget`、`POST /admin/rebuild`、`GET /admin/jobs/{job_id}`、`GET /stats`、`GET /health`
- `/metrics` 已作为正式运维接口实现；仅当 `enable_metrics=true` 时挂载，默认配置开启，默认路径为 `/metrics`，也可由 `observability.metrics_path` 覆盖
- `POST /forget` 当前正式契约固定为 `mode=soft`，`hard` 不属于当前 MVP 发布契约
- SQLite 备份、回滚、rebuild、health / stats / metrics 的人工巡检路径已在现有文档中定义

## 8. 当前仍缺证据 / 缺验证

以下 release blocker 需要区分“已关闭”和“仍未关闭”：

已关闭的 release blocker 证据：

- `contract`: `cargo test -p seahorse-server -- --nocapture` 已通过，覆盖 API contract 与 runtime config
- `E2E`: `cargo test -p seahorse-server -- --nocapture` 已通过，覆盖 lifecycle roundtrip 与 rebuild 查询链路
- `故障注入`: `cargo test -p seahorse-server --test fault_recovery -- --nocapture` 已通过，覆盖 repair success / deadletter / running recovery / rebuild fallback
- `10k chunk` 性能 gate 已存在：`cargo test -p seahorse-server perf_baseline_10k -- --ignored --nocapture`
  - 当前本机 hard gate 样本未通过，详见 `docs/reports/2026-03-26-mvp-release-readiness.md`
  - 本机如需只记录数据点，可使用 `SEAHORSE_PERF_RECORD_ONLY=1`

当前仍未关闭的 release blocker：

- 结构化请求日志完整接入
- 告警规则在监控平台正式落地（当前仅给出 MVP 阈值建议）
- release 环境上的 `10k chunk` hard gate 最终过线确认

## 9. 当前结论

如果以下条件成立，可接受将当前版本定义为“发布候选 MVP”：

- 主链路可手工跑通
- rebuild / repair / recovery 行为已自动化验证
- health / stats / metrics 可用于最小巡检
- 性能 gate 已存在且基线数据已归档
- 备份与回滚步骤明确

如果要定义为“最终可交付的发布型 MVP”，还需要补齐：

1. operator docs 与 release handoff 文档收口
2. 监控平台中的告警规则落地
3. release 环境上的 `10k chunk` hard gate 通过确认
4. 最终发布材料回填与归档