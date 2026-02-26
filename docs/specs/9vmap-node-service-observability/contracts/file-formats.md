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
