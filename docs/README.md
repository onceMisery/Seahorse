# Seahorse Docs Index

## 可发布版 MVP 必读顺序

1. `design-all.md`
2. `mvp-design-and-roadmap.md`
3. `mvp-openapi.yaml`
4. `mvp-release-checklist.md`
5. `runbooks/mvp-deploy-backup-rollback.md`
6. `reports/2026-03-26-mvp-release-readiness.md`
7. `handoff/2026-04-22-mvp-release-handoff.md`
8. `runbooks/mvp-release-execution-checklist.md`
9. `runbooks/mvp-logging-validation-record-template.md`

## 其他文档

- `design-all.md`
  长期设计背景与范围边界，帮助理解为什么 MVP 只收口 SQLite + vector recall + REST API。
- `mvp-design-and-roadmap.md`
  MVP 设计基线，定义状态机、主链路、验收门槛与 release gates。
- `mvp-openapi.yaml`
  当前正式 API 契约。
- `mvp-release-checklist.md`
  发布前核对项、已关闭的 release blocker 证据、剩余待收口事项。
- `runbooks/mvp-deploy-backup-rollback.md`
  部署、备份、回滚、rebuild、health / stats / metrics 巡检手册。
- `runbooks/mvp-alert-rules.example.yaml`
  Prometheus 告警规则样例，覆盖 health failed、repair deadletter、repair backlog stall、rebuild stall、HTTP error rate。
- `runbooks/mvp-release-execution-checklist.md`
  release 当天可直接照跑的执行清单，覆盖启动、探针、人工链路、告警验证、perf gate 与证据归档。
- `runbooks/mvp-logging-validation-record-template.md`
  日志采集、落库、检索链路验证模板，用于关闭结构化日志剩余的外部验证 blocker。
- `reports/2026-03-26-mvp-release-readiness.md`
  当前 release readiness 证据汇总，包括测试命令、通过情况与性能基线结果。
- `handoff/2026-04-22-mvp-release-handoff.md`
  当前发布候选状态、已完成能力、验证证据与剩余外部 blocker 的最终交接摘要。
- `mvp-issue-breakdown.md`
  MVP issue 拆解和历史工作分批信息。
- `mvp-config.example.toml`
  配置示例。
