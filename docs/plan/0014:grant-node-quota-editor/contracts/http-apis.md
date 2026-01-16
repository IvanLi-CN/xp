# HTTP APIs（Admin）

本文件描述本计划新增的 Admin API：按“用户 × 节点”管理节点配额（node quota）。

## Auth

- `Authorization: Bearer <admin_token>`

## Data model

### `UserNodeQuota`

```json
{
  "user_id": "u_...",
  "node_id": "n_...",
  "quota_limit_bytes": 10737418240
}
```

- `quota_limit_bytes`: integer, `>= 0`
  - `0` 表示“无限制/不触发配额封禁”（与现有 `quota_limit_bytes == 0` 语义一致）

## APIs

### List node quotas

- Method: `GET`
- Path: `/api/admin/users/:user_id/node-quotas`
- Response: `200 OK`

```json
{
  "items": [
    {
      "user_id": "u_...",
      "node_id": "n_...",
      "quota_limit_bytes": 10737418240
    }
  ]
}
```

### Set node quota

- Method: `PUT`
- Path: `/api/admin/users/:user_id/node-quotas/:node_id`
- Request: `application/json`

```json
{
  "quota_limit_bytes": 10737418240
}
```

- Response: `200 OK`

```json
{
  "user_id": "u_...",
  "node_id": "n_...",
  "quota_limit_bytes": 10737418240
}
```

### Errors（shared）

- `401 Unauthorized`: missing/invalid admin token
- `404 Not Found`: user/node not found
- `400 Bad Request`: invalid payload (negative quota, wrong type, overflow, etc.)
- `500 Internal Server Error`: unexpected failures

## Compatibility & rollout notes

- 本 API 为新增接口，不影响既有 `/api/admin/grants` 的字段与行为。
- 若历史数据存在“同一节点下不同 grants 的 `quota_limit_bytes` 不一致”，该接口设置节点配额后应统一该节点下所有相关 grants 的视角（具体策略在实现阶段落地，但对外口径以本 API 的返回为准）。
