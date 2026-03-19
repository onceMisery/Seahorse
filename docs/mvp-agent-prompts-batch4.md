# Seahorse MVP Agent Prompts Batch 4

> 用途：这是第四批开发任务的可直接分发 prompt，覆盖 `FB-016 ~ FB-020`。
>
> 对应清单：
> - `docs/mvp-first-batch-issues.md`
> - `docs/mvp-agent-handoff.md`

## 1. 使用说明

- 这批 prompt 默认建立在 `FB-001 ~ FB-015` 已经开始或已完成的前提上。
- 这批任务会把系统从“最小业务闭环”推进到“生命周期闭环与基础运维闭环”。
- 发给 agent 前，不要删减“必须阅读”“唯一负责范围”“要求”“完成定义”。
- 若前置接口尚未稳定，agent 应停止并回报，不要自行扩展契约。

## 2. Prompt 16: `FB-016`

对应任务：recall 过滤 tombstone 与索引可见性标记

```text
你负责 Seahorse MVP 的 `FB-016`：recall 过滤 tombstone 与索引可见性标记。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml
5. docs/mvp-schema.sql

任务目标：
让 soft delete 真正生效，保证被删除 chunk 不会继续被 recall 返回，并让索引层具备最小可见性控制。

你的唯一负责范围：
- crates/seahorse-core/src/index/**
- crates/seahorse-core/src/pipeline/**
- crates/seahorse-core/src/types/**

要求：
1. recall 结果必须强制过滤：
   - `chunks.is_deleted = 1`
   - `chunks.index_status = deleted`
   - 其他不可见状态
2. 索引层需要支持最小可见性控制：
   - `mark_deleted` 或等价能力
   - recall 时跳过不可见节点
3. 不要实现 HTTP 接口，不要修改 repository schema，不要新增召回模式。
4. `source_type` 仍固定为 `Vector`。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- 已删除 chunk 不再出现在 recall 结果中
- 不依赖全量 rebuild 也能实现最小删除可见性
- recall pipeline 与 index adapter 的边界清晰

交付时必须说明：
- 修改了哪些文件
- tombstone 过滤在哪里执行
- 索引可见性如何实现
- 还留给后续 rebuild 的部分有哪些
```

## 3. Prompt 17: `FB-017`

对应任务：实现 `repair_queue` worker

```text
你负责 Seahorse MVP 的 `FB-017`：实现 `repair_queue` worker。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-schema.sql

任务目标：
实现 repair 队列的状态流转和重试机制，让索引失败、embedding 缺失等异常进入可恢复路径。

你的唯一负责范围：
- crates/seahorse-core/src/jobs/**
- crates/seahorse-core/src/pipeline/**
- crates/seahorse-core/src/types/**

要求：
1. `repair_queue` 至少支持状态：
   - pending
   - running
   - succeeded
   - failed
   - deadletter
2. 必须支持：
   - 重试计数
   - 最后错误信息
   - 可观测状态更新
3. repair worker 先做 MVP 最小能力，不要引入复杂调度系统。
4. 不要实现 HTTP 接口，不要改 OpenAPI，不要新增数据库表。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- `repair_queue` 状态流转正确
- 索引失败场景可进入 repair 路径
- 重试和 deadletter 逻辑明确

交付时必须说明：
- 修改了哪些文件
- repair worker 如何扫描和执行任务
- 重试策略是什么
- 哪些 repair 类型已经支持，哪些还未支持
```

## 4. Prompt 18: `FB-018`

对应任务：实现 `maintenance_jobs` 与 rebuild job manager

```text
你负责 Seahorse MVP 的 `FB-018`：实现 `maintenance_jobs` 与 rebuild job manager。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml
5. docs/mvp-schema.sql

任务目标：
建立异步 rebuild 作业管理，让系统可以创建、查询和限制 rebuild 作业，而不是把 rebuild 做成不可追踪的一次性动作。

你的唯一负责范围：
- crates/seahorse-core/src/jobs/**
- crates/seahorse-core/src/types/**
- crates/seahorse-core/src/health/**

要求：
1. `maintenance_jobs` 至少支持：
   - queued
   - running
   - succeeded
   - failed
   - cancelled
2. rebuild job manager 至少支持：
   - 创建作业
   - 查询作业
   - 记录 progress
   - 限制同一 namespace 只有一个激活 rebuild
3. `force` 策略应有明确行为，但不要超出 MVP。
4. 不要实现 HTTP handler，不要改 OpenAPI，不要把 job manager 与 API 耦死。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- rebuild 作业可持久化
- 作业状态可查询
- 并发保护有效
- 可被 `POST /admin/rebuild` 和 `GET /admin/jobs/{job_id}` 复用

交付时必须说明：
- 修改了哪些文件
- rebuild job manager 提供了哪些接口
- progress 是如何表示的
- `force` 和并发冲突时的行为是什么
```

## 5. Prompt 19: `FB-019`

对应任务：实现生命周期 API

```text
你负责 Seahorse MVP 的 `FB-019`：实现生命周期 API。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml

任务目标：
把生命周期闭环通过 HTTP 接口暴露出来，覆盖 forget、rebuild 提交和 job 查询。

你的唯一负责范围：
- crates/seahorse-server/src/**
- tests/integration/api/**
- tests/e2e/**

要求：
1. 必须实现：
   - `POST /forget`
   - `POST /admin/rebuild`
   - `GET /admin/jobs/{job_id}`
2. 请求字段、响应字段、状态值必须严格对齐 `docs/mvp-openapi.yaml`。
3. 必须复用统一响应包络，不要为 admin 接口单独定义返回格式。
4. 不要修改底层 storage、jobs、pipeline 的契约。
5. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- 生命周期 API 可联调
- forget、rebuild、job 查询返回结构稳定
- 错误码和 HTTP 状态可复用

交付时必须说明：
- 修改了哪些文件
- 这三个接口分别调用了哪些下游 service
- 做了哪些参数校验
- 当前仍依赖哪些未完成能力
```

## 6. Prompt 20: `FB-020`

对应任务：健康检查与基础统计

```text
你负责 Seahorse MVP 的 `FB-020`：健康检查与基础统计。

开始前必须阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-first-batch-issues.md
3. docs/mvp-agent-handoff.md
4. docs/mvp-openapi.yaml
5. docs/mvp-schema.sql

任务目标：
实现最小可运维面，让系统可以通过 `health` 与 `stats` 输出当前健康状态和核心计数。

你的唯一负责范围：
- crates/seahorse-core/src/health/**
- crates/seahorse-core/src/observability/**
- crates/seahorse-server/src/**

要求：
1. 必须实现：
   - `GET /health`
   - `GET /stats`
2. `health.status` 至少区分：
   - ok
   - degraded
   - failed
3. `stats` 至少返回：
   - chunk_count
   - tag_count
   - deleted_chunk_count
   - repair_queue_size
   - index_status
4. 不要扩展到完整 metrics 系统，不要引入 MVP 范围外的运维平台集成。
5. 响应字段必须对齐 `docs/mvp-openapi.yaml`。
6. 你不是单独在代码库里工作，其他 agents 也可能同时修改别的文件；不要回退他人的改动。

完成定义：
- `GET /health` 可反映 DB / index / provider 的基础状态
- `GET /stats` 可输出核心计数
- 返回结构稳定，可作为后续告警和监控基础

交付时必须说明：
- 修改了哪些文件
- `health` 状态判定规则是什么
- `stats` 的各字段从哪里聚合而来
- 哪些能力仍属于后续 M3/M4 范围
```
