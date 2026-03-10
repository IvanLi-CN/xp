# File formats

## inbound_ip_usage.json

- 路径：`${XP_DATA_DIR}/inbound_ip_usage.json`
- 作用：本地节点保存最近 7 天 membership source IP 分钟 presence 与 Geo 缓存。
- 不进入 Raft，不参与快照复制。

## Top-level shape

```json
{
  "schema_version": 1,
  "generated_at": "2026-03-08T10:12:00Z",
  "minutes_window": 10080,
  "latest_minute": "2026-03-08T10:11:00Z",
  "online_stats_unavailable": false,
  "memberships": {
    "u1:ep1": {
      "user_id": "u1",
      "node_id": "node-1",
      "endpoint_id": "ep1",
      "endpoint_tag": "vless-ep-1",
      "ips": {
        "203.0.113.7": {
          "bitmap_b64": "...",
          "minutes": 31,
          "first_seen_at": "2026-03-08T08:40:00Z",
          "last_seen_at": "2026-03-08T09:11:00Z",
          "geo": {
            "country": "JP",
            "region": "Tokyo",
            "city": "Tokyo",
            "operator": "Example ASN Org"
          }
        }
      }
    }
  }
}
```

## Rules

- `minutes_window` 固定为 `10080`。
- `latest_minute` 必须是 UTC 对齐到分钟的 RFC3339。
- `bitmap_b64` 表示从 `latest_minute - minutes_window + 1` 到 `latest_minute` 的 presence 位图。
- `minutes` 是当前窗口内该 IP 的置位总数，作为 API 快速路径缓存；若位图重算与其不一致，以位图为准并在保存时修正。
- 删除 membership / endpoint / user 后，相关 `memberships.*` 条目必须被裁掉。
- 若 Geo enrichment 未命中或查询失败，`geo.country/region/city/operator` 允许为空字符串。
