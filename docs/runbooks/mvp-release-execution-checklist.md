# Seahorse MVP Release 执行清单

## 1. 目的

本清单用于 release 当天执行，目标不是解释设计，而是按顺序完成上线前验证、上线动作、证据归档与失败回退。

适用前提：

- 当前仓库版本已经通过本地回归
- `docs/mvp-openapi.yaml`、`docs/mvp-release-checklist.md`、`docs/runbooks/mvp-deploy-backup-rollback.md` 与当前代码一致
- release 环境使用受信内网或本地部署

## 2. 上线前准备

1. 记录本次发布 commit：

```powershell
git rev-parse HEAD
git log --oneline -n 5
```

2. 确认工作树干净：

```powershell
git status --short
```

3. 归档本次使用的配置文件：

- `./config/seahorse.toml`
- 若使用默认配置，也要在发布记录里注明“default config”

4. 重新执行文档与测试基线：

```powershell
cargo fmt --check
cargo test -p seahorse-core --lib -- --nocapture
cargo test -p seahorse-server -- --nocapture
powershell -File scripts/check-mvp-docs.ps1
```

## 3. 启动与基础探针

1. 启动服务：

```powershell
cargo run -p seahorse-server
```

2. 在另一个终端检查平台探针与人工巡检接口：

```powershell
curl http://127.0.0.1:8080/live
curl http://127.0.0.1:8080/ready
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/stats
curl http://127.0.0.1:8080/metrics
```

验收标准：

- `/live` 返回 `200`
- `/ready` 返回 `200`
- `/health` 返回 `success=true`
- `/metrics` 返回 `text/plain; version=0.0.4`

## 4. 最小业务链路

1. ingest：

```powershell
curl -X POST http://127.0.0.1:8080/ingest `
  -H "content-type: application/json" `
  -d '{"namespace":"default","content":"release smoke alpha beta gamma","source":{"type":"inline","filename":"release-smoke.txt"},"options":{"auto_tag":true}}'
```

2. recall：

```powershell
curl -X POST http://127.0.0.1:8080/recall `
  -H "content-type: application/json" `
  -d '{"namespace":"default","query":"release smoke alpha","mode":"basic","top_k":5,"timeout_ms":5000}'
```

3. 从 ingest 响应中记录 `chunk_ids` 和 `file_id`，然后执行 forget：

```powershell
curl -X POST http://127.0.0.1:8080/forget `
  -H "content-type: application/json" `
  -d '{"namespace":"default","chunk_ids":[<chunk_id>],"mode":"soft"}'
```

4. 再次 recall，确认已不再返回该 chunk。

5. 执行 rebuild：

```powershell
curl -X POST http://127.0.0.1:8080/admin/rebuild `
  -H "content-type: application/json" `
  -d '{"namespace":"default","scope":"all","force":false}'
```

6. 从 rebuild 响应中记录 `job_id`，轮询 job：

```powershell
curl http://127.0.0.1:8080/admin/jobs/<job_id>
```

验收标准：

- rebuild 最终进入 `succeeded`
- rebuild 完成后 `/ready` 仍为 `200`
- `/stats` 中 `index_status` 处于 `ready` 或可解释的 `degraded`

## 5. Metrics 与告警验证

1. 确认 `/metrics` 至少包含以下指标：

- `seahorse_http_requests_total`
- `seahorse_http_request_errors_total`
- `seahorse_http_request_latency_ms_max`
- `seahorse_repair_queue_tasks`
- `seahorse_rebuild_jobs`
- `seahorse_repair_oldest_task_age_seconds`
- `seahorse_rebuild_oldest_active_job_age_seconds`
- `seahorse_index_state`
- `seahorse_health_status`

2. 将样例规则导入监控平台：

- `docs/runbooks/mvp-alert-rules.example.yaml`

3. 至少验证两类告警：

- `health failed` 类
- `repair/rebuild stalled` 类

4. 归档验证截图或平台事件记录。

## 6. Release 机器性能 gate

在 release 机器执行：

```powershell
cargo test -p seahorse-server perf_baseline_10k -- --ignored --nocapture
```

验收标准：

- hard gate 通过
- 若失败，记录完整输出并停止对外宣称“最终可交付发布型 MVP”

仅需记录数据点时，可用：

```powershell
$env:SEAHORSE_PERF_RECORD_ONLY="1"
cargo test -p seahorse-server perf_baseline_10k -- --ignored --nocapture
```

## 7. 发布材料归档

至少归档以下内容：

- release commit hash
- 配置文件或默认配置说明
- `/live`、`/ready`、`/health`、`/stats`、`/metrics` 响应样本
- 最小业务链路请求与响应样本
- rebuild job 最终状态记录
- 告警规则导入与验证证据
- `10k chunk` perf gate 输出

## 8. 失败回退

出现以下任一情况，按回滚手册执行，不继续发布：

- `/ready` 非 `200`
- `/health` 返回 `failed`
- rebuild 无法收敛到终态
- 告警规则导入后立即出现无法解释的 critical 告警
- `10k chunk` hard gate 未通过

回退手册：

- `docs/runbooks/mvp-deploy-backup-rollback.md`
