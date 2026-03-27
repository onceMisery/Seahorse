# Seahorse MVP Release Readiness

## 当前结论

- 当前版本可以视为“发布候选 MVP”
- 已关闭的关键 release blocker：runtime config、contract、E2E、fault recovery、10k chunk perf gate 实现
- 当前仍未关闭的 release blocker：结构化请求日志、监控平台告警落地、release 环境 10k chunk hard gate 最终过线

## 已验证证据

| Area | Command | Result |
| --- | --- | --- |
| seahorse-server 回归 | `cargo test -p seahorse-server -- --nocapture` | PASS |
| seahorse-core 库测试 | `cargo test -p seahorse-core --lib -- --nocapture` | PASS |
| fault recovery | `cargo test -p seahorse-server --test fault_recovery -- --nocapture` | PASS |
| perf gate smoke | `cargo test -p seahorse-server --test perf_baseline -- --nocapture` | PASS (`perf_gate_env_parsers_support_record_only_overrides`) |
| 10k chunk hard gate | `cargo test -p seahorse-server perf_baseline_10k -- --ignored --nocapture` | FAIL on current machine |

## 10k Chunk 基线样本

- 执行日期: 2026-03-27
- Git commit: `fbf58bb`
- 运行模式: hard gate
- record-only 开关: `SEAHORSE_PERF_RECORD_ONLY=1`
- p95 阈值覆盖: `SEAHORSE_PERF_RECALL_P95_MS_MAX=<ms>`

### 样本规模

- 文档数: 200
- 总 chunks: 10,000
- 每篇文档 chunks: 50
- chunk_size: 128 chars
- recall 样本数: 200
- recall `top_k`: 5
- rebuild scope: `all`

### 指标结果

| Metric | Value |
| --- | ---: |
| `ingest_total_ms` | 29942.75 |
| `recall_p50_ms` | 201.75 |
| `recall_p95_ms` | 364.01 |
| `rebuild_total_ms` | 79723.94 |
| `recall_p95_gate_ms` | 300.00 |

### 结论

- 当前本机 hard gate 未通过：`recall_p95_ms=364.01 >= 300.00`
- gate 逻辑已存在，当前数据已归档
- 本机如果只需要保留数据点，可使用 `SEAHORSE_PERF_RECORD_ONLY=1`

## 后续动作

1. 在更稳定的 release 机器上复测 10k chunk hard gate
2. 完成 operator docs 与 release handoff 文档最终收口
3. 在监控平台落地告警规则并保留验证记录