# Seahorse MVP 部署、备份与回滚手册

## 1. 适用范围

本手册仅适用于当前 Seahorse MVP：

- 单 namespace，固定为 `default`
- SQLite 作为唯一事实源
- server 通过 REST API 暴露 `ingest` / `recall` / `forget` / `rebuild` / `jobs` / `stats` / `health` / `metrics`
- 部署目标为本地环境或受信内网

当前代码已实现并可按本手册执行的能力：

- `/metrics` 已作为正式运维接口实现；仅当 `observability.enable_metrics=true` 时挂载，默认配置开启；默认路径为 `/metrics`，如配置 `observability.metrics_path`，则以配置路径挂载
- `POST /forget` 当前真实契约按 `mode=soft` 执行；`hard` 不属于当前 MVP 正式发布契约
- `health` / `stats` / `metrics` 可用于最小人工巡检，`rebuild` / `jobs` 可用于恢复路径操作

以下事项当前仍缺证据 / 缺验证，不应被视为“可发布版 MVP 已完成”：

- `repair_queue` 仅具备最小状态机与 worker 框架，尚未接入完整运行调度
- observability 已提供基础请求日志与 Prometheus 文本指标，尚未接入分位数/直方图等高级观测能力
- 当前未在可用 Rust 工具链环境下完成编译、自动化测试与性能验收

## 2. 部署前检查

发布前至少确认以下条件：

- `docs/mvp-openapi.yaml` 与当前 API 语义一致
- `migrations/0001_init.sql`、`migrations/0002_relax_dedup_constraints.sql` 已纳入发布包
- `docs/mvp-config.example.toml` 中的路径和参数已按环境调整
- SQLite 数据目录可写
- 部署环境不对公网直接暴露管理接口

建议重点配置：

- `SEAHORSE_DB_PATH`
- SQLite 文件所在目录
- embedding 配置：当前默认使用 stub provider
- server 监听地址：仅绑定本地或内网地址
- `./config/seahorse.toml` 的 `[observability]` 段
- `observability.enable_metrics` 与 `observability.metrics_path` 是否与 Prometheus 抓取配置一致

## 3. 首次部署

建议流程：

1. 准备工作目录，例如 `./data/`
2. 设置数据库路径，例如 `./data/seahorse.db`
3. 准备 `./config/seahorse.toml`（如不提供则使用默认：开启 metrics，路径 `/metrics`）
4. 启动服务，让程序自动执行 SQLite migration
5. 调用 `GET /health`，确认服务可启动
6. 调用 `GET /stats`，确认基础统计可读
7. 若开启 metrics，调用 `GET /metrics`，确认返回 `200` 且 `Content-Type: text/plain; version=0.0.4`
8. 手工执行一次最小链路：
   - `POST /ingest`
   - `POST /recall`
   - `POST /forget`
   - `POST /admin/rebuild`
   - `GET /admin/jobs/{job_id}`

首次部署后建议立即保留一份空库或基线库备份。

## 4. 日常健康检查

最小检查项：

- `GET /health`
  - `status = ok` 表示当前索引状态正常
  - `status = degraded` 常见于 rebuild 期间或索引存在待修复项
  - `status = failed` 表示当前实例不可作为正常服务实例使用
- `GET /stats`
  - `chunk_count`
  - `deleted_chunk_count`
  - `repair_queue_size`
  - `index_status`
- `GET /metrics`（或配置指定路径）
  - `seahorse_http_requests_total{scope="total"}`：总请求数
  - `seahorse_http_request_errors_total{scope="total"}`：错误请求数（HTTP >= 400）
  - `seahorse_http_request_latency_ms_max{scope="total"}`：最大请求延迟（毫秒）
  - `seahorse_index_state{state="..."}`：索引状态 one-hot 指标
  - `seahorse_health_status{status="..."}`：健康状态 one-hot 指标

推荐人工判断规则：

- `repair_queue_size` 持续增长：视为需要人工介入
- `index_status = rebuilding` 长时间不结束：优先检查 `maintenance_jobs`
- `index_status = degraded` 且未在维护窗口：视为异常
- `request_errors_total / requests_total` 持续上升：优先检查近 5 分钟错误请求与慢请求

建议最低告警规则（MVP）：

1. 可用性告警：`seahorse_health_status{status="failed"} == 1` 持续 1 分钟。
2. 索引降级告警：`seahorse_index_state{state="degraded"} == 1` 持续 10 分钟（排除计划内维护窗口）。
3. 错误率告警：`rate(seahorse_http_request_errors_total{scope="total"}[5m]) / clamp_min(rate(seahorse_http_requests_total{scope="total"}[5m]), 1) > 0.05` 持续 10 分钟。
4. 延迟突增告警：`seahorse_http_request_latency_ms_max{scope="total"} > 1000` 持续 10 分钟。

## 5. SQLite 备份

当前 MVP 的核心资产是 SQLite 文件。最小备份策略：

1. 停止写流量
2. 确认没有主动 rebuild 作业正在运行
3. 复制数据库文件到备份目录
4. 记录备份时间、文件大小、对应版本

建议至少保留：

- 最近一次部署前备份
- 最近一次部署后基线备份
- 最近一次 rebuild 前备份

注意：

- 如果在高写入期间直接复制 SQLite 文件，可能得到不一致快照
- MVP 阶段优先采用“停写后复制文件”的保守方式

## 6. 回滚

适用场景：

- 新版本启动后 `health` 异常
- 主链路 `ingest` / `recall` 出现明显回归
- rebuild 作业异常导致服务持续不可用

最小回滚步骤：

1. 停止当前服务实例
2. 保留当前故障库文件，重命名为诊断副本
3. 恢复最近一次可用备份的 SQLite 文件
4. 使用上一个稳定版本重新启动
5. 依次检查：
   - `GET /health`
   - `GET /stats`
   - 最小 `ingest -> recall` 链路

如果回滚原因与新 schema 或 rebuild 行为有关，必须同时保留：

- 故障时数据库文件
- 对应版本号
- 手工触发过的 rebuild job 记录

## 7. Rebuild 操作

适用场景：

- 索引状态异常
- 索引被清空或损坏
- embedding 配置发生变更

操作流程：

1. 调用 `POST /admin/rebuild`
2. 记录返回的 `job_id`
3. 轮询 `GET /admin/jobs/{job_id}`
4. 等待状态进入终态：
   - `succeeded`
   - `failed`
   - `cancelled`

处理规则：

- 同一 namespace 同时只应保留一个有效 rebuild
- 如需替换当前 rebuild，请显式使用 `force=true`
- 服务重启后会尝试恢复活跃 rebuild job；当前实现会保留最新 active job，并取消较旧 active job

## 8. 故障恢复

### 8.1 `health = degraded`

优先检查：

- 是否正处于 rebuild 中
- `repair_queue_size` 是否堆积
- 是否存在 `maintenance_jobs.status in ('queued', 'running')`

建议动作：

- 若为计划内 rebuild，持续观察
- 若为非计划内降级，优先触发 rebuild 并观察 job 终态

### 8.2 rebuild 失败

建议动作：

1. 获取失败 job 的 `error_message`
2. 检查 SQLite 文件是否可读写
3. 检查配置中的 embedding 维度与模型信息
4. 必要时回滚到最近可用数据库备份

### 8.3 repair 队列堆积

当前仅有最小 repair worker 框架，未形成完整自动修复闭环。

因此当前阶段建议：

- 将 repair backlog 视为人工处理信号
- 先使用 rebuild 恢复服务
- 不要把 repair queue 当成已经稳定可依赖的自愈系统

## 9. 当前仍缺证据 / 缺验证

当前运行手册必须明确以下 release blocker：

- 无正式发布版编译验证记录
- 无自动化 contract / E2E / 故障注入发布验收记录
- 无 `1 万 chunk` 基线性能验收结果
- 告警规则仅有 MVP 建议阈值，尚未在统一监控平台完成落地验证
- 无完整 repair worker 调度与生产级恢复闭环验证

因此当前更准确的定位是：

- 可作为开发闭环版 MVP
- 尚未达到严格意义上的可发布版 MVP
