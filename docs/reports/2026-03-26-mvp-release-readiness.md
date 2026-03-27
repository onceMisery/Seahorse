# Seahorse MVP Release Readiness

## 执行信息

- 执行日期: 2026-03-27
- Git commit: `fbf58bb`
- 测试命令: `cargo test -p seahorse-server perf_baseline_10k -- --ignored --nocapture`
- 运行模式: hard gate
- record-only 开关: `SEAHORSE_PERF_RECORD_ONLY=1`
- p95 阈值覆盖: `SEAHORSE_PERF_RECALL_P95_MS_MAX=<ms>`

## 样本规模

- 文档数: 200
- 总 chunks: 10,000
- 每篇文档 chunks: 50
- chunk_size: 128 chars
- recall 样本数: 200
- recall `top_k`: 5
- rebuild scope: `all`

## 指标结果

| Metric | Value |
| --- | ---: |
| `ingest_total_ms` | 29942.75 |
| `recall_p50_ms` | 201.75 |
| `recall_p95_ms` | 364.01 |
| `rebuild_total_ms` | 79723.94 |
| `recall_p95_gate_ms` | 300.00 |

## 结论

- `10k chunk` 性能 release gate: 当前本机样本未通过
- 判定依据: `recall_p95_ms=364.01 >= 300.00`
- 当前结果已作为本机基线样本记录，后续需要在更稳定的 release 环境重新跑 hard gate

## 备注

- 该测试位于 `crates/seahorse-server/tests/perf_baseline.rs`
- 测试默认启用 hard gate；本地环境如果波动过大，可临时切到 record-only 模式保留数据点
- record-only 模式的开关解析已由轻量测试覆盖，但 10k 样本的本机 record-only 长跑本次未额外固化为第二份数据
- 本次测量对应的运行时代码是 `fbf58bb test: verify repair and rebuild fault recovery`
- 下一步优化方向:
  - 在更稳定的 release 机器上复测 hard gate
  - 继续观察 recall 热路径在 10k chunk 下的 p95 波动
  - 评估是否需要单独的 release profile/perf 环境来承载最终发布门禁
