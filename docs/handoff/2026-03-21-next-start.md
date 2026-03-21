# Seahorse 开发交接与下次启动指引（2026-03-21）

## 1. 当前结论（可直接用于恢复开发）

- 代码已进入可联调阶段：`ingest / recall / forget / rebuild / jobs / health / stats / metrics` 路由与主链路均已实现。
- 已补齐 `StubEmbeddingProvider` 的错误映射与 `InMemoryVectorIndex::insert` 原子语义（已提交到 main）。
- metrics 配置开关与路径配置已在 worktree 完成（尚未合并到 main）。
- 运行与发布文档仍需补齐 metrics 指标说明、告警建议与发布清单更新。
- 本地 Rust toolchain 未配置，未运行编译/测试。

## 2. 已完成（main 分支）

**已提交**
- `51171e2`：修复 stub provider 错误映射能力 + 索引插入原子语义。

**当前 main 未提交文件**
- `docs/mvp-release-checklist.md`（已添加未提交）
- `docs/superpowers/plans/2026-03-20-metrics-config-plan.md`（已添加未提交）
- `docs/superpowers/specs/2026-03-20-metrics-config-design.md`（已添加未提交）
- 未跟踪：`.idea/`、`.spec-workflow/`

## 3. 待合并（worktree: metrics-config）

**分支/commit**
- worktree：`D:\code\ai\Seahorse\.worktrees\metrics-config`
- commit：`06061bd`（`feat: add metrics config loading`）

**改动内容**
- 新增 `crates/seahorse-server/src/config.rs`：读取 `./config/seahorse.toml` 的 `[observability]`，支持 `enable_metrics` 与 `metrics_path`（空值回退 `/metrics`，无前导 `/` 自动补齐）。
- `main.rs` 改为加载配置，并按开关注册 `/metrics` 路由。
- `crates/seahorse-server/Cargo.toml` 新增 `toml = "0.8"` 依赖。

**合并建议（任选其一）**
1. 直接在 main 上 cherry-pick：  
   `git cherry-pick 06061bd`
2. 切到分支合并：  
   `git merge metrics-config`

## 4. 待办事项（下一批任务）

**任务 A（代码）**
1. 把 metrics 配置改动合并到 main（见第 3 节）。
2. 合并后做一次 `git diff --check` 静态校验。
3. Rust toolchain 就绪后再补 `cargo build` 验证。

**任务 B（文档）**
1. 更新运行手册：补 metrics 指标说明 + 告警建议。
2. 更新发布检查清单：补 metrics/告警条目与验证步骤。

## 5. 关键约束与决定

- MVP 阶段不新增测试（已确认）。
- metrics 通过配置文件 `./config/seahorse.toml` 开关控制，不用环境变量。
- 配置文件不存在走默认值；配置解析失败直接 panic。
- `/metrics` Content-Type 使用 `text/plain; version=0.0.4`（Prometheus）。

## 6. 风险与限制

- 未执行 `cargo build` / `cargo test`（本机 Rust toolchain 未配置）。
- 仍存在文档缺口：metrics 指标说明与告警规则未补齐。

## 7. 下次启动步骤（重装系统后）

1. 重新克隆仓库：
   - `git clone <repo> D:\code\ai\Seahorse`
2. 检查 main 分支最新状态：
   - `git status -sb`
3. 处理 main 未提交文档：
   - 如需保留：`git add docs/mvp-release-checklist.md docs/superpowers/plans/... docs/superpowers/specs/...` 并提交
   - 如不需要：删除或恢复
4. 合并 metrics-config 分支改动（见第 3 节）。
5. 补齐文档（见第 4 节）。
6. 安装 Rust toolchain，执行：
   - `cargo build`（先验证编译）
   - 可选 `cargo test`（等测试策略确认后再跑）

## 8. 常用命令速查

- 查看主干状态：`git status -sb`
- 查看最近提交：`git log --oneline -n 10`
- 合并 metrics-config：`git cherry-pick 06061bd`
- 静态校验：  
  `git diff --check -- crates/seahorse-server/src/config.rs crates/seahorse-server/src/main.rs crates/seahorse-server/Cargo.toml`

## 9. 关键文件清单

- 配置样例：`docs/mvp-config.example.toml`
- 运行手册：`docs/runbooks/mvp-deploy-backup-rollback.md`
- 发布检查：`docs/mvp-release-checklist.md`
- metrics 相关：
  - `crates/seahorse-server/src/handlers/metrics.rs`
  - `crates/seahorse-server/src/api/observability.rs`
  - `crates/seahorse-server/src/config.rs`（待合并）

