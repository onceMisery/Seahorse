# Seahorse MVP 最终版设计文档

## 1. 文档定位

本文件是当前 Seahorse MVP 的最终实现态设计文档。

它不是愿景稿，也不是待实现路线图，而是对当前仓库中已经落地的 MVP 架构、接口边界、运行模型、可观测性和发布边界的最终收口说明。

与其它文档的关系：

- `docs/mvp-design-and-roadmap.md`
  - 设计基线与演进路线，保留背景、范围与里程碑语义
- `docs/mvp-openapi.yaml`
  - 当前正式 API 契约
- `docs/mvp-release-checklist.md`
  - 发布前核对项与 blocker 状态
- `docs/runbooks/mvp-deploy-backup-rollback.md`
  - 运维执行手册
- `docs/handoff/2026-04-22-mvp-release-handoff.md`
  - 发布候选交接摘要

## 2. 最终结论

当前 Seahorse MVP 已完成仓库内可交付范围，形成如下闭环：

- `ingest -> recall -> forget -> rebuild`
- SQLite 作为唯一事实源
- in-memory vector index 作为召回加速层
- REST API 作为对外访问面
- repair / rebuild 作为后台恢复机制
- health / ready / live / stats / metrics 作为最小可运维接口
- 结构化日志、Prometheus 指标、自动化测试、发布文档作为工程交付面

当前仍未闭环的事项只剩外部环境依赖：

- 监控平台正式导入并启用告警规则
- 日志采集、落库、检索链路验证留档
- release 机器执行并通过 `10k chunk` hard gate

## 3. 系统边界

### 3.1 纳入 MVP 的能力

- 文本写入
- 基础 chunk 切分
- embedding 生成
- 向量召回
- 显式 tags
- 规则型自动打标
- soft delete
- rebuild / repair
- 后台作业状态查询
- REST API
- 最小可观测性

### 3.2 明确不纳入 MVP 的能力

- Tide、LIF、Gravity、Dream、STDP 等研究型机制
- MCP、SDK、WASM、多语言绑定
- 完整多租户
- 复杂安全治理
- 图召回或 tag 作为主候选集生成路径

## 4. 最终架构

```text
Client
  -> seahorse-server (HTTP handlers / validation / envelope / observability)
    -> seahorse-core pipeline
      -> SQLite repository (source of truth)
      -> Embedding provider
      -> In-memory vector index
      -> Repair / rebuild jobs
```

### 4.1 seahorse-server

职责：

- HTTP 路由
- 输入校验
- 错误码映射
- 统一响应包络
- request_id / request span / request metrics
- probe 与 metrics 输出

### 4.2 seahorse-core

职责：

- ingest / recall / forget / rebuild 主流程
- repository 读写与状态流转
- embedding / index 抽象
- repair worker 与后台恢复

### 4.3 SQLite

定位：

- 唯一事实源
- 保存 files / chunks / tags / chunk_tags / repair_queue / maintenance_jobs / schema_meta

### 4.4 In-memory vector index

定位：

- 查询加速层
- 不是最终真相来源
- 可通过 rebuild 从 SQLite 重新构建

## 5. 最终数据与状态模型

### 5.1 关键状态

- `files.ingest_status`
  - `pending_index`
  - `ready`
  - `partial`
  - `deleted`
- `chunks.index_status`
  - `pending`
  - `ready`
  - `failed`
  - `deleted`
- `schema_meta.index_state`
  - `ready`
  - `rebuilding`
  - `degraded`
  - `unavailable`
- `maintenance_jobs.status`
  - `queued`
  - `running`
  - `succeeded`
  - `failed`
  - `cancelled`
- `repair_queue.status`
  - `pending`
  - `running`
  - `succeeded`
  - `failed`
  - `deadletter`

### 5.2 状态原则

- SQLite 写入先成功，再尝试索引更新
- 索引失败时不得伪装成成功，必须进入 `partial` / `failed` 并写入 repair queue
- rebuild 是异步维护操作，不阻塞主 API 契约
- recall 的 `degraded` 语义由运行时 `index_state` 推导

## 6. 最终主链路设计

### 6.1 Ingest

流程：

1. 校验输入
2. 文本预处理
3. 固定模式 chunk 切分
4. embedding 生成
5. 显式 tags + 规则型 auto_tag
6. SQLite 事务写入
7. 尝试更新向量索引
8. 若失败则写入 repair queue，并返回部分成功语义

当前实现边界：

- `options.chunk_mode` 仅支持 `fixed`
- `options.auto_tag` 默认 `false`
- `options.dedup_mode` 支持 `reject / upsert / allow`

### 6.2 Recall

流程：

1. query embedding
2. vector top-k
3. 回表补全 chunk / source / tags / metadata
4. 文件和 tags 过滤
5. 去重和结果封装

当前实现边界：

- `mode` 仅支持 `basic`
- `timeout_ms` 已实现，命中返回 `504 TIMEOUT`

### 6.3 Forget

流程：

1. 按 `chunk_ids` 或 `file_id` 进行 soft delete
2. 更新可见性
3. 尝试进行索引清理
4. 若失败则进入 repair

当前实现边界：

- 正式契约仅支持 `mode=soft`

### 6.4 Rebuild / Repair

- `POST /admin/rebuild` 创建持久化 job
- `GET /admin/jobs/{job_id}` 查询状态
- 服务重启后会恢复 active rebuild job
- repair worker 会重试失败索引任务，超过阈值进入 `deadletter`

## 7. 最终对外接口面

### 7.1 业务接口

- `POST /ingest`
- `POST /recall`
- `POST /forget`
- `POST /admin/rebuild`
- `GET /admin/jobs/{job_id}`

### 7.2 巡检 / 运维接口

- `GET /stats`
- `GET /health`
- `GET /ready`
- `GET /live`
- `GET /metrics`

### 7.3 统一响应包络

```json
{
  "success": true,
  "data": {},
  "error": null,
  "request_id": "req-..."
}
```

### 7.4 最终错误码集合

- `INVALID_INPUT`
- `EMBEDDING_FAILED`
- `STORAGE_ERROR`
- `INDEX_UNAVAILABLE`
- `REBUILD_FAILED`
- `TIMEOUT`

## 8. 最终可观测性设计

### 8.1 结构化日志

当前已落地：

- request_id
- request.start / request.end
- method / route / path / status / latency_ms
- ingest / recall / forget / rebuild / jobs 关键事件
- repair / rebuild 后台事件

### 8.2 Probe 语义

- `/live`
  - 只表示进程存活
- `/ready`
  - 用于平台 readiness
  - 内部不可服务时返回 `503`
- `/health`
  - 用于人工巡检
  - 即使内部状态失败也保留 `200`，状态在 `data.status` 中表达

### 8.3 Metrics

当前已落地：

- HTTP 请求总量
- HTTP 错误总量
- HTTP 最大延迟
- chunk / tag / deleted_chunk 计数
- repair queue backlog
- repair queue 按状态分布
- rebuild jobs 按状态分布
- oldest repair task age
- oldest active rebuild job age
- index_state
- health_status

## 9. 最终测试与交付状态

当前仓库内已具备：

- core 单测
- server 单测
- API contract 测试
- lifecycle E2E
- fault recovery
- perf gate smoke
- docs 一致性校验

发布前仍需外部执行：

- release 机器上的 `10k chunk` hard gate
- 日志平台链路验证
- 监控平台告警导入与验证

## 10. 发布边界

### 10.1 现在可以宣称的状态

- “发布候选 MVP”
- “仓库内功能与文档已收口”
- “具备最小运行、恢复、观测和测试闭环”

### 10.2 现在还不能宣称的状态

- “最终可交付的发布型 MVP 已全部闭环”

原因不是仓库内功能未完工，而是以下三项仍依赖外部环境执行：

1. 告警规则平台化落地
2. 日志链路验证留档
3. release 机器 perf gate 过线

## 11. 建议的最终执行入口

优先按以下顺序使用文档和脚本：

1. `docs/mvp-final-design.md`
2. `docs/mvp-openapi.yaml`
3. `docs/runbooks/mvp-release-execution-checklist.md`
4. `scripts/run-mvp-release-validation.ps1`
5. `docs/runbooks/mvp-logging-validation-record-template.md`
6. `docs/handoff/2026-04-22-mvp-release-handoff.md`
