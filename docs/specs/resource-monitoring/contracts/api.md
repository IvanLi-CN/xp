# Resource Monitoring API and stream contract

所有 external Resource Monitoring API 都在 admin token 保护的 `/api/admin` 管理面下。写请求沿用 Leader/follower
转发与 revision conflict 语义；internal local API 只接受现有内部签名。

## Common values

- 时间使用 UTC RFC 3339；history bucket 使用 `bucket_start_unix_seconds`。
- bytes、计数、毫秒与采样数量是非负整数；百分比为 `0` 到 `100` 的数值，表示相对于该 Resource Domain 可用 CPU 容量的利用率。
- `capability` 为 `supported`、`partial` 或 `unsupported`。`unsupported` 不带 `value`，并带稳定
  `reason_code`；`partial` 表示至少一个相关 measurement 缺失，但已返回可读值。
- `capture_state` 为 `active` 或 `suspended`。它描述长期历史，不表示节点健康或资源为零。
- `resource_domain` 为 `host` 或 `cgroup`。`host` 仅用于 host-managed 节点；官方单镜像容器节点只能返回 `cgroup`。

## Current snapshot

`ResourceSnapshot` 包含：

```json
{
  "node_id": "node-a",
  "observed_at": "2026-09-01T00:00:00Z",
  "resource_domain": "host",
  "capture_state": "active",
  "capability": "partial",
  "domain": {
    "cpu_busy_percent": { "capability": "supported", "value": 23.4 },
    "cpu_iowait_percent": { "capability": "supported", "value": 1.2 },
    "memory_available_bytes": { "capability": "supported", "value": 1610612736 },
    "root_filesystem": {
      "capability": "supported",
      "available_bytes": 34359738368,
      "used_percent": 42.0,
      "available_inodes": 1200000
    }
  },
  "runtimes": [
    {
      "role": "xray",
      "state": "managed",
      "capability": "partial",
      "metrics": {
        "cpu_percent": { "capability": "supported", "value": 8.3 },
        "rss_bytes": { "capability": "supported", "value": 12582912 },
        "pss_bytes": {
          "capability": "unsupported",
          "reason_code": "proc_permission_denied"
        }
      }
    }
  ]
}
```

- `domain` 固定包含 CPU、load、memory、Swap、root/`XP_DATA_DIR` filesystem 与可归属 I/O 的 measurement；同一
  filesystem 只出现一次。
- `runtimes[].role` 仅为 `xp`、`xray`、`cloudflared` 或 `canary`。`state` 为 `managed` 或 `not_managed`；
  后者不同于 capability `unsupported`。
- 每个 runtime 的固定 measurement 是 CPU、RSS、PSS、read rate、write rate、FD count 与 thread count。不得出现
  PID、command line、任意 process name 或动态 label。

## Admin routes

- `GET /api/admin/nodes/resources`
  - 返回 `{ items, partial, unreachable_nodes }`。
  - `items` 是每个可达节点的 `ResourceSnapshot` 摘要，不包含近期 raw series。
- `GET /api/admin/nodes/:node_id/resources`
  - 返回一个完整 current `ResourceSnapshot`。
- `GET /api/admin/nodes/:node_id/resources/recent`
  - query：`metric=<name>` 与可选的 `role=<role?>`。
  - 返回一个指定 measurement 的最多 240 个 15 秒点。`role` 只用于 runtime measurement；每次请求只能返回一个 series，以保持响应有界。
- `GET /api/admin/nodes/:node_id/resources/history`
  - query：`metric=<name>`、可选 `role=<role?>`、`from=<unix>`、`to=<unix>`。
  - query：`resolution=auto|1m|15m|1h` 与 `limit=<1..1500>`。
  - 从最完整健康的 ready History Repository 返回一个语义 Rollup series，并携带 `quality`、coverage、watermark、gaps、
    freshness 与 `truncated`。
  - 没有 ready Repository 时可返回本地 24 小时 minute 数据并标记 `local_only`；超出本地窗口不得伪造 history。
- `GET /api/admin/resource-monitoring/policy`
  - 返回 revisioned cluster default 及显式 node/role overrides。
- `PUT /api/admin/resource-monitoring/policy`
  - 接受完整 policy 与 `expected_revision`，成功时创建下一个 revision。
  - 阈值必须有合法范围和非零持续时间；无效输入返回 `invalid_request`。

## Internal routes

- `GET /api/admin/_internal/nodes/resources/local`
  - 返回本节点 current `ResourceSnapshot`。
- `GET /api/admin/_internal/nodes/resources/local/recent`
  - query：`metric=<name>` 与可选 `role=<role?>`。
  - 返回本节点的一个 bounded recent series。

外部聚合只能调用这两个 internal route；它不能读取另一个节点的 Resource Store 文件。

## History stream

- schema ID 为 `resource_metrics.v1`，stream 为 `resource_metrics-v1`。
- identity 是 `(source_node_id, source_epoch, stream, sequence)`；subject 是资源所属 node。
- payload 的 `kind` 为 `rollup` 或 `capture_gap`。`rollup` 包含固定 Resource Domain、固定 role、bucket、
  expected/captured sample count、数值 aggregates 与 field capability。 `capture_gap` 只含连续的已知 minute
  范围和原因。
- payload 超过当前 resolution 的 2 KiB、1 KiB 或 768 B 上限时，source 必须拒绝入队并报告
  `resource_payload_budget_exceeded`；不得截断字段或添加动态压缩后例外。
- Repository 按资源流专用 reducer 生成 15-minute 和 hour Rollup，保留数值、counter、capability 与 capture
  completeness；不能退化为通用 hash-only aggregate。

## Alerts and events

`GET /api/admin/alerts` 的现有 response 新增 additive item：

```json
{
  "type": "resource_threshold",
  "node_id": "node-a",
  "scope": "domain",
  "metric": "cpu_busy_percent",
  "severity": "warning",
  "opened_at": "2026-09-01T00:00:00Z",
  "latest_bucket_start_unix_seconds": 1788220800
}
```

- `type` 还可为 `resource_capture_suspended`。已有 alert type 与 consumers 保持兼容。
- 现有 admin status SSE 增加 `resource_alert_opened`、`resource_alert_escalated` 与
  `resource_alert_recovered`。事件只表达状态转换，不能携带 15 秒原始 series。
- alert 只在管理面可见，不包含 webhook、email、IM 或自动修复命令。

## Stable errors

- `resource_monitoring_unsupported`
- `resource_history_capacity_rejected`
- `resource_history_unavailable`
- `resource_payload_budget_exceeded`
- `revision_conflict`
- `invalid_request`
