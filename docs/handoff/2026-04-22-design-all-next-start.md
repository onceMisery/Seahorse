# Seahorse Design.All 交接与下次启动指引（2026-04-22）

## 1. 当前结论

- 当前主线任务已经不是 MVP 收口，而是 `design.all` 的全景实现。
- `Thalamus / Synapse / Cerebellum / Hippocampus / Cortex bootstrap` 的最小闭环已经真实进入代码与验证，不再只是设计文档占位。
- 当前最值得继续推进的方向已经收敛为两条：
  - `Cortex` 真正持久化：从 bootstrap archive 推进到更可恢复的持久化索引路径。
  - `Cerebellum` 继续深化：从“能修”推进到“能解释 drift、能分层修复、能服务 archive 恢复”。

## 2. 这轮已完成能力

### Recall / Thalamus

- `tagmemo` 的 `SpikeAssociation` 结果已追加结构化 metadata：
  - `mode`
  - `seed_tags`
  - `matched_tags`
  - `score`
- `Thalamus` 已真实接入 recall 主链路：
  - 输出 `worldview`
  - 输出 `entropy`
  - 输出 `focus_terms`
  - 输出 `association_allowed / association_reason`
  - 输出 `weak_signal_allowed / weak_signal_reason`
- `tagmemo` 联想扩散不再是无条件执行，已受 `Thalamus` 最小 gate 控制。
- `retrieval_log.params_snapshot` 已记录上述 recall route 决策与 focus 信息。

### Metrics / Observability

- `/metrics` 已暴露 recent recall telemetry：
  - worldview 分布
  - entropy 平均值
  - spike depth 平均值
  - emergent 总量
  - `Vector / SpikeAssociation` 来源结果总量
  - association gate allowed / blocked 统计
- `/metrics` 已暴露 connectome topology：
  - `connectome_edge_count`
  - `connectome_density`
- `/metrics` 已暴露 connectome drift：
  - `expected_edge_count`
  - missing edge 数
  - stale edge 数
  - `cooccur_mismatch` 数
  - `weight_mismatch` 数

### Cerebellum / Repair / Recovery

- forget 后已自动排入 `connectome_rebuild`。
- repair worker 已能消费 `connectome_rebuild`。
- 启动恢复时已不只检测“connectome 为空”：
  - 也能检测 `cooccur_count / weight` 漂移。
- connectome 健康快照已统一收敛到 repository：
  - `expected_edge_count`
  - `actual_edge_count`
  - `missing_edge_count`
  - `stale_edge_count`
  - `cooccur_mismatch_count`
  - `weight_mismatch_count`
  - `expected_cooccur_total`
  - `actual_cooccur_total`

## 3. 关键提交记录

- `79b7ad1` `feat(recall): add spike association metadata`
- `6b2545f` `test(config): isolate runtime env overrides`
- `ec22dc3` `feat(thalamus): gate tagmemo association routes`
- `0f77c00` `feat(metrics): expose recall telemetry signals`
- `0fa7452` `feat(cerebellum): recover missing connectome on startup`
- `15944ef` `docs: refresh design-all implementation status`
- `09667a4` `feat(metrics): expose connectome topology gauges`
- `c2ab9aa` `feat(cerebellum): validate connectome drift health`
- `3822d68` `feat(thalamus): surface recall focus terms`
- `1ecfd35` `docs: update design-all thalamus and drift status`
- `2dc39aa` `feat(thalamus): expose weak-signal route metadata`

## 4. 已验证证据

- `cargo test -p seahorse-core --lib -- --nocapture`
- `cargo test -p seahorse-server -- --nocapture`
- `powershell -File scripts/check-mvp-docs.ps1`

这些验证在最近几轮功能提交前都已反复执行，当前主干应视为可继续迭代的稳定基线。

## 5. 当前工作区状态

- 当前未提交的仓库内变更：无功能代码变更。
- 当前无关脏文件：
  - `.idea/`
  - `AGENTS.md`
- 不要把它们混入功能提交。

## 6. 当前设计边界

- 不把 `design.all` 实验能力写进 `docs/mvp-openapi.yaml`。
- 继续坚持 `namespace TEXT`，不要在这一步引入 `namespace_id` 全量重构。
- `storage/*` 继续作为 `Hippocampus` 的事实底座，不重写平行 repository。
- `tagmemo` 仍然只是最小联想召回，不要误写成完整 LIF engine。
- `weak_signal` 目前只暴露 route metadata，不代表 `Tide` 已落地。

## 7. 下一步优先级

### 优先级 1：Cortex 持久化继续落地

- 目标：
  - 推进 archive versioning
  - 为从 archive + SQLite 恢复到可服务状态打通更真实的路径
  - 继续逼近 `Cortex` 持久化而不是长期停留在 bootstrap facade
- 建议先做：
  - archive 元数据增强
  - 启动时 archive 恢复边界与失败降级
  - 对应 metrics / health 可观测字段

### 优先级 2：Cerebellum 修复策略继续深化

- 目标：
  - 不只“发现 drift 就全量 rebuild”，而是为后续分层修复留接口
  - 为 archive 恢复后的拓扑一致性修复打基础
- 建议先做：
  - connectome drift reason 细化
  - 更明确的 repair payload
  - 更细粒度的 metrics / log 字段

### 优先级 3：Thalamus recall plan 继续成形

- 目标：
  - 从 `focus + gate` 继续推进到更完整的 recall plan
  - 为未来 `Tide / WeakSignal` 提供稳定入口
- 建议先做：
  - 统一 `RecallPlan` 结构
  - 将 focus / worldview / entropy / weak-signal route 显式归一
  - 保持不扩 OpenAPI 契约

## 8. 下次启动建议

1. `git status --short`
2. 确认只有无关脏文件：
   - `.idea/`
   - `AGENTS.md`
3. 从 `docs/design-all.md` 和本文档一起恢复上下文
4. 继续按“每个功能单独提交”的方式推进
5. 每次提交前固定执行：
   - `cargo test -p seahorse-core --lib -- --nocapture`
   - `cargo test -p seahorse-server -- --nocapture`
   - `powershell -File scripts/check-mvp-docs.ps1`

## 9. 建议重点文件

- 设计文档：
  - `docs/design-all.md`
  - `docs/handoff/2026-04-22-design-all-next-start.md`
- recall / thalamus：
  - `crates/seahorse-core/src/thalamus/mod.rs`
  - `crates/seahorse-core/src/pipeline/recall.rs`
  - `crates/seahorse-server/src/handlers/recall.rs`
  - `crates/seahorse-server/src/api/mod.rs`
- repair / recovery：
  - `crates/seahorse-core/src/storage/repository.rs`
  - `crates/seahorse-server/src/state/mod.rs`
  - `crates/seahorse-server/tests/fault_recovery.rs`
- metrics：
  - `crates/seahorse-server/src/handlers/metrics.rs`
  - `crates/seahorse-server/src/app.rs`
