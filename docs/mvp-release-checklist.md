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
- repair queue 自动修复、deadletter、running recovery 已有自动化验证

## 7. 已补齐的 release gates

- runtime config 与 observability 契约验证已补齐
- contract / lifecycle / fault recovery 自动化测试已补齐
- `10k chunk` 性能 gate 已实现，结果记录在 `docs/reports/2026-03-26-mvp-release-readiness.md`
- 本机样本若波动过大，可使用 `SEAHORSE_PERF_RECORD_ONLY=1` 仅记录基线数据

## 8. 当前未完成项

以下项未完成时，不应将当前版本定义为“所有发布收口工作已结束”：

- 结构化请求日志完整接入
- 告警规则在监控平台正式落地（当前仅给出 MVP 阈值建议）
- README / docs index / runbook / release handoff 文档最终收口
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