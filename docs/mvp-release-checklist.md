# Seahorse MVP 发布检查清单

## 1. 范围确认

- 只发布 MVP 必须项
- 不包含 Tide、LIF、connectome、MCP、SDK、多租户增强
- 当前版本仍以 SQLite + vector recall + REST API 为边界

## 2. 代码与契约

- `docs/mvp-design-and-roadmap.md` 仍可作为当前实现基线
- `docs/mvp-openapi.yaml` 已覆盖当前对外 API
- 请求/响应结构仍使用统一包络：`success/data/error/request_id`
- 未新增设计文档中未定义的状态值或外部 API

## 3. 配置与运行前置

- 已确认数据库路径
- 已确认数据目录可写
- 已确认服务仅绑定本地或受信内网地址
- 已确认当前 embedding 配置与实现一致
- 已确认 migration 文件齐全

## 4. 存储与恢复

- 已执行 SQLite 备份
- 已记录备份文件位置和时间
- 已确认回滚时使用的稳定版本
- 已准备 rebuild 操作步骤

## 5. 主链路验收

- `POST /ingest` 可调用
- `POST /recall` 可调用
- `POST /forget` 可调用
- `POST /admin/rebuild` 可调用
- `GET /admin/jobs/{job_id}` 可调用
- `GET /health` 可调用
- `GET /stats` 可调用

建议至少完成一次人工链路：

1. ingest 一条文本
2. recall 命中该文本
3. forget 后 recall 不再返回
4. rebuild 成功完成
5. rebuild 后 health / stats 正常

## 6. 状态与可恢复性

- `files.ingest_status` 状态流转符合设计
- `chunks.index_status` 状态流转符合设计
- `schema_meta.index_state` 可反映 `ready / rebuilding / degraded`
- 重启后 active rebuild job 可恢复
- 多个 active rebuild job 启动恢复时仅保留最新一条

## 7. 当前未完成项

以下项未完成时，不应将当前版本定义为“可发布版 MVP 已完成”：

- `repair_queue` 完整自动修复闭环
- 结构化请求日志完整接入
- metrics 导出与告警规则
- 自动化 contract / E2E / 故障注入测试
- `1 万 chunk` 基线性能验收

## 8. 当前可接受结论

如果以下条件成立，可接受将当前版本定义为“开发闭环版 MVP”：

- 主链路可手工跑通
- rebuild 能提交、查询、恢复
- health / stats 可用于最小人工巡检
- SQLite 备份与回滚步骤明确

如果要定义为“可发布版 MVP”，则还必须补齐：

1. repair worker 运行接入与恢复策略
2. observability 收口
3. 自动化验收
4. 性能基线与发布检查记录
