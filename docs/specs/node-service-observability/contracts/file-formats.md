# File Format Contracts（#9vmap）

## `${XP_DATA_DIR}/service_runtime.json`

```json
{
  "schema_version": 1,
  "last_snapshot": {
    "checked_at": "RFC3339",
    "summary": "up|degraded|down|unknown",
    "components": []
  },
  "recent_slots": [],
  "events": []
}
```

约束：

- `recent_slots` 仅保留最近 336 个（7d/30min）。
- `events` 仅保留最近 7 天。
- 文件写入需原子化，坏文件启动时应回退为空状态并记录告警。

## `${XP_DATA_DIR}/ddns_state.json`

```json
{
  "schema_version": 1,
  "hostname": "node-ep.example.com",
  "zone_id": "cloudflare-zone-id",
  "snapshot": {
    "status": "disabled|unknown|up|degraded|down",
    "current_ipv4": "203.0.113.8",
    "current_ipv6": null,
    "last_error": null
  },
  "ipv4": {
    "record_id": "cloudflare-dns-record-id",
    "synced_ip": "203.0.113.8",
    "missing_count": 0
  },
  "ipv6": {
    "record_id": null,
    "synced_ip": null,
    "missing_count": 3
  }
}
```

约束：

- `MissingCandidate` 表示该地址族当前没有可发布公网地址；连续达到 grace 后删除对应 `A` / `AAAA` 记录，但不作为 DDNS 组件异常。
- `Unknown` 表示探测、DNS 或 Cloudflare API 异常；若已有任一同步 IP，DDNS 组件进入 `degraded`，否则进入 `down`。
- timeout、HTTP 错误、连接拒绝等不能证明地址族缺失的错误必须保持 `Unknown`，避免误删 DNS 记录。
