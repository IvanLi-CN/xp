# Admin HTTP APIs (spec #nujzm)

## Removed APIs

以下接口在本版本移除：

- `GET /api/admin/grant-groups`
- `POST /api/admin/grant-groups`
- `GET /api/admin/grant-groups/:group_name`
- `PUT /api/admin/grant-groups/:group_name`
- `DELETE /api/admin/grant-groups/:group_name`

行为：统一走路由 fallback，返回 `404 not_found`。

## New APIs

## GET `/api/admin/users/:user_id/access`

返回该用户当前有效接入关系（membership + grant 明细）。

### Response `200`

```json
{
  "items": [
    {
      "membership": {
        "user_id": "user_...",
        "node_id": "node_...",
        "endpoint_id": "endpoint_..."
      },
      "grant": {
        "grant_id": "grant_...",
        "enabled": true,
        "quota_limit_bytes": 0,
        "note": null,
        "credentials": {
          "vless": {
            "uuid": "...",
            "email": "..."
          }
        }
      }
    }
  ]
}
```

### Errors

- `404 not_found`: user 不存在。

## PUT `/api/admin/users/:user_id/access`

以 hard-cut 语义替换该用户全部接入关系。

### Request

```json
{
  "items": [
    {
      "endpoint_id": "endpoint_...",
      "note": null
    }
  ]
}
```

规则：

- 保存后该用户有效接入集合严格等于请求 `items` 的 endpoint 集合（按 endpoint_id 去重）。
- 新写入 grants 统一为 `enabled=true`。
- 请求 `items=[]` 表示清空该用户全部接入。

### Response `200`

与 `GET /api/admin/users/:user_id/access` 同形态。

### Errors

- `400 invalid_request`: endpoint 不存在、重复 endpoint、payload 结构非法。
- `404 not_found`: user 不存在。
