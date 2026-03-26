# Seahorse MVP 发布检查清单

## 1. 范围确认

- 只发布 MVP 必须项
- 不包含 Tide、LIF、connectome、MCP、SDK、多租户增强
- 当前版本仍以 SQLite + vector recall + REST API 为边界

## 2. 代码与契约

- `docs/mvp-design-and-roadmap.md` 仍可作为当前实现基线
- `docs/mvp-openapi.yaml` 已覆盖当前对外 API
- 请求/响应结构仍使用统一包络：`success/data/error/request_id`
- 未新增设计文档中未定义的状态值或外部 API

## 3. 配置与运行前置

- 已确认数据库路径
- 已确认数据目录可写
- 已确认服务仅绑定本地或受信内网地址
- 已确认当前 embedding 配置与实现一致
- 已确认 migration 文件齐全
- 已确认 `./config/seahorse.toml` 可被读取（不存在时将走默认配置）
- 已确认 `[observability]` 中 `enable_metrics` / `metrics_path` 与预期一致
- 已确认 `metrics_path` 为空时回退 `/metrics`，不带前导 `/` 时会自动补齐

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
- 若开启 metrics：`GET /metrics`（或配置指定路径）可调用并返回 Prometheus 文本格式

建议至少完成一次人工链路：

1. ingest 一条文本
2. recall 命中该文本
3. forget 后 recall 不再返回
4. rebuild 成功完成
5. rebuild 后 health / stats 正常
6. 若开启 metrics，确认可抓取到核心指标：
   - `seahorse_http_requests_total`
   - `seahorse_http_request_errors_total`
   - `seahorse_http_request_latency_ms_max`
   - `seahorse_index_state`
   - `seahorse_health_status`

建议至少完成一组告警规则验证（手工或预发布环境）：

1. `seahorse_health_status{status="failed"} == 1` 可触发告警
2. `seahorse_index_state{state="degraded"} == 1` 持续触发告警
3. `error_rate > 5%`（基于 `request_errors_total / requests_total`）可触发告警
4. 告警恢复后可自动清除

## 6. 状态与可恢复性

- `files.ingest_status` 状态流转符合设计
- `chunks.index_status` 状态流转符合设计
- `schema_meta.index_state` 可反映 `ready / rebuilding / degraded`
- 重启后 active rebuild job 可恢复
- 多个 active rebuild job 启动恢复时仅保留最新一条

## 7. 当前代码已实现（待发布前复核）

- HTTP 主链路接口已实现：`POST /ingest`、`POST /recall`、`POST /forget`、`POST /admin/rebuild`、`GET /admin/jobs/{job_id}`、`GET /stats`、`GET /health`
- `/metrics` 已实现为正式运维接口；仅当 `enable_metrics=true` 时挂载，默认配置开启，默认路径为 `/metrics`，也可由 `[observability].metrics_path` 覆盖
- `/forget` 当前真实契约为 `mode=soft`；`hard` 不属于当前 MVP 正式发布契约
- SQLite 备份、回滚、rebuild、health / stats / metrics 的人工巡检路径已在现有文档中定义

## 8. 当前仍缺证据 / 缺验证（release blocker）

以下项在证据补齐前，不应将当前版本定义为“可发布版 MVP 已完成”：

- `repair_queue` 完整自动修复闭环的实现与恢复验证
- 结构化请求日志完整接入与发布验收记录
- 告警规则在监控平台落地并完成验证（当前仅提供 MVP 建议阈值）
- 自动化 contract / E2E / 故障注入测试
- 正式发布版编译 / 回归验证记录
- `1 万 chunk` 基线性能验收

## 9. 当前可接受结论

如果以下条件成立，可接受将当前版本定义为“开发闭环版 MVP”：

- 主链路可手工跑通
- rebuild 能提交、查询、恢复
- health / stats / metrics 可用于最小人工巡检
- SQLite 备份与回滚步骤明确

如果要定义为“可发布版 MVP”，则必须关闭第 8 节全部 release blocker，并保留对应验证证据。
