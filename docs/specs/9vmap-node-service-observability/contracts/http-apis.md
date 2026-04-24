# HTTP API Contracts（#9vmap）

## `GET /api/admin/nodes`

Response:

```json
{
  "items": [
    {
      "node_id": "01J...",
      "node_name": "node-1",
      "access_host": "example.com",
      "api_base_url": "https://node-1.internal:8443",
      "egress_probe": {
        "public_ipv4": "203.0.113.8",
        "public_ipv6": "2001:db8::8",
        "selected_public_ip": "203.0.113.8",
        "country_code": "TW",
        "geo_region": "Taiwan",
        "geo_city": "Taipei",
        "geo_operator": "HiNet",
        "subscription_region": "taiwan",
        "checked_at": "RFC3339",
        "last_success_at": "RFC3339|null",
        "stale": false,
        "error_summary": "string|null"
      }
    }
  ]
}
```

Notes:

- `egress_probe` 可为空，表示该节点还没有保存任何主动探测结果。
- `subscription_region` 固定取值：`japan|hong_kong|taiwan|korea|singapore|us|other`。

## `GET /api/admin/nodes/{node_id}`

Response:

- 与 `GET /api/admin/nodes` 中的单项结构相同。

## `POST /api/admin/nodes/{node_id}/egress-probe/refresh`

Response:

```json
{
  "node_id": "01J...",
  "accepted": true,
  "egress_probe": {
    "selected_public_ip": "203.0.113.8",
    "subscription_region": "taiwan",
    "stale": false
  }
}
```

Notes:

- 当前节点会直接触发本地刷新；远端节点由 leader 通过 internal signature 转发到目标节点的 local refresh 接口。

## `GET /api/admin/nodes/runtime`

Response:

```json
{
  "partial": false,
  "unreachable_nodes": [],
  "items": [
    {
      "node_id": "01J...",
      "summary": "up|degraded|down|unknown",
      "components": [
        {
          "component": "xp|xray|cloudflared",
          "status": "disabled|up|down|unknown",
          "checked_at": "RFC3339",
          "down_since": "RFC3339|null",
          "consecutive_failures": 0,
          "restart_attempts": 0,
          "last_restart_at": "RFC3339|null",
          "last_restart_failed_at": "RFC3339|null"
        }
      ],
      "recent_slots": [
        {
          "slot_start": "RFC3339",
          "summary": "up|degraded|down|unknown",
          "xp": "up|down|unknown",
          "xray": "disabled|up|down|unknown",
          "cloudflared": "disabled|up|down|unknown"
        }
      ]
    }
  ]
}
```

## `GET /api/admin/nodes/{node_id}/runtime`

Response:

```json
{
  "node_id": "01J...",
  "summary": "up|degraded|down|unknown",
  "components": [],
  "recent_slots": [],
  "events": []
}
```

## `GET /api/admin/nodes/{node_id}/runtime/events` (SSE)

- Event types: `hello`, `snapshot`, `event`, `node_error`, `lagged`
- `hello`: connection metadata
- `snapshot`: full runtime snapshot
- `event`: single runtime event increment

## Internal APIs

- `GET /api/admin/_internal/nodes/runtime/local`
- `GET /api/admin/_internal/nodes/runtime/local/events`
- `GET /api/admin/_internal/nodes/egress-probe/local`
- `POST /api/admin/_internal/nodes/egress-probe/local/refresh`

要求：仅允许 internal signature 访问；禁止 bearer-only 直接访问。
