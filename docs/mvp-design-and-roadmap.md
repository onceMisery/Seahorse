# Seahorse MVP 技术设计与演进路线

> 文档文件名：`docs/mvp-design-and-roadmap.md`
>
> 文档定位：面向工程团队的执行型设计文档，优先回答首版做什么、为什么这样做、怎么实现、怎么验收，以及后续如何演进。

## 1. 文档摘要

### 1.1 结论
Seahorse 首版应聚焦一个最小且完整的记忆引擎闭环：`ingest -> recall -> forget -> rebuild`。首版目标不是实现完整“仿脑认知系统”，而是交付一个可写入、可召回、可删除、可重建、可观测、可测试的工程化基础版本。

### 1.2 首版定位
Seahorse MVP 是一个以 SQLite 为事实源、以向量检索为主召回路径、以 Tag 提取与规范化作为辅助语义层、通过基础 REST API 暴露能力的本地/服务化记忆引擎。

### 1.3 本文档回答的问题
本文档重点回答以下问题：
- 首版做什么，不做什么
- 为什么首版要这样收敛范围
- MVP 的系统边界、主链路和关键技术选择
- 数据模型、接口契约、错误处理、测试与验收标准
- 哪些高级机制进入后续迭代或实验阶段

---

## 2. 背景与目标

### 2.1 背景
当前仓库已有一份战略级设计稿《ZH-设计文档V2.md》，其中包含大量长期愿景、仿脑叙事、认知增强算法、平台化接口和生态扩展设想。这些内容对项目方向有价值，但不适合作为首版工程实施依据。

首版文档必须从“战略级 + 全量技术设想”切换为“执行优先 + 可验证交付”。工程团队首先需要的是一份可直接拆分任务、定义边界、指导实现与验收的设计文档。

### 2.2 核心问题定义
Seahorse 首版要解决的问题不是“如何完整模拟类人记忆”，而是：

1. 如何稳定接收记忆内容并写入系统。
2. 如何基于统一的数据模型进行可靠召回。
3. 如何删除或忘记已有内容而不破坏系统一致性。
4. 如何在索引损坏、模型升级或数据变更后完成重建。
5. 如何以最低可上线标准提供 API、观测、错误处理与测试能力。

### 2.3 首版目标
首版目标定义如下：
- 交付一个可运行的记忆引擎最小闭环。
- 以 SQLite 作为 source of truth。
- 以向量检索作为唯一主召回路径。
- 保留 Tag 提取、规范化、过滤和解释能力。
- 提供最小 REST API 集合。
- 具备基础可观测性、可恢复性与自动化测试。

### 2.4 非目标
以下内容明确不属于首版交付目标：
- 不以“全球首个”“事实标准”类战略叙事作为实现目标。
- 不把 Tide、LIF、Gravity、Dream、STDP 等高级机制作为首版主路径依赖。
- 不在首版实现多语言绑定、WASM、Marketplace、MCP 平台协议层。
- 不在首版实现完整多租户、复杂安全治理或研究型评测体系。
- 不把百万级极限性能作为首版 release gate。

---

## 3. 愿景与设计原则

### 3.1 愿景
Seahorse 的长期愿景是成为适合 AI Agent 和长期知识场景的记忆引擎：不仅支持基础检索，还能逐步演进出更强的关联、解释、重建和增强能力。

但在首版阶段，愿景必须服从交付。愿景用于指导架构预留，不用于扩大当前范围。

### 3.2 设计原则

#### 原则一：先闭环，再增强
首版先完成 ingest、recall、forget、rebuild 的稳定闭环，再验证高级召回增强算法。

#### 原则二：SQLite 是事实源
所有结构化记忆数据以 SQLite 为唯一事实源；向量索引是加速层，不是最终真相来源。

#### 原则三：主路径必须可降级
任何后续增强能力都必须建立在 Basic Recall 主路径可用的前提下，并支持关闭或降级。

#### 原则四：Tag 是辅助语义层
首版中 Tag 用于显式标签承载、基础规范化、过滤、解释和结果展示，不作为复杂推理主引擎。自动提取能力建议纳入首版，但不应阻塞最小闭环。

#### 原则五：先职责表达，后叙事表达
工程设计优先采用职责导向命名，例如“向量索引层”“查询编排层”“后台维护任务”；“Hippocampus”“Thalamus”等仿脑命名仅作为愿景叙事别名，不主导代码与接口设计。

#### 原则六：先验收，再扩展
没有明确测试路径、错误边界和验收标准的能力，不进入 MVP 必须项。

---

## 4. MVP 范围定义

### 4.1 MVP 必须项
下列能力属于首版必须交付项：

| 能力 | 级别 | 说明 |
|---|---|---|
| 文本 ingest | MVP 必须项 | 记忆写入主入口 |
| 基础 chunk 切分 | MVP 必须项 | 构成最小检索单元 |
| embedding 生成与维度校验 | MVP 必须项 | 向量召回的基础 |
| SQLite 主存储 | MVP 必须项 | source of truth |
| chunks / tags / chunk_tags 模型 | MVP 必须项 | 支撑数据关联与标签能力 |
| vector recall | MVP 必须项 | 首版唯一候选集生成路径 |
| 显式 tags 支持 | MVP 必须项 | 支持写入时传入标签 |
| tag normalization | MVP 必须项 | 保证标签一致性 |
| forget（soft delete） | MVP 必须项 | 生命周期闭环的一部分 |
| rebuild / repair | MVP 必须项 | 可恢复性保障 |
| 基础 REST API | MVP 必须项 | 首版对外接口 |
| observability | MVP 必须项 | logs / metrics / health / stats |
| 错误码与降级策略 | MVP 必须项 | 工程可用性要求 |
| 测试矩阵与验收标准 | MVP 必须项 | release gate |

### 4.2 MVP 可选项
以下能力可在首版酌情加入，但不能阻塞主线：

| 能力 | 级别 | 说明 |
|---|---|---|
| 自动 tag extraction | MVP 可选项 | 规则提取或可插拔自动提取，不阻塞最小闭环 |
| embedding cache | MVP 可选项 | 用于降低重复 embedding 成本 |
| retrieval log 简化版 | MVP 可选项 | 便于调试与运维 |
| tag-based filter recall | MVP 可选项 | 仅作为召回后过滤，不独立生成候选集 |
| hard delete 管理接口 | MVP 可选项 | 可作为管理能力后置 |
| compact 管理命令 | MVP 可选项 | 用于处理 tombstone 累积 |
| namespace 预留字段 | MVP 可选项 | 为未来多租户做兼容设计 |

### 4.3 后续迭代项
以下能力属于明确的后续迭代项，不进入首版主路径：

| 能力 | 级别 | 说明 |
|---|---|---|
| tag centroid | 后续迭代项 | 当 tag 参与更强召回增强时引入 |
| connectome 共现图 | 后续迭代项 | 用于关联增强，不阻塞首版 |
| gravity rerank | 后续迭代项 | 属于召回质量增强 |
| 弱信号召回 | 后续迭代项 | 需有离线验证后再进入实现 |
| 多租户完整隔离 | 后续迭代项 | 首版不承担该复杂度 |
| API 鉴权和安全增强 | 后续迭代项 | 视部署模式逐步完善 |
| MCP / SDK 集成 | 后续迭代项 | 等核心引擎稳定后推进 |

### 4.4 实验性方向
以下能力属于研究或实验方向，仅保留为长期探索：

| 能力 | 级别 | 说明 |
|---|---|---|
| LIF 脉冲扩散 | 实验性方向 | 参数复杂、验证成本高 |
| SpikeAssociation recall | 实验性方向 | 不进入首版主路径 |
| Tide / Gram-Schmidt / 投影熵 | 实验性方向 | 需独立验证收益 |
| world-view gate | 实验性方向 | 不作为首版路由前提 |
| Gravity Field | 实验性方向 | 暂不进入主链路 |
| Dream Mode | 实验性方向 | 离线联想整合，边界较大 |
| STDP / 自适应可塑性 | 实验性方向 | 研究性质强 |
| WASM / 多语言绑定 | 实验性方向 | 非首版必需 |

### 4.5 暂不考虑
以下方向不进入本轮设计范围：
- Marketplace
- 平台协议标准化
- 全量生态集成承诺
- 拓扑可视化和实时动画系统
- 以超大规模性能目标替代首版工程验收

---

## 5. MVP 方案总览

### 5.1 方案结论
Seahorse MVP 采用“SQLite 主存储 + 向量索引主召回 + Tag 辅助语义层 + REST API + 后台修复/重建任务”的最小架构方案。

### 5.2 主链路概览

#### 写入主链路
`input -> preprocess -> chunk -> embed -> extract tags -> normalize tags -> SQLite transaction -> vector index update -> audit/log`

#### 召回主链路
`query -> embed -> vector top-k -> load chunk metadata -> dedup -> filters -> response`

#### 删除主链路
`delete request -> mark tombstone in SQLite -> update visibility -> lazy index cleanup -> optional rebuild`

#### 重建主链路
`rebuild request / repair trigger -> scan valid chunks from SQLite -> regenerate/load embeddings -> rebuild vector index -> switch index state`

### 5.3 为什么采用该方案
- 范围收敛，便于首版落地。
- SQLite 便于保证事务、一致性、迁移和恢复。
- 向量检索可以快速形成可用召回路径。
- Tag 层保留了后续增强空间，但不会把首版拉入复杂认知算法。
- REST API 足以支撑首版集成，不需要一开始投入多协议与多语言绑定。

### 5.4 首版容量与性能假设
为保证首版可验收，建议定义最小非功能基线：
- 数据规模假设：先支持 `1 万 ~ 10 万` chunk 级别数据集。
- recall 性能目标：`P95 < 300ms`，单请求默认 `top_k <= 20`。
- ingest 请求大小：单次文本内容建议不超过 `1 MB` 原始文本。
- tags 限制：单次写入标签数量建议不超过 `32`，单个标签长度建议不超过 `64` 字符。
- rebuild 目标：在上述规模下，可在可接受维护窗口内完成重建，并具备进度与状态输出。

这些数字不是长期性能承诺，而是首版开发、测试和上线验收的最低基线。

---

## 6. MVP 架构设计

### 6.1 最小架构

```text
┌─────────────────────────────────────────────┐
│                 Client / Caller             │
└──────────────────────┬──────────────────────┘
                       │ REST API
┌──────────────────────▼──────────────────────┐
│             Application Service Layer       │
│  ingest / recall / forget / rebuild / stats │
└───────────────┬─────────────────────┬───────┘
                │                     │
                │                     │
┌───────────────▼──────────────┐  ┌──▼──────────────────┐
│        SQLite Storage         │  │   Vector Index      │
│ files / chunks / tags / ...   │  │ main recall path    │
└───────────────┬──────────────┘  └──┬──────────────────┘
                │                    │
                └─────────┬──────────┘
                          │
                 ┌────────▼─────────┐
                 │ Maintenance Jobs │
                 │ repair / rebuild │
                 └──────────────────┘
```

### 6.2 组件职责

#### Application Service Layer
负责接收 API 请求、执行参数校验、调用各模块完成编排，并统一返回响应与错误结构。

#### SQLite Storage
负责持久化源数据、元数据、标签关系、删除状态、版本信息与修复任务信息。

#### Vector Index
负责 embedding 的插入、查询、删除标记和重建后的索引切换。该组件仅是加速层。

#### Maintenance Jobs
负责 repair、rebuild、compact 等后台维护任务。后台任务不得阻塞 recall 主路径。

### 6.3 架构边界
- 首版不单独拆出 Tide、Synapse、Dream 等复杂子系统。
- 未来增强能力只能作为 recall pipeline 的可选扩展阶段引入。
- 即使后续加入图扩展或弱信号召回，也必须保证 Basic Recall 仍然独立可运行。

---

## 7. MVP 技术设计

### 7.1 数据模型设计

#### 7.1.1 首版必需表
建议首版优先实现如下 SQLite 表：

- `files`
- `chunks`
- `tags`
- `chunk_tags`
- `repair_queue`
- `schema_meta`

可选表：
- `embedding_cache`
- `retrieval_log`

首版不强制实现：
- `connectome`
- `neuron_states`
- 完整 `namespaces` 体系

#### 7.1.2 建议��段

**files**
- `id`
- `filename`
- `source_type`
- `file_hash`
- `created_at`
- `updated_at`

**chunks**
- `id`
- `file_id`
- `chunk_index`
- `chunk_text`
- `content_hash`
- `embedding_id`
- `token_count`
- `model_id`
- `dimension`
- `is_deleted`
- `deleted_at`
- `created_at`
- `updated_at`

**tags**
- `id`
- `name`
- `normalized_name`
- `category`
- `usage_count`
- `created_at`
- `updated_at`

**chunk_tags**
- `chunk_id`
- `tag_id`
- `confidence`
- `source`

**repair_queue**
- `id`
- `task_type`
- `payload`
- `status`
- `retry_count`
- `last_error`
- `created_at`
- `updated_at`

**schema_meta**
- `key`
- `value`
- `updated_at`

### 7.2 ingest 设计

#### 7.2.1 输入边界
首版 ingest 输入建议至少包含：
- `content`
- `source`（可选）
- `tags`（可选显式标签）
- `metadata`（可选）
- `options`（chunk 模式、是否自动提取标签等）

#### 7.2.1.a 输入约束
建议为首版接口补充以下输入边界：

| 字段 | 约束 | 非法处理 |
|---|---|---|
| `content` | 必填，UTF-8 文本，长度 `1..1MB` | 返回 `INVALID_INPUT` |
| `tags` | 可选，最多 `32` 个 | 超限返回 `INVALID_INPUT` |
| 单个 `tag` | 长度 `1..64` 字符 | 超限返回 `INVALID_INPUT` |
| `metadata` | 可选，建议限制为平铺 JSON 对象 | 非法结构返回 `INVALID_INPUT` |
| `top_k` | 默认 `10`，最大 `20` | 超限截断或返回 `INVALID_INPUT` |

首版应明确拒绝无法解析的非法文本输入、超长字段和不受控 metadata 结构，避免存储污染和接口歧义。

#### 7.2.2 处理流程
1. 输入校验。
2. 文本预处理：规范换行、去除无意义控制字符、生成内容 hash。
3. chunk 切分。
4. embedding 生成。
5. tag 提取：显式标签优先，规则提取补充。
6. tag 规范化：trim、lowercase、alias、stopword、dedup。
7. SQLite 事务写入：`files / chunks / tags / chunk_tags`。
8. 向量索引更新。
9. 若索引更新失败，写入 `repair_queue`。
10. 记录日志与观测信息。

#### 7.2.3 一致性策略
- `files / chunks / tags / chunk_tags` 必须在同一 SQLite 事务中提交。
- 向量索引更新可在事务提交后执行。
- 若向量索引失败，不回滚已提交数据，但必须把该状态标记为可修复。

### 7.3 recall 设计

#### 7.3.1 首版召回模式
首版只实现 `basic` 模式：
- query embedding
- vector top-k
- 回表补充文本、来源、标签、元数据
- 去重与过滤
- 响应组装

其中，向量召回是首版唯一候选集生成路径；Tag 过滤、去重和后续可能加入的 rerank 仅属于召回后处理，不独立生成候选集。

#### 7.3.2 Tag 的首版角色
Tag 在 MVP 中承担以下职责：
- 结果解释
- 基础过滤
- 展示辅助
- 后续召回增强的扩展入口

Tag 在首版中不承担：
- 实时拓扑扩散
- 主排序信号
- 动态神经状态传播

#### 7.3.3 返回结果对象
建议首版结果对象至少包含：
- `chunk_id`
- `chunk_text`
- `source_file`
- `tags`
- `score`
- `source_type`
- `metadata`

其中 `source_type` 在首版固定为 `Vector`，为后续扩展预留枚举空间。

### 7.4 forget 设计

#### 7.4.1 删除策略
首版建议采用 `soft delete` 为主：
- 业务删除通过 `is_deleted = true` + `deleted_at` 表达。
- recall 阶段必须过滤掉已删除 chunk。
- 向量索引的物理清理可延迟到 compact / rebuild。

#### 7.4.2 删除级联行为
- 标记 chunk 删除。
- 对应 `chunk_tags` 不再参与可见结果。
- 索引中的相关向量节点标记不可见或待清理。
- 不立即触发全量重建。

### 7.5 rebuild / repair 设计

#### 7.5.1 rebuild 触发条件
- 索引损坏或不可用。
- tombstone 比例过高。
- embedding 模型升级。
- repair queue 中存在无法自动恢复的问题。

#### 7.5.2 rebuild 流程
1. 从 SQLite 扫描所有有效 chunks。
2. 重新加载或重新生成 embeddings。
3. 重建向量索引。
4. 校验索引状态。
5. 切换当前可用索引。
6. 更新健康状态与日志。

#### 7.5.3 repair 设计
repair queue 负责记录以下异常：
- 索引更新失败
- embedding 缺失或维度不一致
- 后台重建失败

repair 任务需具备：
- 状态字段
- 重试计数
- 最后错误信息
- 可观测输出

### 7.6 EmbeddingProvider 抽象
首版需要一个稳定的 provider 抽象，至少包含：
- `embed(text)`
- `embed_batch(texts)`
- `dimension()`
- `model_id()`
- `max_batch_size()`

工程约束：
- 同一索引版本只允许一种主 embedding 维度。
- 所有 chunk embedding 必须记录 `model_id` 和 `dimension`。
- 模型变更时必须触发 rebuild 或建立新索引版本。

### 7.7 错误处理与降级

#### 7.7.1 错误码
建议首版统一使用以下错误码：
- `INVALID_INPUT`
- `EMBEDDING_FAILED`
- `STORAGE_ERROR`
- `INDEX_UNAVAILABLE`
- `REBUILD_FAILED`
- `TIMEOUT`
- `PARTIAL_RESULT`

#### 7.7.2 统一原则
- ingest 优先一致性：失败时要么回滚，要么明确进入 repair。
- recall 优先可用性：必要时可以返回部分结果，但必须标记清楚。
- 所有降级都必须可观测、可追踪。

### 7.8 可观测性
首版最低可上线标准应包含：
- ingest latency
- recall latency
- rebuild duration
- error count
- repair queue backlog
- index health
- db health
- request-level structured logs

### 7.10 REST API 最小契约

#### `POST /ingest`
请求建议字段：
- `content: string`
- `source?: { type?: string, filename?: string }`
- `tags?: string[]`
- `metadata?: object`
- `options?: { chunk_mode?: string, auto_tag?: boolean }`

响应建议字段：
- `file_id`
- `chunk_ids`
- `index_status`（`completed` / `pending_repair`）
- `warnings`（可选）

#### `POST /recall`
请求建议字段：
- `query: string`
- `top_k?: number`
- `filters?: { file_id?: number, tags?: string[] }`
- `mode?: "basic"`
- `timeout_ms?: number`

响应建议字段：
- `results: RecallResultItem[]`
- `metadata: { top_k, latency_ms, degraded, result_count }`

#### `POST /forget`
请求建议字段：
- `chunk_ids?: number[]`
- `file_id?: number`
- `mode?: "soft" | "hard"`

首版建议默认只开放 `soft`。

响应建议字段：
- `affected_chunks`
- `index_cleanup_status`

#### `POST /admin/rebuild`
请求建议字段：
- `scope?: "all" | "missing_index"`
- `force?: boolean`

该接口建议按异步任务语义设计，返回：
- `job_id`
- `status`
- `submitted_at`

#### `GET /stats`
返回建议字段：
- `chunk_count`
- `tag_count`
- `deleted_chunk_count`
- `repair_queue_size`
- `index_status`

#### `GET /health`
返回建议字段：
- `status`
- `db`
- `index`
- `embedding_provider`
- `version`

### 7.11 上线准备与运行要求
为避免“开发完成但无法发布”，首版应补充最小上线要求：

#### 必填配置
- SQLite 数据文件路径
- embedding provider 类型与模型 ID
- embedding 维度
- provider timeout
- 日志级别
- rebuild 并发/批处理配置

#### 启动前检查
- `schema_meta` 版本是否匹配
- 索引是否存在且可读
- embedding 配置是否完整
- 数据目录是否可写

#### 发布与回滚
- schema 变更必须有迁移脚本或迁移步骤
- rebuild 相关变更必须有回滚说明
- 索引切换必须支持失败后回退到上一可用状态或重新触发重建

#### 运维要求
- 定期备份 SQLite 文件
- 记录 repair queue 积压情况
- 为 rebuild 设置维护窗口或资源限制
- 基于 `health`、`stats` 和错误率建立最小告警规则


---

## 8. 实施路径与交付里程碑

### 8.1 里程碑设计原则
首版里程碑以“先形成稳定闭环，再逐步补齐工程必需能力”为原则，而不是先实现高级算法模块。

### 8.2 Milestone 1：最小写入与召回闭环
目标：完成最小可用主链路。

范围：
- SQLite schema 初始化
- 文本 ingest
- chunk 切分
- embedding provider 抽象
- vector recall basic 模式
- 基础结果模型

交付结果：
- 能写入文本并完成 Top-K 召回
- 能通过基础测试验证 ingest -> recall

### 8.3 Milestone 2：删除、重建与修复能力
目标：补齐生命周期闭环。

范围：
- soft delete
- repair queue
- rebuild pipeline
- 健康检查
- 索引异常处理

交付结果：
- 删除后结果不再召回
- 索引损坏时可通过 rebuild 恢复服务

### 8.4 Milestone 3：Tag 与基础工程化
目标：提升首版工程可用性。

范围：
- tag extraction
- tag normalization
- 过滤能力
- stats / structured logs / metrics
- retrieval log（可选）

交付结果：
- 标签具备基础一致性与可解释性
- 服务具备最小可运维能力

### 8.5 Milestone 4：API 完整化与验收收口
目标：完成首版对外交付能力。

范围：
- `POST /ingest`
- `POST /recall`
- `POST /forget`
- `POST /admin/rebuild`
- `GET /stats`
- `GET /health`
- API 契约测试
- E2E 测试

交付结果：
- 工程团队可基于 API 直接集成和验证
- MVP 达到发布前验收门槛

### 8.6 工程任务拆解建议
为便于工程团队直接排期，建议按以下四类任务拆分：

| 任务类别 | 子任务 | 输出物 | 验收标准 |
|---|---|---|---|
| 开发任务 | schema、repository、embedding provider、vector index adapter、repair job、rebuild job | 可运行模块与代码 | 能支撑 ingest / recall / forget / rebuild 主链路 |
| 接口任务 | ingest/recall/forget/rebuild/stats/health 契约、错误码、输入校验 | API 文档或 OpenAPI | 请求响应结构稳定、错误码可复用 |
| 测试任务 | 单元测试、集成测试、E2E、故障注入、回归测试 | 自动化测试用例与报告 | 主链路和关键故障路径全部覆盖 |
| 上线任务 | 配置清单、迁移步骤、备份恢复、监控告警、发布回滚 | 发布手册与检查清单 | 能完成首版部署、回滚和故障定位 |

建议以该表为基础继续细化 issue、milestone 或迭代任务清单。

---

## 9. 风险

### 9.1 首版主要风险

| 风险 | 影响 | 缓解策略 |
|---|---|---|
| embedding 依赖不稳定 | ingest / recall 失败 | provider 抽象、超时与错误码、批处理策略 |
| 索引与 SQLite 状态不一致 | recall 错误或数据缺失 | 先落 SQLite，索引失败进入 repair queue |
| 删除后仍被召回 | 生命周期闭环失效 | recall 强制过滤 tombstone，增加集成测试 |
| rebuild 不可用 | 故障后无法恢复 | 以 SQLite 为事实源设计独立重建路径 |
| tag 规范化失控 | 标签污染、过滤不稳定 | 显式 normalization 流程与停用策略 |
| 文档范围再次膨胀 | 研发节奏失控 | 能力分级与里程碑边界写入文档并验收 |
| 服务被误用于公网暴露 | 安全风险上升 | 明确首版默认单租户、本地或受信内网部署 |

---

## 10. 验证与演进

### 10.1 首版验证方式
首版不以复杂算法收益为主要验证目标，而以工程闭环验证为主：
- 是否能稳定写入
- 是否能稳定召回
- 是否能正确删除
- 是否能在故障后完成重建
- 是否能通过 API、日志和健康检查定位问题

### 10.2 需要降级处理的高级机制
以下内容统一视为后续阶段或实验方向，不进入首版承诺：
- LIF 脉冲扩散
- Tide / Gram-Schmidt / Weak Signal
- Gravity Field
- Dream Mode
- STDP / 自适应可塑性
- 多语言绑定、WASM、Marketplace、平台协议层

这些能力只在满足以下条件后进入下一阶段：
- Basic Recall 已稳定上线
- 已有离线评测基线
- 已能证明增强机制对目标场景有明确收益
- 引入后仍可保持主路径可降级

### 10.3 首版最小安全边界
虽然完整安全治理不纳入首版主线，但必须明确以下部署边界：
- 首版默认面向单租户、本地环境或受信内网环境部署。
- 若以服务形式对外暴露，至少需要网关鉴权、网络隔离或等效访问控制。
- 管理接口（如 rebuild）必须限制访问范围。
- 错误响应不得暴露内部路径、凭据或原始堆栈信息。
- 输入大小和 metadata 结构必须受限，防止资源滥用和脏数据写入。

---

## 11. 路线图

### 11.1 Phase 1：MVP 工程闭环
范围：
- ingest / recall / forget / rebuild
- SQLite 主存储
- vector recall
- tag extraction & normalization
- REST API
- observability
- errors
- tests

退出条件：
- 主链路全部可用
- 自动化测试覆盖核心场景
- 故障可恢复
- 文档和接口稳定

### 11.2 Phase 2：召回质量增强
候选方向：
- tag-based filtering 强化
- 结果去重与 rerank 优化
- retrieval log 驱动调优
- recall profile 配置化

触发条件：
- MVP 在真实场景跑通
- 已明确主召回瓶颈

### 11.3 Phase 3：结构化关联增强
候选方向：
- tag centroid
- connectome 共现图
- 基础图扩展召回

触发条件：
- 已证明 tag 层具备稳定质量
- 图增强有明确收益假设和评测方案

### 11.4 Phase 4：认知增强实验
候选方向：
- Tide
- Weak Signal
- Gravity rerank
- LIF / Spike propagation

触发条件：
- 已建立离线数据集与对照基线
- 算法收益可量化
- 不破坏主路径时延和稳定性

### 11.5 Phase 5：生态与平台化扩展
候选方向：
- MCP
- Python / Node SDK
- WASM
- 更强的多租户和安全治理

触发条件：
- 核心引擎稳定
- 有明确外部集成需求
- 运维和发布方式成熟

---

## 12. 附录

### 12.1 推荐首版 API 集合
- `POST /ingest`
- `POST /recall`
- `POST /forget`
- `POST /admin/rebuild`
- `GET /stats`
- `GET /health`

### 11.2 推荐首版测试矩阵

#### 单元测试
- chunk 切分
- tag normalization
- repository 行为
- 错误码映射
- 输入校验

#### 集成测试
- ingest -> recall
- forget -> recall
- rebuild -> recall
- SQLite 与索引一致性

#### 端到端测试
- REST API 主链路
- 非法输入与错误路径
- health / stats 可用性

### 11.3 推荐首版验收标准
首版完成交付前，至少满足以下条件：
- 能成功 ingest 文本并形成可召回 chunk。
- 能通过 basic recall 返回相关结果。
- soft delete 后，对应内容不会继续被召回。
- 索引损坏或清空后，系统可通过 rebuild 恢复可用状态。
- 所有核心 API 具备稳定请求响应结构和错误码。
- 系统具备基础健康检查、日志和指标输出。
- 核心链路具备自动化测试。
- 文档明确区分了 MVP 必须项、后续迭代项和实验性方向。

### 11.4 术语说明
为避免工程歧义，首版文档以职责导向表达为主：
- 向量索引层（叙事别名：Cortex）
- 查询编排层（叙事别名：Thalamus）
- 存储层（叙事别名：Hippocampus）
- 后台维护任务（叙事别名：Cerebellum）

仿脑术语仅用于长期愿景辅助表达，不作为首版工程实现边界定义。
