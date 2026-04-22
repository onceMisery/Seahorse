# Seahorse MVP 日志链路验证记录模板

## 1. 目的

用于验证 Seahorse 的结构化日志不只是在 stdout 输出，而是真正完成了采集、落库、检索和按 request_id 追踪。

## 2. 基本信息

- 验证日期：
- 验证环境：
- 验证人：
- 服务版本 / commit：
- 日志平台：
- 采集组件：
- 日志存储位置：

## 3. 验证前置

- Seahorse 服务已启动
- 平台已能采集 stdout JSON 日志
- 可从日志平台按字段搜索
- 本次测试使用的配置文件已归档

## 4. 验证步骤

### 4.1 生成一条带 request_id 的业务请求

记录请求：

- 方法：
- 路径：
- 请求时间：
- 关键参数摘要：

记录响应：

- HTTP 状态：
- `request_id`：
- `success`：

### 4.2 验证日志采集

检查项：

- 是否能在日志平台中搜到同一个 `request_id`
- 是否存在 `request.start`
- 是否存在 `request.end`
- 是否存在对应 handler 业务事件

记录结果：

- `request.start`：
- `request.end`：
- 业务事件：
- 首次可检索时间：

### 4.3 验证字段完整性

至少确认以下字段存在并可查询：

- `request_id`
- `event`
- `method`
- `route`
- `status`
- `latency_ms`

记录结果：

- `request_id`：
- `event`：
- `method`：
- `route`：
- `status`：
- `latency_ms`：

### 4.4 验证后台事件可检索

至少验证一种后台事件：

- rebuild 提交 / running / succeeded / failed
- repair task started / succeeded / failed

记录结果：

- 事件名称：
- 检索条件：
- 是否可见：
- 样本时间：

## 5. 验证结论

- 是否完成“采集 -> 落库 -> 检索”全链路验证：
- 是否支持按 `request_id` 追踪：
- 是否存在字段缺失或解析异常：
- 是否需要补日志平台字段映射：

## 6. 附件

- 日志平台截图：
- 查询语句：
- 对应请求与响应样本：
- 其它备注：
