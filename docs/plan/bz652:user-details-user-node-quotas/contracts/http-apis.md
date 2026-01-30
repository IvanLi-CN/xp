# HTTP APIs（User access matrix）

本计划不新增后端接口；但为了实现可测试与可复用，本文件冻结本次 UI 依赖的 HTTP 契约（请求/响应/错误形状与关键语义）。

## Auth

- `Authorization: Bearer <admin_token>`
- `Accept: application/json`

## Errors（通用）

非 `2xx` 时，响应体应为：

```json
{
  "error": {
    "code": "string",
    "message": "string",
    "details": {}
  }
}
```

备注：UI 会把 `status`、`code`（若存在）与 `message` 拼接为可读错误提示。

---

## GET `/api/admin/nodes`

用于矩阵行（nodes）渲染与过滤。

### Response `200`

```json
{
  "items": [
    {
      "node_id": "node_xxx",
      "node_name": "node-a",
      "api_base_url": "https://...",
      "access_host": "example.com",
      "quota_reset": { "policy": "monthly", "day_of_month": 1, "tz_offset_minutes": 480 }
    }
  ]
}
```

---

## GET `/api/admin/endpoints`

用于矩阵列（protocols）与单元格（cell）内容（node+protocol 可用 endpoints、端口、tag）。

### Response `200`

```json
{
  "items": [
    {
      "endpoint_id": "ep_xxx",
      "node_id": "node_xxx",
      "tag": "node-a-443",
      "kind": "vless_reality_vision_tcp",
      "port": 443,
      "meta": {}
    }
  ]
}
```

---

## GET `/api/admin/users/:user_id/node-quotas`

用于构建/回填 grant group members 的 `quota_limit_bytes`（与 `GrantNewPage` 一致：按 endpoint 的 `node_id` 取该用户对应 node quota；若不存在则默认为 `0`）。

### Response `200`

```json
{
  "items": [
    {
      "user_id": "usr_xxx",
      "node_id": "node_xxx",
      "quota_limit_bytes": 123,
      "quota_reset_source": "node"
    }
  ]
}
```

备注：`quota_reset_source` 的取值由后端决定；UI 仅透传（若后续需要编辑 node quota 再在本契约中补充 PUT 口径）。

---

## GET `/api/admin/grant-groups/:group_name`

用于加载本用户的 managed group（若不存在则按“空选择”处理）。

### Response `200`

```json
{
  "group": { "group_name": "managed-<user_id_fragment>" },
  "members": [
    {
      "user_id": "usr_xxx",
      "endpoint_id": "ep_xxx",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": null
    }
  ]
}
```

备注：响应体可能包含 `credentials` 等额外字段；本计划 UI 明确忽略这些字段（不展示、不中转）。

### Error `404`

- 语义：该 group 当前不存在（UI 将其视为“空选择”）。

---

## POST `/api/admin/grant-groups`

用于首次创建本用户的 managed group（当用户有至少 1 个勾选项时）。

### Request body

```json
{
  "group_name": "managed-<user_id_fragment>",
  "members": [
    {
      "user_id": "usr_xxx",
      "endpoint_id": "ep_xxx",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": null
    }
  ]
}
```

### Response `200`

与 `GET /api/admin/grant-groups/:group_name` 相同形状。

---

## PUT `/api/admin/grant-groups/:group_name`

用于“硬切（hard cut）”保存：用当前矩阵选择覆盖该 group 的 members。

### Request body

```json
{
  "members": [
    {
      "user_id": "usr_xxx",
      "endpoint_id": "ep_xxx",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": null
    }
  ]
}
```

### Response `200`

```json
{
  "group": { "group_name": "managed-usr_xxx" },
  "created": 1,
  "updated": 2,
  "deleted": 3
}
```

### Error `404`

- 语义：group 不存在（UI 应转为走 `POST /api/admin/grant-groups` 创建；或在实现中先探测 `GET`）。

### Constraints（服务端校验）

- `members` 不能为空；因此“全关闭/空选择”必须走 `DELETE`。

---

## Managed group naming（本计划冻结）

- `group_name = managed-${sanitize_group_name_fragment(user_id)}`
- 其中 `sanitize_group_name_fragment` 的字符集规则与 `src/state.rs` 一致：仅保留 `[a-z0-9-_]`，其他字符替换为 `-`，字母统一小写。

---

## DELETE `/api/admin/grant-groups/:group_name`

用于“空选择”保存：删除 managed group（等价于该用户无接入点）。

### Response `200`

```json
{ "deleted": 3 }
```
