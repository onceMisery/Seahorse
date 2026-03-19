# Seahorse MVP Agent Prompts Batch 2

> 用途：这是第二批开发任务的可直接分发 prompt，覆盖 `FB-006 ~ FB-010`。
>
> 对应清单：
> - `docs/mvp-first-batch-issues.md`
> - `docs/mvp-agent-handoff.md`

## 1. 使用说明

- 这批 prompt 默认建立在 `FB-001 ~ FB-005` 已经开始或已完成的前提上。
- 发给 agent 前，不要删减“必须阅读”“唯一负责范围”“要求”“完成定义”。
- 若 agent 发现前置骨架未准备好，应停止并回报，不要自行补全不属于自己范围的模块。
- 这批 prompt 主要目的是把 MVP 从“代码骨架”推进到“可组成主链路的基础模块”。

## 2. Prompt 6: `FB-006`

对应任务：落实 schema 索引、约束与 `schema_meta` 校验

```text
你负责 Seahorse MVP 的 `FB-006`：落实 schema 索引、约束与 `schema_meta` 校验。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-schema.sql
5. docs/mvp-config.example.toml

任务目标：
把 schema 从“只有表定义”推进到“可被运行时校验和依赖”的状态。

你的唯一负责范围：
- migrations/**
- crates/seahorse-core/src/storage/**
- crates/seahorse-core/src/types/storage.rs

要求：
1. 落实 schema 中的必要索引、唯一约束和状态字段约束。
2. 实现 `schema_meta` 读取与启动前校验逻辑，至少覆盖：
   - schema_version
   - index_version
   - embedding_model_id
   - embedding_dimension
3. schema 不一致时必须返回明确错误，不能静默忽略。
4. 不要实现 HTTP 层，不要实现 embedding provider，不要实现 pipeline。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- schema 约束与 `docs/mvp-schema.sql` 一致
- 启动时可校验 `schema_meta`
- schema 不匹配时会拒绝继续运行并输出可诊断错误

交付时必须说明：
- 修改了哪些文件
- 实际校验了哪些 schema_meta 键
- 哪些 schema 约束最依赖后续代码配合
- 当前仍未覆盖的边界
```

## 3. Prompt 7: `FB-007`

对应任务：实现 repository 层与事务边界

```text
你负责 Seahorse MVP 的 `FB-007`：实现 repository 层与事务边界。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-schema.sql

任务目标：
建立可复用的 storage/repository 层，确保 SQLite 写入边界稳定，供 ingest/forget/rebuild 复用。

你的唯一负责范围：
- crates/seahorse-core/src/storage/**
- crates/seahorse-core/src/types/storage.rs

要求：
1. repository 层必须支持：
   - files 写入
   - chunks 写入
   - tags 写入
   - chunk_tags 写入
   - 查询基础 chunk / tag / file 数据
2. 必须建立清晰事务边界：
   - `files/chunks/tags/chunk_tags` 在同一 SQLite 事务中提交
   - 向量索引更新不得混入 SQLite 事务
3. 为后续 `soft delete` 与 `repair_queue` 预留接口，但当前不需要全部实现。
4. 不要实现向量索引，不要实现 HTTP handler，不要改 OpenAPI。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- repository 可被 ingest pipeline 调用
- 单事务写入 `files/chunks/tags/chunk_tags`
- 接口命名稳定，便于下游 agent 依赖

交付时必须说明：
- 修改了哪些文件
- 哪些 repository 方法已经可用
- 事务边界如何实现
- 为后续 `forget`/`repair` 预留了哪些接口
```

## 4. Prompt 8: `FB-008`

对应任务：接入首个 embedding provider adapter

```text
你负责 Seahorse MVP 的 `FB-008`：接入首个 embedding provider adapter。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-config.example.toml

任务目标：
在已经冻结的 `EmbeddingProvider` trait 上，提供第一个可运行的 provider 实现，供 ingest 和 recall 调用。

你的唯一负责范围：
- crates/seahorse-core/src/embedding/**
- crates/seahorse-core/src/types/**

要求：
1. 至少提供一个可运行 provider：
   - 推荐先做 `stub` 或本地开发 provider
2. provider 必须返回稳定的：
   - embedding 向量
   - model_id
   - dimension
3. provider 超时、失败、维度不匹配必须映射到已有错误模型。
4. 不要实现 pipeline，不要改 schema，不要改 HTTP 层。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- provider 可被下游模块实例化并调用
- 错误处理与 trait 定义一致
- 至少支持单条和批量 embedding

交付时必须说明：
- 修改了哪些文件
- 提供了哪些 provider 实现
- 默认开发模式下应如何使用该 provider
- 哪些生产能力仍未覆盖
```

## 5. Prompt 9: `FB-009`

对应任务：实现向量索引 adapter 骨架

```text
你负责 Seahorse MVP 的 `FB-009`：实现向量索引 adapter 骨架。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-config.example.toml

任务目标：
冻结索引层接口，让 ingest、recall、rebuild 可以依赖同一套 index adapter。

你的唯一负责范围：
- crates/seahorse-core/src/index/**
- crates/seahorse-core/src/types/index.rs

要求：
1. 至少定义并实现最小索引接口：
   - insert
   - search
   - mark_deleted 或等价可见性控制
   - rebuild 或 rebuild_from_entries
2. 先做 MVP 所需的最小 adapter 骨架，不要引入 Tide、LIF、connectome 等能力。
3. index 层只做加速，不得把 SQLite 事实源逻辑搬进来。
4. 不要改 repository，不要改 HTTP 层，不要新增数据库表。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- ingest / recall / rebuild 后续都可依赖此 adapter
- 索引接口命名稳定
- 支持最小可见性控制

交付时必须说明：
- 修改了哪些文件
- index adapter 暴露了哪些核心接口
- 目前是骨架还是已有最小可运行实现
- 下游 pipeline 该如何调用它
```

## 6. Prompt 10: `FB-010`

对应任务：实现 `dedup_mode` 与 `file_hash` 逻辑

```text
你负责 Seahorse MVP 的 `FB-010`：实现 `dedup_mode` 与 `file_hash` 逻辑。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-schema.sql
5. docs/mvp-config.example.toml

任务目标：
实现 ingest 入口的幂等与重复写入策略，避免后续主链路在重试和回放时行为不一致。

你的唯一负责范围：
- crates/seahorse-core/src/pipeline/**
- crates/seahorse-core/src/types/**

要求：
1. 支持以下策略：
   - reject
   - upsert
   - allow
2. 必须基于规范化内容计算 `file_hash`。
3. 行为必须与设计文档一致：
   - reject：命中重复直接返回已有结果
   - upsert：软删除旧版本，再写入新版本
   - allow：允许重复写入
4. 不要直接实现完整 ingest pipeline，只聚焦幂等和重复写入策略模块。
5. 不要修改 repository / index / HTTP 层。
6. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- `dedup_mode` 可被后续 ingest pipeline 直接调用
- `file_hash` 计算逻辑稳定
- 三种策略行为清晰可测

交付时必须说明：
- 修改了哪些文件
- `file_hash` 基于什么输入计算
- 三种 `dedup_mode` 的行为差异
- 对 repository 或 pipeline 的依赖点有哪些
```
