# Seahorse MVP Agent Prompts Batch 3

> 用途：这是第三批开发任务的可直接分发 prompt，覆盖 `FB-011 ~ FB-015`。
>
> 对应清单：
> - `docs/mvp-first-batch-issues.md`
> - `docs/mvp-agent-handoff.md`

## 1. 使用说明

- 这批 prompt 默认建立在 `FB-001 ~ FB-010` 已经开始或已完成的前提上。
- 这批任务会把系统从“基础模块就绪”推进到“最小业务闭环”。
- 发给 agent 前，不要删减“必须阅读”“唯一负责范围”“要求”“完成定义”。
- 若前置接口尚未冻结，agent 应暂停并回报，不要自行修改契约。

## 2. Prompt 11: `FB-011`

对应任务：实现 ingest pipeline

```text
你负责 Seahorse MVP 的 `FB-011`：实现 ingest pipeline。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml
5. docs/mvp-schema.sql
6. docs/mvp-config.example.toml

任务目标：
实现 MVP 主写入链路，把预处理、切分、embedding、事务写入和索引更新串起来。

你的唯一负责范围：
- crates/seahorse-core/src/pipeline/**
- crates/seahorse-core/src/jobs/**
- crates/seahorse-core/src/types/**

要求：
1. 主链路至少覆盖：
   - input validation
   - preprocess
   - chunk
   - embed
   - SQLite transaction write
   - vector index update
2. 必须依赖已有 repository 和 index/provider 接口，不要重写一套新抽象。
3. 索引失败时：
   - 不回滚已提交 SQLite 数据
   - 更新 `files.ingest_status`
   - 更新 `chunks.index_status`
   - 写入 `repair_queue`
4. 不要实现 HTTP handler，不要改 OpenAPI，不要改 migration。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- ingest pipeline 可被 API 层直接调用
- 成功写入时状态字段正确
- 索引失败时 repair 路径正确
- 不破坏 SQLite 事实源约束

交付时必须说明：
- 修改了哪些文件
- ingest pipeline 的主要步骤
- 成功路径和失败路径如何分流
- 还依赖哪些前置能力才能完全跑通
```

## 3. Prompt 12: `FB-012`

对应任务：实现 basic recall pipeline

```text
你负责 Seahorse MVP 的 `FB-012`：实现 basic recall pipeline。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml
5. docs/mvp-config.example.toml

任务目标：
实现 MVP 唯一召回主路径：query embedding -> vector top-k -> 回表组装 -> 去重与过滤。

你的唯一负责范围：
- crates/seahorse-core/src/pipeline/**
- crates/seahorse-core/src/types/**
- crates/seahorse-core/src/health/**

要求：
1. 只实现 `basic` 模式，不要实现 Tide、LIF、WeakSignal、SpikeAssociation。
2. 主链路至少覆盖：
   - query embedding
   - vector top-k
   - load chunk metadata
   - dedup
   - tombstone / visibility filter
   - result assembly
3. 结果中的 `source_type` 固定为 `Vector`。
4. 如果索引不可用且无有效降级结果，必须返回明确错误，不得伪造成功。
5. 不要实现 HTTP handler，不要改 schema，不要新增召回模式。
6. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- recall pipeline 可被 API 层直接调用
- 结果结构与 OpenAPI 契约一致
- deleted / failed visibility 数据不会被错误召回

交付时必须说明：
- 修改了哪些文件
- recall pipeline 的步骤
- 去重和过滤规则是什么
- 索引异常时如何处理
```

## 4. Prompt 13: `FB-013`

对应任务：暴露 `POST /ingest` 与 `POST /recall`

```text
你负责 Seahorse MVP 的 `FB-013`：暴露 `POST /ingest` 与 `POST /recall`。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml

任务目标：
把 ingest / recall 两条主链路通过 HTTP 接口稳定暴露出来，并对齐统一响应包络。

你的唯一负责范围：
- crates/seahorse-server/src/**
- tests/integration/api/**
- tests/e2e/**

要求：
1. 必须实现：
   - `POST /ingest`
   - `POST /recall`
2. 请求字段、响应字段、错误结构必须严格对齐 `docs/mvp-openapi.yaml`。
3. 必须复用已有统一响应包络，不要为单个接口自定义返回格式。
4. 参数校验必须在 API 层或 service 边界清晰执行。
5. 不要修改底层 pipeline、repository、index、schema。
6. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- 这两个接口可被联调
- 请求响应结构稳定
- 错误码与 HTTP 状态可复用

交付时必须说明：
- 修改了哪些文件
- 两个接口的 handler 如何调用下游 service
- 做了哪些参数校验
- 当前仍是 stub 的部分有哪些
```

## 5. Prompt 14: `FB-014`

对应任务：建立 `ingest -> recall` 集成测试

```text
你负责 Seahorse MVP 的 `FB-014`：建立 `ingest -> recall` 集成测试。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml
5. docs/mvp-schema.sql

任务目标：
建立第一条真正的自动化回归链路，验证最小 MVP 主路径。

你的唯一负责范围：
- tests/integration/**
- tests/e2e/**
- tests/fixtures/**

要求：
1. 至少覆盖：
   - ingest 成功
   - recall 成功
   - 非法输入失败
2. 测试必须验证：
   - 返回包络
   - 结果结构
   - 至少一条写入内容确实能被召回
3. 不要修改业务实现代码，除非为了修正显式测试接口错误且已在说明中注明。
4. 不要扩展测试范围到 Tide、LIF、MCP、SDK。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- 自动化测试能覆盖 `ingest -> recall`
- 至少有一个非法输入用例
- 测试结果可作为第一轮回归基线

交付时必须说明：
- 修改了哪些文件
- 新增了哪些测试场景
- 测试依赖哪些 fixture 或环境前提
- 当前还缺哪些场景未覆盖
```

## 6. Prompt 15: `FB-015`

对应任务：实现 soft delete 数据落库

```text
你负责 Seahorse MVP 的 `FB-015`：实现 soft delete 数据落库。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-schema.sql
5. docs/mvp-openapi.yaml

任务目标：
实现 MVP 生命周期闭环中的第一步：把 forget 的核心存储行为落库，但不触发全量 rebuild。

你的唯一负责范围：
- crates/seahorse-core/src/storage/**
- crates/seahorse-core/src/types/storage.rs
- migrations/**（仅当确需微调 soft delete 相关 schema 时）

要求：
1. soft delete 必须通过以下字段表达：
   - `chunks.is_deleted`
   - `chunks.deleted_at`
2. 删除行为不能直接物理删除 chunk。
3. 不允许在这个任务里直接实现完整 HTTP 接口或完整 recall 过滤逻辑。
4. 应为后续 `POST /forget`、recall tombstone 过滤、index visibility 更新提供清晰存储接口。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- soft delete 能正确更新数据
- 不触发全量 rebuild
- repository 层已暴露后续可复用接口

交付时必须说明：
- 修改了哪些文件
- soft delete 的 repository 接口是什么
- 哪些行为留给后续 `FB-016` 和 `FB-019`
- 是否需要补充 migration 或索引
```
