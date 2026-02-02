# HTTP Admin APIs (plan #8rrmk)

本文件冻结“用户接入点配置（无 group 概念）”所需的 Admin API 契约。

## Goals

- UI 只以 user 维度读取/写入“实际生效的 grants”。
- API 请求/响应不出现 `group_name` / `grant-group` 语义。

## Endpoints

### List user grants (effective)

`GET /api/admin/users/:user_id/grants`

Response `200`:

```json
{
  "items": [
    {
      "grant_id": "grant_...",
      "user_id": "user_...",
      "endpoint_id": "endpoint_...",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": "optional display note"
    }
  ]
}
```

Notes:

- 返回的是该用户当前所有 grants（包含历史带 group 的存量），但响应不暴露 group 字段。
- `enabled=false` 的历史 grants：
  - 推荐默认不返回（仅返回 enabled=true），以降低 UI 心智负担；如需要排障可通过 query 参数扩展（TBD）。

### Hard cut: replace user grants

`PUT /api/admin/users/:user_id/grants`

Request body:

```json
{
  "items": [
    {
      "endpoint_id": "endpoint_...",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": "optional"
    }
  ]
}
```

Response `200`:

```json
{
  "items": [
    {
      "grant_id": "grant_...",
      "user_id": "user_...",
      "endpoint_id": "endpoint_...",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": "optional"
    }
  ]
}
```

Semantics:

- hard cut：保存后该用户**实际生效**的 grants 集合 == 请求中的 `items`（按 endpoint_id 去重后）。
- 清理策略：
  - 推荐：delete 该 user 的历史 grants（不论其历史 group 来源），再按请求 items 创建新 grants。
  - 创建的新 grants 必须落为“无 group”（内部 `group_name=""`）。

Errors:

- `400 invalid_request`：endpoint_id 不存在 / items 为空但字段非法等。
- `404 not_found`：user 不存在。

## Compatibility

- 旧的 `grant-groups` API 仍可保留一段时间，但 UI 不再依赖。
