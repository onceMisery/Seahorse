# Design-All Phase 1 Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在现有 MVP 基线上落地 `design-all` 的 Phase 1 foundation/bootstrap，为后续完整的 `Cortex HNSW + mmap` 与 `Hippocampus` 演进建立可运行地基。

**Architecture:** 这一阶段明确不是完整 Phase 1 终态，而是其可交付 bootstrap。范围先聚焦四件事：新增 `embedding_cache` 存储与 ingest 复用路径、为 recall 增加 `retrieval_log` 审计记录、在 `seahorse-core` 中建立 `cortex` / `synapse` / `thalamus` / `hippocampus` / `cerebellum` 的最小骨架与统一引擎 facade、为后续 `Cortex HNSW + mmap` 提供最小可运行接口与验证基线。

**Tech Stack:** Rust 2021, rusqlite, Axum workspace, SQLite migrations, cargo test

---

## File Map

- Create: `migrations/0003_design_all_phase1.sql`
- Create: `crates/seahorse-core/src/cortex/mod.rs`
- Create: `crates/seahorse-core/src/cortex/hnsw.rs`
- Create: `crates/seahorse-core/src/cortex/archive.rs`
- Create: `crates/seahorse-core/src/synapse/mod.rs`
- Create: `crates/seahorse-core/src/thalamus/mod.rs`
- Create: `crates/seahorse-core/src/hippocampus/mod.rs`
- Create: `crates/seahorse-core/src/cerebellum/mod.rs`
- Create: `crates/seahorse-core/src/engine.rs`
- Modify: `crates/seahorse-core/src/lib.rs`
- Modify: `crates/seahorse-core/src/storage/models.rs`
- Modify: `crates/seahorse-core/src/storage/repository.rs`
- Modify: `crates/seahorse-core/src/storage/mod.rs`
- Modify: `crates/seahorse-core/src/pipeline/ingest.rs`
- Modify: `crates/seahorse-core/src/pipeline/recall.rs`
- Modify: `crates/seahorse-server/src/state/mod.rs`
- Test: `crates/seahorse-core/src/storage/repository.rs`
- Test: `crates/seahorse-core/src/pipeline/ingest.rs`
- Test: `crates/seahorse-core/src/pipeline/recall.rs`
- Test: `crates/seahorse-core/src/lib.rs`
- Test: `crates/seahorse-core/src/cortex/hnsw.rs`
- Test: `crates/seahorse-core/src/cortex/archive.rs`

### Task 1: 迁移与存储模型扩展

**Files:**
- Create: `migrations/0003_design_all_phase1.sql`
- Modify: `crates/seahorse-core/src/storage/models.rs`
- Modify: `crates/seahorse-core/src/storage/repository.rs`
- Modify: `crates/seahorse-core/src/storage/mod.rs`
- Test: `crates/seahorse-core/src/storage/repository.rs`

- [x] **Step 1: 写失败测试，覆盖新表与读写接口**
- [x] **Step 2: 运行单测确认失败**
- [x] **Step 3: 实现 migration、models 与 repository 最小代码**
- [x] **Step 4: 运行 repository 相关测试**
- [x] **Step 5: 提交**

Commit: `9bd3955` `feat(storage): add design-all phase1 persistence`

### Task 2: ingest 接入 embedding cache

**Files:**
- Modify: `crates/seahorse-core/src/pipeline/ingest.rs`
- Modify: `crates/seahorse-core/src/storage/repository.rs`
- Test: `crates/seahorse-core/src/pipeline/ingest.rs`

- [x] **Step 1: 写失败测试，证明重复 chunk 会命中缓存**
- [x] **Step 2: 运行单测确认失败**
- [x] **Step 3: 实现最小缓存接入**

实现要求：
- ingest 在批量 embedding 前先按 `namespace + content_hash + model_id` 查缓存
- 未命中项走 provider
- 新生成 embedding 回写缓存
- 保持现有 `InMemoryVectorIndex` 接口不变

- [x] **Step 4: 运行 ingest 测试**
- [x] **Step 5: 提交**

Commit: `f1c0880` `feat(ingest): cache embeddings during design-all phase1`

### Task 3: recall 接入 retrieval log

**Files:**
- Modify: `crates/seahorse-core/src/pipeline/recall.rs`
- Modify: `crates/seahorse-core/src/pipeline/forget.rs`
- Modify: `crates/seahorse-server/src/state/mod.rs`
- Test: `crates/seahorse-core/src/pipeline/recall.rs`

- [x] **Step 1: 写失败测试，证明 recall 会产生日志**
- [x] **Step 2: 运行单测确认失败**
- [x] **Step 3: 实现 recall 日志记录**

记录字段至少包括：
- namespace
- query_text
- query_hash
- mode=`basic`
- result_count
- total_time_us
- params_snapshot

- [x] **Step 4: 运行 recall/forget 相关测试**
- [x] **Step 5: 提交**

Commit: `c0e8b1a` `feat(recall): log retrieval metadata for design-all phase1`

### Task 4: 全景架构骨架与统一 facade

**Files:**
- Create: `crates/seahorse-core/src/cortex/mod.rs`
- Create: `crates/seahorse-core/src/synapse/mod.rs`
- Create: `crates/seahorse-core/src/thalamus/mod.rs`
- Create: `crates/seahorse-core/src/hippocampus/mod.rs`
- Create: `crates/seahorse-core/src/cerebellum/mod.rs`
- Create: `crates/seahorse-core/src/engine.rs`
- Modify: `crates/seahorse-core/src/lib.rs`
- Test: `crates/seahorse-core/src/lib.rs`

- [x] **Step 1: 写失败测试，证明 facade 可构建并导出**
- [x] **Step 2: 运行单测确认失败**
- [x] **Step 3: 实现最小架构骨架**

实现要求：
- 每个模块提供最小 config/state/type 占位
- `Hippocampus` 作为对 `storage/*` 的门面包装，而不是重复实现 repository
- `Cerebellum` 作为后台任务门面占位
- `SeahorseEngine` 组合五个脑区模块
- 不改现有 server API 合约

- [x] **Step 4: 运行 lib 测试与 workspace 编译**
- [x] **Step 5: 提交**

Commit: `1fdab0d` `feat(core): add design-all phase1 architecture skeleton`

### Task 5: Cortex 最小 HNSW facade 与 mmap archive 基线

**Files:**
- Create: `crates/seahorse-core/src/cortex/hnsw.rs`
- Create: `crates/seahorse-core/src/cortex/archive.rs`
- Modify: `crates/seahorse-core/src/cortex/mod.rs`
- Modify: `crates/seahorse-core/src/lib.rs`
- Test: `crates/seahorse-core/src/cortex/hnsw.rs`
- Test: `crates/seahorse-core/src/cortex/archive.rs`

- [x] **Step 1: 写失败测试，覆盖最小插入/查询与快照恢复**

新增测试：
- `searches_inserted_vectors_through_cortex_hnsw`
- `round_trips_cortex_archive_snapshot`

- [x] **Step 2: 运行单测确认失败**
- [x] **Step 3: 实现最小可运行 Cortex**

实现要求：
- `Cortex` 提供最小插入/查询 facade
- 第一版允许使用现有 `InMemoryVectorIndex` 作为 bootstrap 后端
- `archive` 提供最小快照序列化/恢复接口，为后续真正 mmap 落地预留稳定 API

- [x] **Step 4: 运行 Cortex 测试**
- [x] **Step 5: 提交**

Commit: `5f95eb5` `feat(cortex): add design-all phase1 cortex foundation`

### Task 6: mmap/recovery 降级路径验证

**Files:**
- Modify: `crates/seahorse-core/src/cortex/archive.rs`
- Modify: `crates/seahorse-core/src/cortex/mod.rs`
- Test: `crates/seahorse-core/src/cortex/archive.rs`

- [x] **Step 1: 写失败测试，覆盖损坏快照降级**

新增测试：
- `rejects_corrupted_cortex_archive_snapshot`

- [x] **Step 2: 运行单测确认失败**
- [x] **Step 3: 实现最小降级语义**

实现要求：
- archive 读取时校验头信息与维度
- 损坏或不匹配时返回明确错误
- 为后续 repair/recovery 留出错误边界

- [x] **Step 4: 运行相关测试**
- [x] **Step 5: 提交**

Commit: `a13eea8` `feat(cortex): validate archive recovery boundaries`

### Task 7: 最终验证

**Files:**
- Verify only

- [x] **Step 1: 运行格式化**

Run: `cargo fmt --all`
Expected: exit 0

- [x] **Step 2: 运行核心测试**

Run: `cargo test -p seahorse-core --lib -- --nocapture`
Expected: PASS

- [x] **Step 3: 运行服务端测试**

Run: `cargo test -p seahorse-server -- --nocapture`
Expected: PASS

- [x] **Step 4: 运行文档校验**

Run: `powershell -File scripts/check-mvp-docs.ps1`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add .
git commit -m "feat: land design-all phase1 foundation"
```

## Scope Guardrails

- 本阶段不引入 `namespace_id` 全量迁移，只继续使用现有 `namespace TEXT` MVP 约束。
- 本阶段不实现 `connectome`、`neuron_states`、`tagmemo`、`tide`、`dream` 的完整行为，只留出稳定接口骨架。
- 本阶段不得修改现有 REST 契约与已有 MVP 语义，新增能力必须是增量兼容的。
- `storage/*` 继续作为 `Hippocampus` 的内部实现来源；新增 `hippocampus/*` 只做门面与边界收敛，不在本阶段重复实现 repository。
