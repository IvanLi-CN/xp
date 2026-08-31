# Service Monitor API and stream contract

所有 HTTP API 都在 admin token 保护的 `/api/admin` 管理面。本文描述当前
wire contract；所有时间字段均为 UTC Unix seconds，延迟为整数毫秒。

## Common rules

- Monitor 与 ad-hoc run ID 使用 ULID。
- `PATCH` body 和 `DELETE` query 都必须提供 `expected_revision`。
- 写入沿用现有 Leader/follower 转发语义。冲突响应使用稳定
  `revision_conflict` code。
- 错误 shape 为 `{"error":{"code":string,"message":string,"details":object}}`；
  客户端只能根据 `code` 分支。
- HTTP 执行固定使用 5 秒连接超时、10 秒总超时；这些不是 API 可配置字段。

## Monitor definition

`target` 是带 `kind` 的 internally-tagged object，不是按方法名嵌套的对象：

```json
{
  "monitor_id": "01J...",
  "name": "api.example.com",
  "target": {
    "kind": "https",
    "url": "https://api.example.com/health",
    "method": "get",
    "accepted_statuses": [{ "start": 200, "end": 399 }],
    "body_contains": "ok"
  },
  "interval_seconds": 60,
  "observer_node_ids": null,
  "lifecycle": "active",
  "revision": 3,
  "revision_effective_at_unix_seconds": 1785278460
}
```

- `kind` 为 `http`、`https`、`ping` 或 `tcping`。
- HTTP/HTTPS 允许 `method` 为 `get` 或 `head`；`body_contains` 仅在 GET
  时有意义，最多 256 bytes。
- PING target 为 `{ "kind": "ping", "host": "example.com" }`。
- TCPING target 为
  `{ "kind": "tcping", "host": "example.com", "port": 443 }`。
- `observer_node_ids: null` 表示默认 Observer Set；非空数组是显式 allowlist。

## REST routes

- `GET /api/admin/monitors?lifecycle=<state>&kind=<kind>`：返回
  `{ "items": [ServiceMonitorSummary] }`。每个摘要包含定义、`status`、
  `stale`、history `quality` 以及 `recent_6h`。`recent_6h` 含
  availability、coverage、expected/executed、latest observation metadata 和
  72 个五分钟状态槽；它只汇总本节点最近六小时的 scheduled Observation，必须随
  `local_only` quality 解释，不能替代 Repository history。
- `POST /api/admin/monitors`：接受 name、target、可选 `interval_seconds`
  和可选/nullable `observer_node_ids`；创建 revision 1 并返回完整定义。
- `GET /api/admin/monitors/:id`：返回完整定义。
- `PATCH /api/admin/monitors/:id`：接受 `expected_revision` 及需替换字段，
  创建下一个 revision；仅 lifecycle 的变更也在下一个 UTC Slot 生效。
- `DELETE /api/admin/monitors/:id?expected_revision=<revision>`：写入
  `deleted` lifecycle，不执行 history purge，成功为 `204`。
- `POST /api/admin/monitors/:id/run`：创建 ad-hoc run，返回
  `202 { "run_id": "01J...", "state": "queued" }`。
- `GET /api/admin/monitor-runs/:run_id`：返回 queued、running、succeeded、
  failed 或 rejected；完成后含 Observation 或拒绝原因。
- `GET /api/admin/monitors/:id/status`：返回聚合状态、freshness、capture
  state、quality 与 Observer 状态。`icmp_supported` 只在本机已自检时出现；
  远端未知时省略，客户端必须显示 `unknown` 而非 `unsupported`。
- `GET /api/admin/monitors/:id/history`：返回 rollup 点和读取质量。支持
  `from`、`to`、`resolution`、`observer_id`、`limit`；resolution 为
  `auto`、`1m`、`5m` 或 `1h`，limit 被限制为 1 至 1,500。

history response 包含 `quality`（`complete`、`partial`、`local_only`）、
`coverage_percent`、`watermark_unix_seconds`、`gaps`、`skew_seconds`、
`freshness_seconds` 与 `truncated`。scheduled rollup 排除 `ad_hoc: true`
的 Observation。

## Structured errors

- `observation_budget_exceeded`
- `revision_conflict`
- `monitor_deleted`
- `run_rate_limited`
- `not_found`
- `invalid_request`
- `internal_error`

执行失败是 Observation 的 `error` 枚举而非 route error，包含 `dns`、
`target_blocked`、`connect_timeout`、`total_timeout`、`tls`、`http_status`、
`body_mismatch`、`redirect_blocked`、`icmp_unsupported`、`icmp_timeout` 和
`tcp_connect`。

## Signed history stream

- schema ID 为 `service_monitor_observation.v1`，stream 为
  `service_monitor_observation-v1`。
- identity 是 source node、epoch、stream、sequence；复用现有签名、cursor、
  ack 与 fork isolation。
- payload 保存 Monitor、revision、Observer、slot、ad-hoc flag、outcome、
  error、统计和 rollup metadata；不保存响应 body、headers 或 credential。
- Repository 对同一 identity 幂等接收，并以分钟、5 分钟、小时粒度聚合。

## Capability

- `admin.service-monitors` 表示管理 API 可用。
- `admin.service-monitor-http-v1` 与 `admin.service-monitor-tcp-v1` 表示
  HTTP/HTTPS 与 TCPING runtime 可用。
- `admin.service-monitor-icmp-v1` 仅在本机 Linux ICMP datagram/raw
  capability self-test 成功时出现。
