# HTTP API Contracts（#0021）

本文件用于冻结“health 输出中 `xray` 状态”的最小对外契约。

## `GET /api/health` (modify, backward compatible)

### Request

- Method: `GET`
- Path: `/api/health`
- Auth: none

### Response (200)

必须保留现有字段：

```json
{ "status": "ok" }
```

并在此基础上追加（additive）：

```json
{
  "status": "ok",
  "xray": {
    "status": "unknown|up|down",
    "last_ok_at": "RFC3339|null",
    "last_fail_at": "RFC3339|null",
    "down_since": "RFC3339|null",
    "consecutive_failures": 0,
    "recoveries_observed": 0
  }
}
```

字段语义（normative）：

- `xray.status`：
  - `unknown`: 启动初期尚未完成一次探活（或探活模块未启动）
  - `up`: 最近一次探活成功
  - `down`: 已连续失败达到阈值
- `xray.last_ok_at`：最近一次探活成功的时间；若从未成功则为 `null`
- `xray.last_fail_at`：最近一次探活失败的时间；若从未失败则为 `null`
- `xray.down_since`：进入 `down` 状态的起始时间；若当前非 down 则为 `null`
- `xray.consecutive_failures`：当前连续失败次数（成功后归零）
- `xray.recoveries_observed`：自进程启动以来观察到的恢复次数（`down -> up` 的次数；不代表由 `xp` 执行重启）

### Errors

- `GET /api/health` MUST 始终返回 `200`（用于“服务存活”探测），不因 `xray` down 而返回非 200。
- `xray` 的状态通过响应体表达，不通过 status code 表达。
