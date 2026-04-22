# Seahorse Design.All 全景设计文档

更新时间：2026-04-22

## 1. 文档定位

本文是 `design.all` 的正式设计与落地基线，目标是把 Seahorse 从现有 MVP 检索服务推进为面向 AI Agent 的认知记忆引擎。

这份文档同时承担三件事：

1. 定义最终目标架构，说明项目要成为什么。
2. 明确当前仓库已经实现到什么程度，避免把愿景误写成现状。
3. 给后续实现提供稳定边界，防止设计漂移、重复造轮子和错误扩面。

本文中的“已落地”仅表示仓库当前代码已经存在并通过验证；“目标态”表示计划实现但当前尚未全部落地。

## 2. 最终目标

Seahorse 的最终目标不是通用向量数据库，也不是 LLM 生成框架，而是一个可组合、可恢复、可审计、可演化的记忆引擎。

最终系统应支持：

- 基础向量检索
- 基于 tag connectome 的联想召回
- 基于 Thalamus 的意图分析、熵估计与门控
- 基于 Cortex 的可持久化向量索引与 archive 恢复
- 基于 Cerebellum 的后台修复、重建、压缩与 dream 流程
- 对 REST、CLI、MCP 与后续多语言 SDK 的统一能力暴露

## 3. 当前实现快照

截至 2026-04-22，仓库内已经落地的 `design.all` 能力如下。

### 3.1 已落地

- `embedding_cache` 已持久化，ingest 会按 `namespace + content_hash + model_id` 复用 embedding。
- `retrieval_log` 已持久化，recall 会记录 `mode`、结果数、耗时与参数快照。
- `cortex / synapse / thalamus / hippocampus / cerebellum / engine` 已有最小骨架。
- `Cortex` 已有 bootstrap 版 facade，可插入、查询，并支持 archive 快照往返与损坏边界校验。
- `connectome` 已持久化，ingest 会根据 chunk tags 更新无向共现边。
- `Synapse` 已能从 connectome 激活邻居信号。
- `Thalamus` 已进入 recall 主链路：
  - 为 query 生成 `worldview + entropy`
  - 为 `tagmemo` 生成最小 gating 决策
  - 把 gating 结果写入响应 metadata 与 `retrieval_log.params_snapshot`
- `Recall` 已支持 `basic` 与 `tagmemo` 两种核心模式。
- `tagmemo` 已具备最小实用语义：
  - 先做 vector recall
  - 结果不足时，从 query 自动提取 seed tags
  - 基于 connectome 激活关联 tags
  - 回填 `SpikeAssociation` 结果
  - 已覆盖超时检查与最终分数排序/截断
- `SpikeAssociation` 结果已追加结构化 metadata，能够解释 seed tags、matched tags 与关联分数。
- `retrieval_log` 已补齐 `spike_depth / emergent_count` 基础写入。
- `/metrics` 已能暴露最近 recall 的 telemetry：
  - worldview 分布
  - entropy 平均值
  - spike depth 平均值
  - emergent 总量
  - `Vector / SpikeAssociation` 来源结果总量
  - association gate 的 allowed / blocked 次数
- `Cerebellum` 已具备 design.all 相关修复闭环：
  - forget 后自动排入 `connectome_rebuild`
  - repair worker 可消费 `connectome_rebuild`
  - 启动时若发现 connectome 缺失且存在多 tag chunk，会自动补排修复任务

### 3.2 未落地或仅有骨架

- 真正的 HNSW 分层图与 mmap/rkyv 持久化尚未实现，当前 `Cortex` 仍以 bootstrap 后端为主。
- `Thalamus` 目前只有最小 gating baseline，还没有 `focus / gravity field / query decomposition`。
- `WeakSignal` / `Tide` / `gravity field` 尚未形成生产可用的召回阶段。
- `Synapse` 当前不是完整 LIF engine，没有 spike trace、涌现检测、STDP 可塑性。
- `Cerebellum` 还没有 dream、compaction、archive refresh 等后台流程。
- `repair_queue` 已可承接 connectome 修复，但尚未形成更细粒度的校验/压缩作业体系。
- MCP、多语言 SDK、WASM、安全过滤、多租户等均未落地为可发布能力。

## 4. 设计原则

### 4.1 单一职责

每个“脑区”模块只负责自己的边界，不跨层偷做别人的事。

| 模块 | 负责 | 不负责 |
| --- | --- | --- |
| `Cortex` | 向量索引、检索、archive | tag 拓扑、意图判断 |
| `Synapse` | connectome、信号扩散、联想回填 | 向量近邻检索 |
| `Thalamus` | query 分析、熵、worldview、门控 | 持久化与索引维护 |
| `Hippocampus` | schema、repository、事务、恢复基础设施 | 认知算法 |
| `Cerebellum` | 后台任务、修复、重建、压缩、dream | 主请求阻塞计算 |
| `Pipeline` | 把各模块编排成 ingest / recall / forget / rebuild 链路 | 长期存储细节与后台调度 |

### 4.2 降级优先

所有高级能力必须可降级为 `basic`。

- recall 失败时优先保证可用性，而不是硬失败。
- ingest 失败时优先保证一致性，而不是部分脏写。
- design.all 新能力不允许破坏 MVP 已有 API 契约。

### 4.3 审计优先

任何高阶召回都必须留下足够的观测与恢复线索。

- recall 记日志
- ingest 记 repair 线索
- archive 恢复失败要有明确错误边界
- 后台修复流程最终应可审计

## 5. 分层架构

### 5.1 接口层

- `seahorse-server`：现有 HTTP/JSON 服务入口
- `seahorse-cli`：命令行入口
- 后续规划：MCP、Python、Node、WASM

### 5.2 编排层

- `pipeline/ingest.rs`
- `pipeline/recall.rs`
- `pipeline/forget.rs`
- `pipeline/rebuild.rs`

编排层的职责是把请求拆成稳定步骤，并把存储、索引、联想和后续高级分析串起来。

### 5.3 认知内核层

#### Cortex

目标态：

- HNSW 分层图
- 插入、查询、删除标记、重建
- archive snapshot
- mmap/rkyv 持久化与恢复

当前态：

- bootstrap 版索引 facade 已存在
- archive 序列化/反序列化已存在
- 损坏 archive 拒绝恢复已覆盖

#### Synapse

目标态：

- connectome 邻接结构
- LIF/spike 扩散
- hop 控制
- spike trace
- emergent pattern 检测

当前态：

- connectome 已落库
- neighbor activation 已可运行
- `tagmemo` 已使用该能力进行最小联想回填

#### Thalamus

目标态：

- query decomposition
- projection entropy
- worldview gate
- weak-signal route
- gravity field

当前态：

- `Thalamus::analyze(query, depth)` 已进入 recall 主路径
- 已能输出最小 `worldview / entropy / route gate`
- 当前 gate 只控制 `tagmemo`，不会提前扩展为真正的 `Tide`

#### Hippocampus

目标态：

- 统一 schema 与 migration
- repository / transaction / recovery
- embedding cache / retrieval log / repair queue
- connectome 与后续 neuron state 的持久化

当前态：

- 已是 design.all 的事实底座
- 当前不重复实现第二套 repository

#### Cerebellum

目标态：

- repair queue 消费
- rebuild / compaction
- connectome maintenance
- dream 批处理

当前态：

- 已能消费 `repair_queue`
- 已闭环 `connectome_rebuild` 修复任务
- 启动恢复阶段能够发现缺失 connectome 并自动补排修复

## 6. 数据模型与存储边界

### 6.1 当前事实边界

当前仓库继续使用 MVP 的 `namespace TEXT` 约束，不做 `namespace_id` 全量改造。

这条边界在 design.all 阶段必须坚持，原因是：

- 当前 schema、repository、HTTP 契约已经围绕 `namespace TEXT` 建立。
- 直接切到 `namespace_id` 会导致一次跨层重构，风险过高。
- design.all 当前更重要的是先把认知能力跑通，而不是提前做租户大迁移。

### 6.2 当前已存在的重要表

- `files`
- `chunks`
- `tags`
- `chunk_tags`
- `repair_queue`
- `embedding_cache`
- `retrieval_log`
- `connectome`

### 6.3 connectome 约束

`connectome` 当前按无向边建模：

- 存储时按 `(tag_i, tag_j)` 排序
- `weight = cooccur_count as REAL`
- ingest 在事务中同步更新

这是当前可接受的 bootstrap 语义，后续如要引入衰减、时间因子或 plasticity，必须在此之上演进，不要重写另一套平行结构。

## 7. 核心运行链路

### 7.1 Ingest

当前 ingest 主链路：

1. 文本预处理
2. chunk 切分
3. 内容去重判断
4. embedding cache 命中检查
5. 缺失 embedding 生成
6. SQLite 事务写入 files/chunks/tags/chunk_tags
7. connectome 更新
8. 向量索引写入
9. 必要时写 repair task

目标补全项：

- richer chunk mode
- neuron state 初始化
- archive snapshot 刷新策略
- connectome repair / rebuild

### 7.2 Recall

当前 recall 已形成两层模式：

#### `basic`

- query embedding
- vector search
- filters
- 去重
- 返回 `Vector`

#### `tagmemo`

- 先执行 `basic`
- 通过 `Thalamus` 判断是否允许联想扩散
- 若结果不足，则自动提取 seed tags
- 通过 `Synapse` 激活 connectome 邻居
- 用关联 tags 回拉 chunk
- 计算最终联想分数
- 返回 `SpikeAssociation`

当前 `tagmemo` 明确具备以下约束：

- 仍受 `timeout_ms` 约束
- 先打分再排序再截断
- 不修改 MVP OpenAPI 正式契约
- 只作为设计版增量能力存在于代码与内部测试，不写入 `docs/mvp-openapi.yaml`

### 7.3 Forget / Rebuild

当前系统已经具备：

- soft delete
- 基础 rebuild job
- repair queue 基础设施
- forget 后的 connectome repair
- 启动时的 connectome 缺失恢复

但 design.all 视角下仍未完成：

- archive 恢复后的拓扑一致性修复
- Synapse / Cortex 联合重建策略
- 更细粒度的 connectome 校验任务

## 8. Recall 模式路线图

| 模式 | 当前状态 | 说明 |
| --- | --- | --- |
| `basic` | 已完成 | MVP 正式模式 |
| `tagmemo` | 已完成最小版 | 向量召回不足时走 connectome 联想 |
| `tide` | 未完成 | 计划引入 entropy / worldview / residual 路由 |
| `dream` | 未完成 | 计划作为后台离线整合而非主链路默认模式 |

## 9. 最终目标态的 recall 编排

目标态 recall 流水线应如下：

1. `Thalamus` 解析 query，得到 `worldview / entropy / focus`
2. `Cortex` 执行主向量召回
3. 若允许弱信号，运行 `Tide` 产生 `WeakSignal`
4. 若允许联想扩散，运行 `Synapse` 产生 `SpikeAssociation`
5. 候选集去重
6. 统一重排
7. 记录 `retrieval_log`
8. 在必要时为后台调优与 dream 提供样本

当前距离该目标的关键缺口是：

- `WeakSignal` 结果源尚不存在
- 统一重排还没有融合 worldview / entropy / gravity
- `Thalamus` 当前只提供最小 gate，尚未产出更丰富的 recall plan

## 10. 恢复、修复与后台任务

### 10.1 Cortex archive

当前已验证：

- snapshot 可以往返
- 损坏快照会被拒绝

目标补全：

- 真正的 mmap 落地
- archive versioning
- 从 archive + SQLite 恢复到可服务状态

### 10.2 Connectome maintenance

当前已具备：

- forget 后触发 `connectome_rebuild`
- repair worker 可执行 `connectome_rebuild`
- 启动恢复时自动检测“connectome 为空但多 tag chunk 仍存在”的缺口并排入修复

仍需补齐：

- 更严格的 connectome 校验任务
- archive 恢复后的边一致性修复
- 非空但部分漂移场景的自动检测

### 10.3 Cerebellum 任务类型

建议后续把后台任务收敛为显式类型，而不是散落在各条链路里：

- `repair_index`
- `repair_connectome`
- `refresh_archive`
- `compact_memory`
- `dream_pass`

## 11. 可观测性要求

design.all 不是只能“跑出结果”，而是要可解释、可归因、可恢复。

至少应持续补齐：

- recall latency
- retrieval mode 分布
- connectome 边数与密度
- repair queue 状态
- rebuild / repair age
- 各来源结果占比：`Vector / WeakSignal / SpikeAssociation`
- worldview、entropy、spike depth、emergent count
- association gate allowed / blocked 统计

## 12. 明确不做的事

以下内容不应在当前 design.all 推进中被混入：

- 不把 design.all 实验能力直接写进 `docs/mvp-openapi.yaml`
- 不为了“全景感”提前上 `namespace_id` 全量重构
- 不在 `hippocampus/` 下重写一套与 `storage/` 平行的 repository
- 不把 `tagmemo` 当前最小版误写成完整 LIF engine
- 不把 `Thalamus` 占位实现误写成真正 Tide 落地

## 13. 建议的实施阶段

### Phase 1：Foundation

目标：

- persistence bootstrap
- embedding cache
- retrieval log
- architecture skeleton
- cortex bootstrap archive
- connectome persistence
- tagmemo minimal recall

状态：已基本完成

### Phase 2：Thalamus 接入

目标：

- 把 `worldview / entropy` 接入 recall metadata
- 建立 query gating 基线
- 为后续 `tide` 留稳定接口

状态：已完成最小基线

### Phase 3：Connectome repair 与后台闭环

目标：

- connectome repair/rebuild
- Cerebellum 任务闭环
- forget/rebuild 后的拓扑一致性

状态：已完成最小闭环，仍需继续深化

### Phase 4：Cortex 真正持久化

目标：

- HNSW 图演进
- mmap/rkyv
- archive 恢复到服务可用

状态：未开始

### Phase 5：高阶能力与生态暴露

目标：

- Tide / WeakSignal
- Dream
- MCP / SDK / WASM
- security / multitenancy

状态：未开始

## 14. 近期执行优先级

按当前仓库状态，建议后续实现顺序为：

1. 推进 `Cortex` 的真正持久化，把 bootstrap archive 演进到可恢复的 HNSW + mmap/rkyv。
2. 继续细化 `Cerebellum` 的 connectome 校验与 archive 恢复后拓扑修复，而不只停留在启动补排。
3. 为 `Thalamus` 补更明确的 recall plan 输出，再为未来 `Tide / WeakSignal` 留出稳定入口。

## 15. 验收标准

design.all 每推进一层，都至少满足以下要求：

- 有明确的失败测试或回归测试
- 不破坏现有 MVP 契约
- 有降级边界
- 有恢复或修复路径
- 有可观测数据或日志字段
- 每个功能独立提交，可从提交历史追踪

## 16. 结论

Seahorse 的 `design.all` 不应该继续停留在“宏大蓝图文档”，而应围绕当前仓库可持续演进的事实地基推进。

当前地基已经具备：

- 存储扩展
- recall 审计
- 架构骨架
- cortex bootstrap
- connectome 持久化
- synapse 最小激活
- tagmemo 最小联想召回
- thalamus 最小 gating
- recent recall telemetry
- cerebellum 最小 connectome 恢复闭环

接下来的关键，不是继续铺更大的概念，而是把 `Thalamus`、`Cerebellum` 和 `Cortex` 的剩余闭环按阶段补齐，直到 Seahorse 真正成为一个可运行、可恢复、可审计的认知记忆引擎。
