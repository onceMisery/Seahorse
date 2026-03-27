# Seahorse MVP 部署、备份与回滚手册

## 1. 适用范围

本手册仅适用于当前 Seahorse MVP：

- 单 namespace，固定为 `default`
- SQLite 作为唯一事实源
- server 通过 REST API 暴露 `ingest` / `recall` / `forget` / `rebuild` / `jobs` / `stats` / `health` / `metrics`
- 部署目标为本地环境或受信内网

当前代码已实现并可按本手册执行的能力：

- `/metrics` 已作为正式运维接口实现；仅当 `enable_metrics=true` 时挂载，默认配置开启，默认路径为 `/metrics`，也可由 `observability.metrics_path` 覆盖
- `POST /forget` 当前正式契约固定为 `mode=soft`，`hard` 不属于当前 MVP 发布契约
- `health` / `stats` / `metrics` 可用于最小人工巡检，`rebuild` / `jobs` 可用于恢复路径操作
- rebuild 启动恢复、repair queue 故障恢复、fault recovery 自动化验证都已有证据

## 2. 部署前检查

- `docs/mvp-openapi.yaml` 与当前 API 语义一致
- SQLite 数据目录可写
- `./config/seahorse.toml` 已按环境调整，或确认默认配置可接受
- 服务仅绑定本地或受信内网地址
- Prometheus 抓取路径与 `enable_metrics` / `metrics_path` 配置一致

## 3. 首次部署

1. 准备工作目录，例如 `./data/`
2. 设置数据库路径，例如 `./data/seahorse.db`
3. 准备 `./config/seahorse.toml`，或使用默认配置启动
4. 启动服务并等待 SQLite migration 自动执行完成
5. 检查：
   - `GET /health`
   - `GET /stats`
   - 若开启 metrics，再检查 `GET /metrics`
6. 手工执行一次最小链路：
   - `POST /ingest`
   - `POST /recall`
   - `POST /forget`
   - `POST /admin/rebuild`
   - `GET /admin/jobs/{job_id}`

## 4. 日常巡检

最小巡检项：

- `GET /health`
  - `ok`: 当前实例可正常服务
  - `degraded`: 常见于 rebuild 期间或索引存在待修复项
  - `failed`: 当前实例不应继续提供正常流量
- `GET /stats`
  - 关注 `chunk_count`、`deleted_chunk_count`、`repair_queue_size`、`index_status`
- `GET /metrics`
  - 关注 `seahorse_http_requests_total`
  - 关注 `seahorse_http_request_errors_total`
  - 关注 `seahorse_http_request_latency_ms_max`
  - 关注 `seahorse_index_state`
  - 关注 `seahorse_health_status`

## 5. SQLite 备份

1. 停止写流量
2. 确认没有主动 rebuild 作业正在运行
3. 复制 SQLite 文件到备份目录
4. 记录备份时间、文件大小、对应版本

建议至少保留：

- 最近一次部署前备份
- 最近一次部署后基线备份
- 最近一次 rebuild 前备份

## 6. 回滚

适用场景：

- 新版本启动后 `health` 异常
- 主链路 `ingest` / `recall` 出现明显回归
- rebuild 作业异常导致服务持续不可用

最小回滚步骤：

1. 停止当前服务实例
2. 保留当前故障数据库文件，重命名为诊断副本
3. 恢复最近一次可用备份的 SQLite 文件
4. 使用上一稳定版本重新启动
5. 依次检查：
   - `GET /health`
   - `GET /stats`
   - 最小 `ingest -> recall` 链路

## 7. Rebuild 操作

1. 调用 `POST /admin/rebuild`
2. 记录返回的 `job_id`
3. 轮询 `GET /admin/jobs/{job_id}`
4. 等待作业进入终态：`succeeded` / `failed` / `cancelled`

注意：

- 同一 namespace 同时只应保留一个有效 rebuild
- 如需替换当前 rebuild，请显式使用 `force=true`
- 服务重启后会尝试恢复 active rebuild job，并仅保留最新一条 active job

## 8. 当前仍缺证据 / 缺验证

以下 release blocker 需要在交接时明确说明：

- `contract` / `E2E` / `故障注入` 自动化测试已经补齐，但要保留对应命令与结果作为交接证据
- release 环境上的 `10k chunk` hard gate 仍需最终过线确认
- 结构化请求日志完整接入尚未完成
- 告警规则虽然已有阈值建议，但还未在统一监控平台落地

## 9. 当前定位

当前版本可作为“发布候选 MVP”运行与交接，但在关闭全部 release blocker 之前，不应直接宣称为“最终可交付的发布型 MVP”。