# HTTP APIs

## Window query

所有 usage API 都接受：

- `window=24h|7d`
- 未提供时默认 `24h`
- 其他值返回 `400 invalid_request`

## GET /api/admin/nodes/{node_id}/ip-usage?window=24h|7d

返回单节点视图；`geo_source` 表示当前节点的 IP Geo 数据源（启用时为 `country_is`，禁用时为 `missing`）：

```json
{
  "node": {
    "node_id": "01J...",
    "node_name": "node-1",
    "access_host": "node-1.example.com",
    "api_base_url": "https://node-1.example.com"
  },
  "window": "24h",
  "window_start": "2026-03-07T10:12:00Z",
  "window_end": "2026-03-08T10:11:00Z",
  "warnings": [
    {
      "code": "online_stats_unavailable",
      "message": "string"
    }
  ],
  "unique_ip_series": [
    {
      "minute": "2026-03-08T09:10:00Z",
      "count": 3
    }
  ],
  "timeline": [
    {
      "lane_key": "vless-ep-1|203.0.113.7",
      "endpoint_id": "ep-1",
      "endpoint_tag": "vless-ep-1",
      "ip": "203.0.113.7",
      "minutes": 28,
      "segments": [
        {
          "start_minute": "2026-03-08T08:40:00Z",
          "end_minute": "2026-03-08T08:55:00Z"
        }
      ]
    }
  ],
  "ips": [
    {
      "ip": "203.0.113.7",
      "minutes": 31,
      "endpoint_tags": ["vless-ep-1", "ss-ep-2"],
      "region": "JP Tokyo",
      "operator": "Example ASN Org",
      "last_seen_at": "2026-03-08T09:11:00Z"
    }
  ]
}
```

## GET /api/admin/_internal/nodes/ip-usage/local?window=24h|7d

仅 internal signature 可访问；返回 shape 与公开 node API 相同，但只允许本地节点调用，不做跨节点转发。

## GET /api/admin/users/{user_id}/ip-usage?window=24h|7d

返回按节点分组的用户视图；每个 `groups[].geo_source` 表示该节点的 IP Geo 数据源：

```json
{
  "user": {
    "user_id": "01J...",
    "display_name": "Alice"
  },
  "window": "7d",
  "partial": false,
  "unreachable_nodes": [],
  "warnings": [],
  "groups": [
    {
      "node": {
        "node_id": "node-1",
        "node_name": "node-1",
        "access_host": "node-1.example.com",
        "api_base_url": "https://node-1.example.com"
      },
      "window_start": "2026-03-01T10:12:00Z",
      "window_end": "2026-03-08T10:11:00Z",
      "warnings": [],
      "unique_ip_series": [],
      "timeline": [],
      "ips": []
    }
  ]
}
```

## GET /api/admin/_internal/users/{user_id}/ip-usage/local?window=24h|7d

仅 internal signature 可访问；返回 shape 与 `groups[]` 的单节点内容相同，外加 `node` 元数据。

## Error semantics

- `404 not_found`: node / user 不存在。
- `400 invalid_request`: `window` 非法。
- `geo_db_missing` warning 已移除；Geo 查询失败时仅留下空的 `region/operator` 字段。
- Node detail 公开 API 访问远端节点失败时：返回 `500 internal`，消息需指明 unreachable/timeout。
- User detail 公开 API 聚合远端节点失败时：该节点加入 `unreachable_nodes`，其余节点结果继续返回，`partial=true`。
