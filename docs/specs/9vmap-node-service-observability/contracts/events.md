# Runtime Events Contracts（#9vmap）

`event` payload:

```json
{
  "event_id": "01J...",
  "occurred_at": "RFC3339",
  "component": "xray|cloudflared",
  "kind": "status_changed|restart_requested|restart_failed",
  "level": "info|warning",
  "message": "string",
  "status_from": "disabled|up|down|unknown|null",
  "status_to": "disabled|up|down|unknown|null"
}
```

语义：

- `status_changed`: 组件状态切换事件。
- `restart_requested`: supervisor 发起 restart 请求。
- `restart_failed`: restart 请求失败（命令/权限/超时）。
