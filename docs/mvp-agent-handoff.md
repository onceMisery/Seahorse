# Seahorse MVP Agent 交接手册

> 目的：让多个开发 agents 可以基于同一套边界、契约和文件归属并行开发，尽量避免冲突和返工。

## 1. 开发前必须读取的文档

所有 agents 开始工作前，至少阅读以下文档：

1. `docs/mvp-design-and-roadmap.md`
2. `docs/mvp-issue-breakdown.md`
3. `docs/mvp-openapi.yaml`
4. `docs/mvp-schema.sql`
5. `docs/mvp-config.example.toml`

若 agent 只负责某一层，也不能跳过 `mvp-design-and-roadmap.md`，否则容易把非目标做进 MVP。

## 2. MVP 代码目标结构

建议 agents 以如下目录为目标创建代码，后续所有任务都围绕这棵树展开：

```text
seahorse/
├── Cargo.toml
├── crates/
│   ├── seahorse-core/
│   │   ├── src/
│   │   │   ├── config/
│   │   │   ├── types/
│   │   │   ├── storage/
│   │   │   ├── embedding/
│   │   │   ├── index/
│   │   │   ├── pipeline/
│   │   │   ├── jobs/
│   │   │   ├── health/
│   │   │   └── observability/
│   ├── seahorse-server/
│   │   └── src/
│   │       ├── api/
│   │       ├── handlers/
│   │       └── state/
│   └── seahorse-cli/
│       └── src/
├── migrations/
│   └── 0001_init.sql
├── tests/
│   ├── integration/
│   ├── e2e/
│   └── fixtures/
└── docs/
```

说明：
- `seahorse-core` 承载所有业务逻辑和存储/索引实现。
- `seahorse-server` 只做 HTTP 编排、参数校验和响应转换。
- `seahorse-cli` 在 MVP 阶段可只承载最小管理命令，优先级低于 server。

## 3. 共享工程规则

- 所有 agents 必须遵守 MVP 范围，不得实现 Tide、LIF、connectome、MCP、SDK。
- 向量召回是 MVP 唯一候选集生成路径。
- SQLite 是唯一事实源，索引是加速层。
- 任何索引失败都不能伪装成成功，必须反映到状态字段和 repair/job 记录。
- 所有接口都必须使用统一响应包络：`success/data/error/request_id`。
- 任何新增状态值、字段名、错误码，都必须先对齐 `docs/mvp-design-and-roadmap.md`。

## 4. 推荐的 agent 分工

### Agent A：Schema 与 Storage

负责范围：
- `migrations/**`
- `crates/seahorse-core/src/storage/**`
- `crates/seahorse-core/src/types/storage.rs`

对应 issue：
- `M1-E1-I1`
- `M1-E1-I2`
- `M1-E1-I3`
- `M1-E1-I4`
- `M2-E1-I1`

交付物：
- SQLite migration
- repository 层
- 事务边界
- schema 版本检查
- soft delete 落库逻辑

禁止越界：
- 不实现 HTTP handler
- 不决定 API 错误码映射
- 不直接实现向量检索算法

### Agent B：Embedding 与 Index

负责范围：
- `crates/seahorse-core/src/embedding/**`
- `crates/seahorse-core/src/index/**`
- `crates/seahorse-core/src/types/index.rs`

对应 issue：
- `M1-E2-I2`
- `M1-E2-I3`
- `M1-E3-I2`
- `M2-E1-I3`
- `M2-E2-I3`

交付物：
- `EmbeddingProvider` 抽象
- 第一个 provider adapter
- 向量索引 adapter
- 删除可见性控制
- rebuild 所需索引重建能力

禁止越界：
- 不改 SQLite schema
- 不实现 HTTP 接口层
- 不自行发明新的状态字段

### Agent C：Pipeline 与 Jobs

负责范围：
- `crates/seahorse-core/src/pipeline/**`
- `crates/seahorse-core/src/jobs/**`
- `crates/seahorse-core/src/health/**`

对应 issue：
- `M1-E2-I4`
- `M1-E3-I1`
- `M1-E3-I3`
- `M1-E3-I4`
- `M2-E2-I1`
- `M2-E2-I2`
- `M2-E2-I4`
- `M2-E3-I1`
- `M2-E3-I2`

交付物：
- ingest pipeline
- recall pipeline
- dedup 策略
- repair worker
- rebuild job manager
- health 状态聚合

禁止越界：
- 不修改 OpenAPI 契约
- 不定义新的数据库表
- 不把 tag 变成召回主路径

### Agent D：API 与契约测试

负责范围：
- `crates/seahorse-server/src/**`
- `tests/e2e/**`
- `tests/integration/api/**`

对应 issue：
- `M4-E1-I1`
- `M4-E1-I2`
- `M4-E1-I3`
- `M4-E1-I4`
- `M4-E2-I1`
- `M4-E2-I2`

交付物：
- HTTP handlers
- 统一响应包络
- API 契约实现
- OpenAPI 对齐
- contract tests
- E2E tests

禁止越界：
- 不修改底层 schema
- 不实现 provider 细节
- 不新增设计稿未定义的 API

### Agent E：Observability 与 Runbook

负责范围：
- `crates/seahorse-core/src/observability/**`
- `tests/fixtures/**`
- `docs/runbooks/**` 或 `docs/*.md` 中与运行有关的文档

对应 issue：
- `M2-E3-I3`
- `M3-E2-I1`
- `M3-E2-I2`
- `M3-E2-I3`
- `M4-E2-I3`
- `M4-E2-I4`

交付物：
- 结构化日志
- metrics
- `GET /stats` 支撑逻辑
- 故障注入测试
- 部署/回滚/重建运行手册
- 发布检查清单

禁止越界：
- 不改业务主链路字段
- 不私自扩展 MVP scope

## 5. 并行开发顺序

### 第一批，可立即并行

- Agent A：`M1-E1-I1`、`M1-E1-I2`
- Agent B：`M1-E2-I2`
- Agent C：`M1-E2-I1`、`M1-E2-I4`

### 第二批，依赖第一批完成

- Agent A：`M1-E1-I3`、`M1-E1-I4`
- Agent B：`M1-E2-I3`、`M1-E3-I2`
- Agent C：`M1-E3-I1`

### 第三批，形成最小闭环

- Agent C：`M1-E3-I3`、`M1-E3-I4`
- Agent D：开始 `M4-E1-I1` 的响应包络与基础 server 骨架
- Agent E：开始测试夹具与日志字段规范

### 第四批，进入生命周期与恢复

- Agent A：`M2-E1-I1`
- Agent B：`M2-E1-I3`
- Agent C：`M2-E2-I1`、`M2-E2-I2`、`M2-E2-I4`
- Agent E：`M2-E3-I3`

## 6. 每个 agent 的完成定义

每个开发任务至少满足以下条件才允许交付：

1. 只修改自己负责的文件范围，或在说明中明确新增文件。
2. 代码与 `docs/mvp-design-and-roadmap.md`、`docs/mvp-openapi.yaml`、`docs/mvp-schema.sql` 一致。
3. 补齐最小测试，不允许“代码先合，测试后补”。
4. 失败路径可观测，至少要有错误返回、状态记录或日志。
5. 最终说明中必须列出：
   - 改了哪些文件
   - 依赖哪些前置任务
   - 还有哪些已知未完成项

## 7. Agent 交付模板

建议你给每个 agent 的任务提示都固定包含以下内容：

```text
你负责 Seahorse MVP 的 <任务名称>。

必须先阅读：
1. docs/mvp-design-and-roadmap.md
2. docs/mvp-issue-breakdown.md
3. docs/mvp-openapi.yaml
4. docs/mvp-schema.sql
5. docs/mvp-config.example.toml
6. docs/mvp-agent-handoff.md

你的唯一负责范围：
<文件路径或目录>

对应 issue：
<issue 列表>

要求：
1. 不要修改不属于你负责范围的文件。
2. 不要实现 MVP 非目标能力。
3. 如果发现契约冲突，先记录并停止，不要自行发明新契约。
4. 交付时列出修改文件、测试结果、剩余风险。
```

## 8. 当前最值得先发给 agents 的任务

若你现在就要发任务，建议优先顺序如下：

1. Agent A：建 `migrations/0001_init.sql` 和 storage repository
2. Agent B：建 `EmbeddingProvider` 与 index adapter 骨架
3. Agent C：建 ingest/recall pipeline 骨架
4. Agent D：建 HTTP server、统一响应包络和接口空实现
5. Agent E：建日志、metrics、集成测试夹具

## 9. 不要交给 agents 自由发挥的事项

以下内容不要让 agents 自己决定，否则很容易漂移：

- 新增 API
- 新增数据库表
- 修改状态枚举
- 改动响应包络
- 把 namespace 从预留字段升级成完整多租户
- 引入 Tide、LIF、connectome、MCP、SDK

这些改动都应先更新设计文档，再发开发任务。
