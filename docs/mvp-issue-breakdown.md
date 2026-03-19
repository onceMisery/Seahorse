# Seahorse MVP 执行拆解清单

> 对应设计基线：`docs/mvp-design-and-roadmap.md`
>
> 用途：把 MVP 设计稿继续拆到工程排期层，供 issue、milestone、迭代计划和看板直接采用。

## 1. 使用方式

- 本文档按 `Milestone -> Epic -> Issue` 三层拆分。
- `Issue` 粒度以 `1 ~ 3` 个工程日为宜；超出该范围应继续拆分。
- 所有 issue 默认以单仓库、本地或受信内网部署为前提。
- 只有进入 `Release Gate` 的 issue 才属于 MVP 必须项；标记为 `Optional` 的项不得阻塞发版。

## 2. 里程碑总览

| 里程碑            | 目标                                | 退出条件                    |
|----------------|-----------------------------------|-------------------------|
| `M1` 最小写入与召回闭环 | 跑通 `ingest -> recall` 主链路         | 文本可写入、可向量召回、主链路测试通过     |
| `M2` 生命周期与恢复   | 跑通 `forget -> rebuild -> recover` | soft delete 生效，索引故障后可恢复 |
| `M3` Tag 与工程化  | 补齐标签、观测、运维最小面                     | 标签可解释、日志/指标/统计可用        |
| `M4` API 与发版收口 | 补齐外部接口和发布门槛                       | API 契约稳定，E2E 通过，可部署可回滚  |

## 3. 排期假设

- 建议至少有 `2` 条并行泳道：
- 泳道 A：核心存储 / 索引 / 作业
- 泳道 B：API / 测试 / 运维
- `M1` 完成前，不进入任何 Tide、LIF、connectome、MCP、SDK 相关开发。
- `M2` 的 `rebuild` 和 `repair` 是 release blocker，不能后置到 `M3`。

## 4. Milestone 1：最小写入与召回闭环

### Epic M1-E1：Schema 与 Repository 基线

| Issue      | 说明                                                                                                             | 依赖         | 输出物           | 完成定义                                    |
|------------|----------------------------------------------------------------------------------------------------------------|------------|---------------|-----------------------------------------|
| `M1-E1-I1` | 建立 SQLite 初始 migration，创建 `files`、`chunks`、`tags`、`chunk_tags`、`repair_queue`、`maintenance_jobs`、`schema_meta` | 无          | migration 脚本  | 本地可初始化空库，重复执行幂等                         |
| `M1-E1-I2` | 落实必要索引与唯一约束                                                                                                    | `M1-E1-I1` | schema 更新     | 唯一约束和索引与设计稿一致，启动校验通过                    |
| `M1-E1-I3` | 实现 repository 层和事务边界                                                                                           | `M1-E1-I1` | repository 模块 | 能在单事务内提交 `files/chunks/tags/chunk_tags` |
| `M1-E1-I4` | 实现 `schema_meta` 版本检查与启动前校验                                                                                    | `M1-E1-I1` | 启动校验逻辑        | schema 版本不匹配时拒绝启动并输出可诊断错误               |

### Epic M1-E2：Chunk 与 Embedding 基线

| Issue      | 说明                             | 依赖                     | 输出物               | 完成定义                                                             |
|------------|--------------------------------|------------------------|-------------------|------------------------------------------------------------------|
| `M1-E2-I1` | 实现基础 chunker，支持固定规则切分          | 无                      | chunker 模块        | 相同输入产生稳定 chunk 序列                                                |
| `M1-E2-I2` | 定义 `EmbeddingProvider` 抽象及错误模型 | 无                      | trait / interface | 支持 `embed`、`embed_batch`、`model_id`、`dimension`、`max_batch_size` |
| `M1-E2-I3` | 接入首个 embedding provider 实现     | `M1-E2-I2`             | provider adapter  | provider 超时、失败、维度不匹配可区分返回                                        |
| `M1-E2-I4` | 落实 ingest 输入校验与内容预处理           | `M1-E2-I1`, `M1-E2-I2` | service 逻辑        | 可拒绝非法输入、超长内容和非法 metadata                                         |

### Epic M1-E3：Ingest 与 Basic Recall

| Issue      | 说明                                                      | 依赖                                 | 输出物                  | 完成定义                                             |
|------------|---------------------------------------------------------|------------------------------------|----------------------|--------------------------------------------------|
| `M1-E3-I1` | 实现 `dedup_mode` 与 `file_hash` 幂等策略                      | `M1-E1-I3`, `M1-E2-I4`             | ingest service       | `reject/upsert/allow` 三种行为可测                     |
| `M1-E3-I2` | 实现向量索引 adapter，支持插入、查询、可见性控制                            | `M1-E2-I3`                         | vector index adapter | 能插入 chunk 向量并返回 Top-K                            |
| `M1-E3-I3` | 实现 ingest 主链路：预处理、切分、embedding、事务写入、索引更新                | `M1-E1-I3`, `M1-E3-I1`, `M1-E3-I2` | ingest pipeline      | 成功写入时 `files/chunks` 状态正确，索引失败时进入 `repair_queue` |
| `M1-E3-I4` | 实现 basic recall：query embedding、vector top-k、回表组装、去重与过滤 | `M1-E3-I2`                         | recall service       | 可基于 chunk 文本返回稳定结果结构                             |
| `M1-E3-I5` | 建立 M1 集成测试：`ingest -> recall`                           | `M1-E3-I3`, `M1-E3-I4`             | integration tests    | 主链路自动化通过，结果包含正确 chunk                            |

### M1 Release Gate

- 可创建空库并正常启动。
- 文本写入后能在 recall 中返回。
- 索引失败不会破坏 SQLite 已提交数据。
- `ingest -> recall` 自动化测试通过。

## 5. Milestone 2：生命周期与恢复

### Epic M2-E1：Forget 与可见性控制

| Issue      | 说明                        | 依赖                     | 输出物            | 完成定义                                |
|------------|---------------------------|------------------------|----------------|-------------------------------------|
| `M2-E1-I1` | 实现 soft delete 数据模型更新     | `M1-E1-I3`             | forget service | `chunks.is_deleted/deleted_at` 正确落库 |
| `M2-E1-I2` | 将 recall 结果强制过滤 tombstone | `M1-E3-I4`, `M2-E1-I1` | recall update  | 已删除 chunk 不再出现在结果中                  |
| `M2-E1-I3` | 向量索引可见性更新或延迟清理标记          | `M1-E3-I2`, `M2-E1-I1` | index update   | 删除后不依赖全量 rebuild 也不会继续召回            |

### Epic M2-E2：Repair 与 Rebuild 作业

| Issue      | 说明                                                      | 依赖                                 | 输出物              | 完成定义                                              |
|------------|---------------------------------------------------------|------------------------------------|------------------|---------------------------------------------------|
| `M2-E2-I1` | 实现 `repair_queue` 状态流转与重试机制                             | `M1-E1-I3`, `M1-E3-I3`             | repair worker    | `pending/running/succeeded/failed/deadletter` 可观测 |
| `M2-E2-I2` | 实现 `maintenance_jobs` 和 rebuild job manager             | `M1-E1-I3`                         | job manager      | 异步 rebuild 可创建、查询、恢复状态                            |
| `M2-E2-I3` | 实现 rebuild pipeline：扫描有效 chunk、重算 embedding、重建索引、切换可用索引 | `M2-E2-I2`, `M1-E3-I2`, `M1-E2-I3` | rebuild pipeline | 索引损坏后可从 SQLite 恢复                                 |
| `M2-E2-I4` | 实现 rebuild 并发保护与 `force` 策略                             | `M2-E2-I2`                         | guard logic      | 同一 namespace 只有一个激活 rebuild                       |

### Epic M2-E3：健康状态与故障恢复

| Issue      | 说明                                                                           | 依赖                                 | 输出物              | 完成定义                     |
|------------|------------------------------------------------------------------------------|------------------------------------|------------------|--------------------------|
| `M2-E3-I1` | 实现 `files.ingest_status`、`chunks.index_status`、`schema_meta.index_state` 状态机 | `M1-E3-I3`, `M2-E2-I3`             | state management | 状态切换符合设计稿，失败不伪装成功        |
| `M2-E3-I2` | 实现健康检查基础逻辑：DB、索引、embedding provider                                          | `M2-E2-I3`                         | health service   | 能区分 `ok/degraded/failed` |
| `M2-E3-I3` | 建立故障注入测试：索引损坏、索引更新失败、provider 超时                                             | `M2-E2-I1`, `M2-E2-I3`, `M2-E3-I2` | failure tests    | 故障后能观测、能恢复、主路径行为符合预期     |

### M2 Release Gate

- soft delete 后对应内容不会被继续召回。
- rebuild 作业可查询进度和结果。
- 索引损坏后可完成 rebuild 并恢复 recall。
- 故障注入测试覆盖 repair / rebuild 关键路径。

## 6. Milestone 3：Tag 与基础工程化

### Epic M3-E1：Tag 基线

| Issue      | 说明                                  | 依赖                     | 输出物              | 完成定义                    |
|------------|-------------------------------------|------------------------|------------------|-------------------------|
| `M3-E1-I1` | 实现显式 tags 写入和 `normalized_name` 规范化 | `M1-E1-I3`, `M1-E3-I3` | tag service      | trim、lowercase、dedup 生效 |
| `M3-E1-I2` | 实现可插拔规则式 tag extraction             | `M1-E2-I4`, `M3-E1-I1` | extractor module | 自动 tag 可开关，不阻塞主链路       |
| `M3-E1-I3` | 实现 recall 结果的 tag 过滤与展示组装           | `M1-E3-I4`, `M3-E1-I1` | recall update    | tag 仅做过滤/解释，不参与主召回生成    |

### Epic M3-E2：Observability 与 Stats

| Issue      | 说明                                                             | 依赖                     | 输出物                     | 完成定义                                 |
|------------|----------------------------------------------------------------|------------------------|-------------------------|--------------------------------------|
| `M3-E2-I1` | 接入结构化日志和 `request_id`                                          | `M1-E3-I3`, `M1-E3-I4` | logging                 | 每次请求可串联主链路日志                         |
| `M3-E2-I2` | 实现指标采集：ingest/recall/rebuild latency、error count、queue backlog | `M2-E2-I1`, `M2-E2-I3` | metrics                 | 指标可被导出并用于告警                          |
| `M3-E2-I3` | 实现 `GET /stats` 聚合逻辑                                           | `M1-E1-I3`, `M3-E2-I2` | stats service           | 返回 chunk/tag/deleted/repair/index 状态 |
| `M3-E2-I4` | Optional：实现简化版 `retrieval_log`                                 | `M1-E3-I4`             | optional table + writer | 可记录查询快照，不影响主路径                       |

### M3 Release Gate

- 标签写入、规范化、过滤能力稳定。
- `health`、`stats`、日志、指标可用于基本运维。
- 观测数据能定位 ingest / recall / rebuild 的失败点。

## 7. Milestone 4：API 与发版收口

### Epic M4-E1：统一 API 契约

| Issue      | 说明                                                    | 依赖                                             | 输出物                | 完成定义             |
|------------|-------------------------------------------------------|------------------------------------------------|--------------------|------------------|
| `M4-E1-I1` | 落实统一响应包络和错误码到 HTTP 映射                                 | `M1-E3-I3`, `M1-E3-I4`, `M2-E3-I2`             | API response layer | 所有接口响应结构一致       |
| `M4-E1-I2` | 实现 `POST /ingest`、`POST /recall`、`POST /forget`       | `M4-E1-I1`, `M1-E3-I3`, `M1-E3-I4`, `M2-E1-I1` | API handlers       | 基础链路均可通过 HTTP 调用 |
| `M4-E1-I3` | 实现 `POST /admin/rebuild` 和 `GET /admin/jobs/{job_id}` | `M4-E1-I1`, `M2-E2-I2`, `M2-E2-I3`             | admin handlers     | 作业可提交、可轮询        |
| `M4-E1-I4` | 实现 `GET /health`、`GET /stats`                         | `M3-E2-I3`, `M2-E3-I2`                         | API handlers       | 运维接口稳定输出         |

### Epic M4-E2：测试、发布与回滚

| Issue      | 说明                          | 依赖                                 | 输出物                | 完成定义                                     |
|------------|-----------------------------|------------------------------------|--------------------|------------------------------------------|
| `M4-E2-I1` | 生成 OpenAPI 或等效 API 契约文档     | `M4-E1-I2`, `M4-E1-I3`, `M4-E1-I4` | API spec           | 请求响应字段冻结，可用于联调                           |
| `M4-E2-I2` | 建立 API 契约测试和 E2E 测试         | `M4-E2-I1`                         | contract/E2E tests | 覆盖主链路与错误路径                               |
| `M4-E2-I3` | 编写部署、备份、回滚、重建运行手册           | `M2-E2-I3`, `M3-E2-I2`             | runbook            | 可按文档完成部署和故障恢复                            |
| `M4-E2-I4` | 完成 `1 万` chunk 基线数据集验证与发布检查 | `M4-E2-I2`, `M4-E2-I3`             | release checklist  | 满足 `recall P95 < 300ms` 且无 P0/P1 数据一致性缺陷 |

### M4 Release Gate

- 所有核心接口具备稳定的请求响应契约。
- `ingest -> recall -> forget -> rebuild -> recall` E2E 通过。
- 发布手册、回滚步骤、最小告警规则齐备。
- 满足 MVP 设计稿中的发布门槛。

## 8. 建议的 issue 标签

| 标签            | 用途                                    |
|---------------|---------------------------------------|
| `mvp-blocker` | 阻塞 MVP 发布                             |
| `schema`      | 数据模型与迁移                               |
| `storage`     | SQLite / repository                   |
| `index`       | 向量索引相关                                |
| `pipeline`    | ingest / recall / forget / rebuild 编排 |
| `api`         | HTTP 接口与契约                            |
| `ops`         | 健康检查、指标、运行手册                          |
| `test`        | 单元、集成、E2E、故障注入                        |
| `optional`    | 可选项，不阻塞发布                             |

## 9. 建议的并行执行顺序

### 第一批

- `M1-E1-I1`
- `M1-E2-I1`
- `M1-E2-I2`

### 第二批

- `M1-E1-I2`
- `M1-E1-I3`
- `M1-E2-I3`
- `M1-E2-I4`

### 第三批

- `M1-E3-I1`
- `M1-E3-I2`
- `M1-E3-I3`
- `M1-E3-I4`

### 第四批

- `M1-E3-I5`
- `M2-E1-I1`
- `M2-E2-I1`
- `M2-E2-I2`

后续里程碑按依赖顺延，不建议把 `M3`、`M4` 提前到 `M2` 之前。

## 10. 不进入 MVP 排期的事项

- Tide / Gram-Schmidt / Weak Signal
- LIF / Spike propagation / connectome
- MCP / Python SDK / Node SDK / WASM
- 多租户完整隔离
- Marketplace / 平台协议标准化
- 以百万级向量指标作为首版发布前置条件
