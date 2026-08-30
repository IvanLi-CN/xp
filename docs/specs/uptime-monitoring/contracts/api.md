# Service Monitor API and stream contract

所有 HTTP API 都在 admin token 保护的 `/api/admin` 管理面。

## Common rules

- 时间为 UTC RFC 3339；duration 为整数毫秒；latency 为整数微秒。
- ID 使用 ULID。除创建外，写请求必须传 `expected_revision`。
- 失败 shape 是 `{"error":{"code":string,"message":string,"details":object}}`。
- `code` 是稳定枚举；客户端不能根据 message 分支。
- list/history 使用 opaque cursor，服务端限制 limit、时间范围与总点数。
- 读响应含 quality：complete/partial/local_only、coverage、watermark、gaps、skew、as_of。

## Monitor definition

HTTP/HTTPS target：

```json
{
  "id": "01J...",
  "name": "api.example.com",
  "lifecycle": "active",
  "revision": 3,
  "kind": "https",
  "target": { "url": "https://api.example.com/health" },
  "schedule": { "interval_seconds": 60, "effective_at": "2026-08-30T08:00:00Z" },
  "timeout": { "connect_ms": 5000, "total_ms": 10000 },
  "observer_policy": { "mode": "all_capable", "node_ids": [] },
  "http": { "method": "GET", "status_ranges": [[200, 399]] }
}
```

PING target 为 `{ "host": string }`。TCPING target 为
`{ "host": string, "port": 1..65535 }`。body_contains 仅用于 HTTP 执行配置，
最多 256 bytes。

## REST routes

- `GET /api/admin/monitors`：按 lifecycle、kind、status、quality 分页列出摘要。
- `POST /api/admin/monitors`：创建 revision 1，校验公网目标、周期、timeout
  与预算，返回 effective_at。
- `GET /api/admin/monitors/:id`：返回定义、capability summary 与预算。
- `PATCH /api/admin/monitors/:id`：新 revision 或 lifecycle 变更，
  返回 next effective slot。
- `DELETE /api/admin/monitors/:id`：写 deleted tombstone；不接受 purge。
- `POST /api/admin/monitors/:id/run`：创建 ad-hoc run，返回 202 和 run_id。
- `GET /api/admin/monitor-runs/:run_id`：返回 queued、running、succeeded、
  failed 或 rejected。
- `GET /api/admin/monitors/:id/status`：返回聚合状态、freshness、
  capture state 与 Observer 状态。
- `GET /api/admin/monitors/:id/history`：返回 rollup 点、可选 checks 和 quality。

history query 使用 `from`、`to`、`resolution`、`observer_id`、`include`、
`limit`、`cursor`。
resolution 只能是 auto、1m、5m、1h。scheduled 统计排除 `mode=ad_hoc`。

## Structured errors

- `history_repository_unavailable`
- `observation_budget_exceeded`
- `invalid_public_target`
- `redirect_private_address`
- `dns_failure`
- `connect_failure`
- `tls_failure`
- `timeout`
- `http_status_mismatch`
- `body_mismatch`
- `tcp_refused`
- `icmp_unsupported`
- `monitor_paused`
- `monitor_deleted`
- `run_rate_limited`
- `revision_conflict`
- `history_partial`

## Signed history stream

- Stream name: `service_monitor_observation-v1`。
- identity 是 source node、epoch、stream、sequence；沿用签名与 canonical payload。
- payload 含 Monitor、revision、Observer、slot、mode、outcome、error、timings、
  packet counters。
- payload 只保存地址 digest，不保存 body、headers、credentials 或完整地址集合。
- Repository 在 rollup 前幂等插入；unknown schema 只转发，不参与 uptime 查询。

## Capability

- `service-monitor-http-v1` 与 `service-monitor-tcp-v1` 随健康 runtime 提供。
- `service-monitor-icmp-v1` 只有 ICMP self-test 成功后才提供。
- capability 变化影响未来 Slot 的 Observer Set，并出现在 detail/status 响应中。
