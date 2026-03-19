# Seahorse MVP 首批 Issue 清单

> 用途：这是第一轮开发的直接分发清单，只覆盖最先应该启动的工作，不覆盖全部 MVP。
>
> 基线来源：
> - `docs/mvp-design-and-roadmap.md`
> - `docs/mvp-issue-breakdown.md`
> - `docs/mvp-agent-handoff.md`
> - `docs/mvp-openapi.yaml`
> - `docs/mvp-schema.sql`

## 1. 使用规则

- 这份清单只覆盖 `M1` 和 `M2` 的前置阻塞项。
- 优先级按 `P0 > P1 > P2` 排序。
- 每个 issue 默认控制在 `1 ~ 3` 个工程日。
- 除非 issue 中显式说明，否则不得越过文件边界。
- 如果 agent 发现契约冲突，应停止开发并回报，不允许自行扩展契约。

## 2. 第一轮并行策略

### 并行泳道

- 泳道 A：Schema / Storage
- 泳道 B：Embedding / Index
- 泳道 C：Pipeline / Jobs
- 泳道 D：API / Tests

### 启动原则

- 第一轮先把“空仓库变成可承载开发的代码骨架”。
- 第二轮跑通 `ingest -> recall`。
- 第三轮补 `soft delete / repair / rebuild`。
- `M3`、`M4` 的完整能力暂不进入第一轮。

## 3. 首批 Issue 列表

| 优先级 | Issue ID | 标题                                                                 | 推荐 Agent          | 依赖                                     | 预计产出                                             |
|-----|----------|--------------------------------------------------------------------|-------------------|----------------------------------------|--------------------------------------------------|
| P0  | `FB-001` | 初始化 workspace 与 crate 骨架                                           | Agent A           | 无                                      | Rust workspace、crate 目录、基础 `Cargo.toml`          |
| P0  | `FB-002` | 建立 SQLite 初始 migration `0001_init.sql`                             | Agent A           | 无                                      | `migrations/0001_init.sql`                       |
| P0  | `FB-003` | 定义 `EmbeddingProvider` trait 与错误模型                                 | Agent B           | 无                                      | `embedding/` trait 与错误类型                         |
| P0  | `FB-004` | 实现基础 chunker 与输入预处理模块                                              | Agent C           | 无                                      | `chunker`、预处理、输入校验基础逻辑                           |
| P0  | `FB-005` | 建立 server 骨架与统一响应包络                                                | Agent D           | `FB-001`                               | HTTP server、`success/data/error/request_id` 响应结构 |
| P1  | `FB-006` | 落实 schema 索引、约束与 `schema_meta` 校验                                  | Agent A           | `FB-002`                               | 完整 schema、启动前 schema 检查                          |
| P1  | `FB-007` | 实现 repository 层与事务边界                                               | Agent A           | `FB-002`                               | `storage/` repository、事务提交逻辑                     |
| P1  | `FB-008` | 接入首个 embedding provider adapter                                    | Agent B           | `FB-003`                               | 可调用的 provider 实现                                 |
| P1  | `FB-009` | 实现向量索引 adapter 骨架                                                  | Agent B           | `FB-003`                               | index adapter，支持 insert/search 接口                |
| P1  | `FB-010` | 实现 `dedup_mode` 与 `file_hash` 逻辑                                   | Agent C           | `FB-004`, `FB-007`                     | 幂等/重复写入策略                                        |
| P1  | `FB-011` | 实现 ingest pipeline                                                 | Agent C           | `FB-007`, `FB-008`, `FB-009`, `FB-010` | 主写入链路                                            |
| P1  | `FB-012` | 实现 basic recall pipeline                                           | Agent C           | `FB-008`, `FB-009`, `FB-007`           | 主召回链路                                            |
| P1  | `FB-013` | 暴露 `POST /ingest` 与 `POST /recall`                                 | Agent D           | `FB-005`, `FB-011`, `FB-012`           | 可联调 API                                          |
| P1  | `FB-014` | 建立 `ingest -> recall` 集成测试                                         | Agent D           | `FB-013`                               | integration / E2E 基线测试                           |
| P2  | `FB-015` | 实现 soft delete 数据落库                                                | Agent A           | `FB-007`                               | `forget` 所需存储变更                                  |
| P2  | `FB-016` | recall 过滤 tombstone 与索引可见性标记                                       | Agent B + Agent C | `FB-012`, `FB-015`                     | 删除后不继续召回                                         |
| P2  | `FB-017` | 实现 `repair_queue` worker                                           | Agent C           | `FB-011`, `FB-007`                     | repair 状态流转                                      |
| P2  | `FB-018` | 实现 `maintenance_jobs` 与 rebuild job manager                        | Agent C           | `FB-007`                               | rebuild 作业管理                                     |
| P2  | `FB-019` | 实现 `POST /forget`、`POST /admin/rebuild`、`GET /admin/jobs/{job_id}` | Agent D           | `FB-015`, `FB-017`, `FB-018`, `FB-005` | 生命周期 API                                         |
| P2  | `FB-020` | 健康检查与 `GET /health`、`GET /stats`                                   | Agent D           | `FB-018`                               | health/stats API                                 |

## 4. 每个 Issue 的直接说明

### `FB-001` 初始化 workspace 与 crate 骨架

- 优先级：`P0`
- 目标：
  建立 Rust workspace、`seahorse-core`、`seahorse-server`、`seahorse-cli` 三个 crate 的最小可编译骨架。
- 文件范围：
    - `Cargo.toml`
    - `crates/seahorse-core/**`
    - `crates/seahorse-server/**`
    - `crates/seahorse-cli/**`
- 验收标准：
    - workspace 结构与 `docs/mvp-agent-handoff.md` 一致
    - 各 crate 至少有最小 `lib.rs` 或 `main.rs`
    - 不实现任何业务逻辑

### `FB-002` 建立 SQLite 初始 migration

- 优先级：`P0`
- 目标：
  将 `docs/mvp-schema.sql` 落成第一版 migration 文件。
- 文件范围：
    - `migrations/0001_init.sql`
- 验收标准：
    - 包含 `files/chunks/tags/chunk_tags/repair_queue/maintenance_jobs/schema_meta`
    - 状态字段、索引、唯一约束与文档一致
    - 可重复执行或可明确作为一次性初始化脚本使用

### `FB-003` 定义 `EmbeddingProvider` trait 与错误模型

- 优先级：`P0`
- 目标：
  先冻结 embedding 接口，避免后续 pipeline 和 index 并行开发时出现接口漂移。
- 文件范围：
    - `crates/seahorse-core/src/embedding/**`
    - `crates/seahorse-core/src/types/**`
- 验收标准：
    - 包含 `embed`、`embed_batch`、`model_id`、`dimension`、`max_batch_size`
    - 定义 provider timeout / provider failure / dimension mismatch 的错误类型

### `FB-004` 实现基础 chunker 与输入预处理模块

- 优先级：`P0`
- 目标：
  先实现可稳定复现的 chunk 与预处理逻辑，供 ingest pipeline 使用。
- 文件范围：
    - `crates/seahorse-core/src/pipeline/**`
    - `crates/seahorse-core/src/types/**`
- 验收标准：
    - 相同输入产生稳定 chunk 序列
    - 支持基础换行清洗、控制字符清理、`content_hash` 计算
    - 输入约束对齐 `mvp-design-and-roadmap.md`

### `FB-005` 建立 server 骨架与统一响应包络

- 优先级：`P0`
- 目标：
  先冻结 HTTP 层壳子和通用响应结构，让后续 handler 并行开发不冲突。
- 文件范围：
    - `crates/seahorse-server/src/**`
- 验收标准：
    - `success/data/error/request_id` 已有统一定义
    - 路由可注册但允许业务逻辑先留空
    - 不偏离 `docs/mvp-openapi.yaml`

### `FB-006` 落实 schema 索引、约束与 `schema_meta` 校验

- 优先级：`P1`
- 依赖：`FB-002`
- 验收标准：
    - 启动时能校验 `schema_version/index_version/embedding_model_id/embedding_dimension`
    - schema 不一致时返回明确错误

### `FB-007` 实现 repository 层与事务边界

- 优先级：`P1`
- 依赖：`FB-002`
- 文件范围：
    - `crates/seahorse-core/src/storage/**`
- 验收标准：
    - 能在单事务里写入 `files/chunks/tags/chunk_tags`
    - 不把索引更新混进 SQLite 事务

### `FB-008` 接入首个 embedding provider adapter

- 优先级：`P1`
- 依赖：`FB-003`
- 验收标准：
    - 至少有一个 stub/local provider 可跑通
    - 能返回 `model_id` 与 `dimension`

### `FB-009` 实现向量索引 adapter 骨架

- 优先级：`P1`
- 依赖：`FB-003`
- 验收标准：
    - 定义 `insert/search/mark_deleted/rebuild` 级别接口
    - 允许先用最小实现，但接口不能再漂移

### `FB-010` 实现 `dedup_mode` 与 `file_hash` 逻辑

- 优先级：`P1`
- 依赖：`FB-004`, `FB-007`
- 验收标准：
    - 支持 `reject / upsert / allow`
    - 行为与 `docs/mvp-design-and-roadmap.md` 一致

### `FB-011` 实现 ingest pipeline

- 优先级：`P1`
- 依赖：`FB-007`, `FB-008`, `FB-009`, `FB-010`
- 验收标准：
    - 主链路：`input -> preprocess -> chunk -> embed -> tx write -> index update`
    - 索引失败进入 `repair_queue`
    - `files.ingest_status`、`chunks.index_status` 更新正确

### `FB-012` 实现 basic recall pipeline

- 优先级：`P1`
- 依赖：`FB-008`, `FB-009`, `FB-007`
- 验收标准：
    - 支持 query embedding、vector top-k、回表组装、去重与过滤
    - `source_type = Vector`

### `FB-013` 暴露 `POST /ingest` 与 `POST /recall`

- 优先级：`P1`
- 依赖：`FB-005`, `FB-011`, `FB-012`
- 验收标准：
    - 请求响应字段与 OpenAPI 对齐
    - 错误码映射与统一包络生效

### `FB-014` 建立 `ingest -> recall` 集成测试

- 优先级：`P1`
- 依赖：`FB-013`
- 验收标准：
    - 自动化测试覆盖成功链路
    - 至少包含一次非法输入校验

### `FB-015` 实现 soft delete 数据落库

- 优先级：`P2`
- 依赖：`FB-007`
- 验收标准：
    - `is_deleted/deleted_at` 正确写入
    - 不直接触发全量 rebuild

### `FB-016` recall 过滤 tombstone 与索引可见性标记

- 优先级：`P2`
- 依赖：`FB-012`, `FB-015`
- 验收标准：
    - 删除后 recall 不再返回对应 chunk
    - 索引层支持最小可见性控制

### `FB-017` 实现 `repair_queue` worker

- 优先级：`P2`
- 依赖：`FB-011`, `FB-007`
- 验收标准：
    - 状态流转：`pending/running/succeeded/failed/deadletter`
    - 有重试计数和错误记录

### `FB-018` 实现 `maintenance_jobs` 与 rebuild job manager

- 优先级：`P2`
- 依赖：`FB-007`
- 验收标准：
    - rebuild job 可创建、查询状态、限制并发
    - 状态值与 `docs/mvp-schema.sql`、`docs/mvp-openapi.yaml` 一致

### `FB-019` 实现生命周期 API

- 优先级：`P2`
- 依赖：`FB-015`, `FB-017`, `FB-018`, `FB-005`
- 验收标准：
    - `POST /forget`
    - `POST /admin/rebuild`
    - `GET /admin/jobs/{job_id}`

### `FB-020` 健康检查与基础统计

- 优先级：`P2`
- 依赖：`FB-018`
- 验收标准：
    - `GET /health`
    - `GET /stats`
    - 能区分 `ok/degraded/failed`

## 5. 建议的实际发单顺序

### 第一批，今天就能发

1. `FB-001`
2. `FB-002`
3. `FB-003`
4. `FB-004`
5. `FB-005`

### 第二批，第一批落地后立即发

1. `FB-006`
2. `FB-007`
3. `FB-008`
4. `FB-009`
5. `FB-010`

### 第三批，形成最小业务闭环

1. `FB-011`
2. `FB-012`
3. `FB-013`
4. `FB-014`

### 第四批，补生命周期闭环

1. `FB-015`
2. `FB-016`
3. `FB-017`
4. `FB-018`
5. `FB-019`
6. `FB-020`

## 6. 哪些是绝对阻塞项

如果你想“尽快看到系统跑起来”，以下 issue 是绝对阻塞项：

- `FB-001`
- `FB-002`
- `FB-003`
- `FB-004`
- `FB-007`
- `FB-008`
- `FB-009`
- `FB-011`
- `FB-012`
- `FB-013`
- `FB-014`

没有这些，第一轮无法形成可运行的 `ingest -> recall`。

## 7. 哪些可以晚一点

以下 issue 可以在看到第一轮闭环后再发：

- `FB-015`
- `FB-016`
- `FB-017`
- `FB-018`
- `FB-019`
- `FB-020`

这些属于生命周期闭环和运维闭环，不阻塞第一轮看到 MVP 主链路。
