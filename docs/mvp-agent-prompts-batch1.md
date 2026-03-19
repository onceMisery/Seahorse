# Seahorse MVP Agent Prompts Batch 1

> 用途：这是第一批开发任务的可直接分发 prompt，覆盖 `FB-001 ~ FB-005`。
>
> 对应清单：
> - `docs/mvp-first-batch-issues.md`
> - `docs/mvp-agent-handoff.md`

## 1. 使用说明

- 每个 prompt 默认对应一个独立 agent。
- 发给 agent 前，不要再删减“必须阅读”“唯一负责范围”“要求”“完成定义”。
- 如果 agent 平台支持 structured task，建议把 `Issue ID` 和 `文件范围` 单独作为 metadata 传入。
- 这批 prompt 只覆盖第一轮启动项，不覆盖完整 MVP。

## 2. Prompt 1: `FB-001`

对应任务：初始化 workspace 与 crate 骨架

```text
你负责 Seahorse MVP 的 `FB-001`：初始化 workspace 与 crate 骨架。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-issue-breakdown.md
3. docs/mvp-first-batch-issues.md
4. docs/mvp-agent-handoff.md
5. docs/mvp-openapi.yaml
6. docs/mvp-schema.sql
7. docs/mvp-config.example.toml

任务目标：
建立 Rust workspace、`seahorse-core`、`seahorse-server`、`seahorse-cli` 三个 crate 的最小可编译骨架。

你的唯一负责范围：
- Cargo.toml
- crates/seahorse-core/**
- crates/seahorse-server/**
- crates/seahorse-cli/**

要求：
1. 目标结构必须对齐 `docs/mvp-agent-handoff.md`。
2. 只建立骨架，不实现业务逻辑。
3. 不要修改 docs 文档。
4. 不要新增 MVP 范围外的 crate 或目录。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- workspace 可解析
- 各 crate 至少有最小 `lib.rs` 或 `main.rs`
- 目录结构与 handoff 文档一致

交付时必须说明：
- 修改了哪些文件
- 当前是否可编译
- 还缺哪些前置依赖才能继续开发
- 已知风险或阻塞项
```

## 3. Prompt 2: `FB-002`

对应任务：建立 SQLite 初始 migration

```text
你负责 Seahorse MVP 的 `FB-002`：建立 SQLite 初始 migration。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-issue-breakdown.md
3. docs/mvp-first-batch-issues.md
4. docs/mvp-agent-handoff.md
5. docs/mvp-schema.sql

任务目标：
把 `docs/mvp-schema.sql` 落成第一版 migration 文件。

你的唯一负责范围：
- migrations/**
如果目录不存在，你可以创建它。
目标文件优先使用：
- migrations/0001_init.sql

要求：
1. migration 必须覆盖这些表：
   - files
   - chunks
   - tags
   - chunk_tags
   - repair_queue
   - maintenance_jobs
   - schema_meta
2. 状态字段、索引、唯一约束必须与 `docs/mvp-schema.sql` 一致。
3. 保留 namespace 预留，但 MVP 默认值固定为 `default`。
4. 不要实现运行时代码，不要修改 server、pipeline、index 代码。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- `migrations/0001_init.sql` 存在
- 表、索引、约束与设计文档一致
- 可作为第一版初始化 migration 使用

交付时必须说明：
- 修改了哪些文件
- migration 与 `docs/mvp-schema.sql` 有无偏差
- 哪些约束你认为后续实现最容易踩坑
```

## 4. Prompt 3: `FB-003`

对应任务：定义 `EmbeddingProvider` trait 与错误模型

```text
你负责 Seahorse MVP 的 `FB-003`：定义 `EmbeddingProvider` trait 与错误模型。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-issue-breakdown.md
3. docs/mvp-first-batch-issues.md
4. docs/mvp-agent-handoff.md
5. docs/mvp-config.example.toml

任务目标：
冻结 embedding 接口，避免后续 pipeline 和 index 并行开发时接口漂移。

你的唯一负责范围：
- crates/seahorse-core/src/embedding/**
- crates/seahorse-core/src/types/**

要求：
1. trait 至少包含：
   - embed
   - embed_batch
   - model_id
   - dimension
   - max_batch_size
2. 必须定义清晰错误类型，至少覆盖：
   - provider timeout
   - provider failure
   - dimension mismatch
3. 接口命名和返回风格要稳定，供后续 agent 直接依赖。
4. 不要接入具体 provider，不要实现 pipeline，不要改 schema。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- trait 已可被其他模块引用
- 错误模型清晰可扩展
- 与 MVP 文档中的 provider 契约一致

交付时必须说明：
- 修改了哪些文件
- 对外暴露了哪些 trait / type
- 下游 agent 应该如何依赖这些接口
- 当前未实现的部分有哪些
```

## 5. Prompt 4: `FB-004`

对应任务：实现基础 chunker 与输入预处理模块

```text
你负责 Seahorse MVP 的 `FB-004`：实现基础 chunker 与输入预处理模块。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-issue-breakdown.md
3. docs/mvp-first-batch-issues.md
4. docs/mvp-agent-handoff.md
5. docs/mvp-config.example.toml

任务目标：
实现可稳定复现的 chunk 与预处理逻辑，供 ingest pipeline 使用。

你的唯一负责范围：
- crates/seahorse-core/src/pipeline/**
- crates/seahorse-core/src/types/**

要求：
1. 支持基础文本预处理：
   - 规范换行
   - 去除无意义控制字符
   - 生成 `content_hash`
2. 支持最小 chunker：
   - 相同输入必须产生稳定 chunk 序列
   - 先做 MVP 所需的基础固定规则切分
3. 输入约束必须对齐设计文档：
   - content 大小
   - tag 数量与长度
   - metadata 约束
4. 不要实现 ingest 全链路，不要写 repository，不要改 HTTP 层。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- 预处理模块可独立调用
- chunk 结果稳定
- 输入非法时有清晰错误
- 能被后续 ingest pipeline 直接复用

交付时必须说明：
- 修改了哪些文件
- chunk 规则是什么
- 输入校验覆盖了哪些边界
- 还依赖哪些上游或下游接口
```

## 6. Prompt 5: `FB-005`

对应任务：建立 server 骨架与统一响应包络

```text
你负责 Seahorse MVP 的 `FB-005`：建立 server 骨架与统一响应包络。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-issue-breakdown.md
3. docs/mvp-first-batch-issues.md
4. docs/mvp-agent-handoff.md
5. docs/mvp-openapi.yaml

任务目标：
冻结 HTTP 层骨架和统一响应结构，避免后续 handler 并行开发冲突。

你的唯一负责范围：
- crates/seahorse-server/src/**

要求：
1. 建立最小 server 骨架和路由注册。
2. 实现统一响应包络：
   - success
   - data
   - error
   - request_id
3. 结构必须对齐 `docs/mvp-openapi.yaml`。
4. 路由可以先是空实现或 stub，但接口名和响应壳子必须稳定。
5. 不要实现底层业务逻辑，不要改 schema，不要改 embedding/index/pipeline。
6. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- server 可启动
- 基础路由已注册
- 统一响应结构可复用
- 后续 `POST /ingest`、`POST /recall`、`GET /health` 等 handler 可直接挂接

交付时必须说明：
- 修改了哪些文件
- 当前注册了哪些路由
- 统一响应结构如何复用
- 哪些 handler 仍是占位实现
```
