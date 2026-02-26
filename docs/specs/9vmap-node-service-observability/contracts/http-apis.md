# HTTP API Contracts（#9vmap）

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

要求：仅允许 internal signature 访问；禁止 bearer-only 直接访问。
