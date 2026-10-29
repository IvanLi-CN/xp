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
- `Unknown` 表示探测、DNS 或 Cloudflare API 异常，并中断该族连续缺失计数。
  若本轮至少一个地址族成功探测并同步 DNS，状态为 `up`，`last_error` 保留其他地址族异常。
  否则历史同步 IP 只用于区分 `degraded` 和 `down`，不能证明本轮成功。
- 探测目标不提供对应地址族或固定为另一族属于 `Unknown`，不能作为删除记录的证据。
  A 和 AAAA 独立处理；整轮无法执行时中断两个地址族的连续缺失计数。
- 启动时清零持久化的缺失计数，避免旧版本或中断期间的非连续观察授权删除。
  记录 ID 和历史同步地址仍保留，状态为 `unknown` 直至本进程完成一轮协调。
- timeout、HTTP 错误、连接拒绝等不能证明地址族缺失的错误必须保持 `Unknown`，避免误删 DNS 记录。
