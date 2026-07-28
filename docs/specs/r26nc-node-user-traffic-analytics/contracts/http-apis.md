# Traffic HTTP API Contract

## Common response shape

Traffic responses use UTC RFC3339 timestamps and return current and immediately preceding equal-length windows:

```json
{
  "timezone": "UTC",
  "window": "24h|31d",
  "window_start_at": "RFC3339",
  "window_end_at": "RFC3339",
  "summary": {
    "mode": "cycle|rolling_30d",
    "cycle_start_at": "RFC3339|null",
    "cycle_end_at": "RFC3339|null",
    "uplink_bytes": 0,
    "downlink_bytes": 0,
    "total_bytes": 0,
    "complete": false,
    "tracking_since": "RFC3339|null"
  },
  "current": [
    {
      "start_at": "RFC3339",
      "end_at": "RFC3339",
      "uplink_bytes": 0,
      "downlink_bytes": 0,
      "total_bytes": 0,
      "complete": true,
      "is_current_day": false
    }
  ],
  "reference": [],
  "partial": false,
  "warnings": [],
  "last_sample_at": "RFC3339|null"
}
```

`current` and `reference` always contain aligned points for the requested window (`288` five-minute points for `24h`, `31` daily points for `31d`). Missing points omit or return `null` byte fields and set `complete=false`; clients must render a gap and must not fill, interpolate, or carry forward values. `partial=true` is returned when either window, the summary, or a remote node is incomplete.

The node response wraps the common shape as `{ "node": {...}, "traffic": {...} }`. The user response wraps it as `{ "user": {"user_id", "display_name"}, "traffic": {...}, "nodes": [{"node_id", "node_name"}], "partial": bool, "unreachable_nodes": [] }`.

## Node endpoint

`GET /api/admin/nodes/{node_id}/traffic?window=24h|31d`

Returns the node aggregate, including endpoint probe traffic, from the panel mirror or the local node.

## User endpoint

`GET /api/admin/users/{user_id}/traffic?window=24h|31d&node_id=<optional>`

Returns user traffic aggregated across all reachable nodes unless `node_id` is supplied. The response additionally contains `partial`, `unreachable_nodes`, and the available node descriptors.

## Internal endpoints

- `GET /api/admin/_internal/nodes/traffic/local?window=24h|31d`
- `GET /api/admin/_internal/users/{user_id}/traffic/local?window=24h|31d`
- `DELETE /api/admin/_internal/users/{user_id}/traffic/local`

All internal traffic endpoints require the existing internal signature authentication. The DELETE
endpoint clears the local user rollup after a replicated cluster-wide user deletion.
