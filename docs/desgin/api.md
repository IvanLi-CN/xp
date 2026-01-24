# xp · HTTP(S) API 设计（MVP）

> 说明：这里定义接口形状与约定，便于后续实现与前端/CLI 对接。\
> 所有写请求在 follower 上会被转发到 leader（或返回 leader 地址供客户端重试）。

## 1. 通用约定

- Base URL：`<node.api_base_url>`（必须是 `https://...` 的完整 origin）
- 路径前缀：所有接口均以 `/api` 开头
- 编码：UTF-8
- 时间：RFC3339（带时区）
- ID：ULID 字符串

### 1.1 认证

- 管理员 API（`/api/admin/*`）：`Authorization: Bearer <admin_token>`
- 订阅 API（`/api/sub/*`）：基于 `subscription_token`（见 `subscription.md`）

### 1.2 错误格式

```json
{
  "error": {
    "code": "string",
    "message": "string",
    "details": {}
  }
}
```

### 1.3 健康检查

`GET /api/health`

返回：

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

## 2. 集群与节点

### 2.1 获取集群状态

`GET /api/cluster/info`

返回：

```json
{
  "cluster_id": "01J...",
  "node_id": "01J...",
  "role": "leader|follower",
  "leader_api_base_url": "https://...",
  "term": 1
}
```

### 2.2 生成 Join Token（leader 写）

`POST /api/admin/cluster/join-tokens`

请求：

```json
{ "ttl_seconds": 900 }
```

返回：

```json
{ "join_token": "base64url(...)" }
```

> join token 内包含 `cluster_ca_pem`、`leader_api_base_url` 与一次性密钥（具体编码后续实现再定）。

### 2.3 节点加入（新节点调用）

`POST /api/cluster/join`

请求：

```json
{
  "join_token": "base64url(...)",
  "node_name": "node-1",
  "access_host": "example.com",
  "api_base_url": "https://node-1.internal:8443",
  "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\n...\n-----END CERTIFICATE REQUEST-----\n"
}
```

返回：

```json
{
  "node_id": "01J...",
  "signed_cert_pem": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n",
  "cluster_ca_pem": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n",
  "cluster_ca_key_pem": "-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----\n"
}
```

> `cluster_ca_key_pem` 为敏感私钥，仅在 leader 节点本地确实持有时才会返回；新节点收到后应以 0600（Unix）权限落盘保存。

### 2.4 查询节点列表（管理员）

`GET /api/admin/nodes`

返回：

```json
{
  "items": [
    {
      "node_id": "01J...",
      "node_name": "node-1",
      "access_host": "example.com",
      "api_base_url": "https://node-1.internal:8443"
    }
  ]
}
```

### 2.5 查询单个节点（管理员）

`GET /api/admin/nodes/{node_id}`

返回：Node（略，字段同上）。

### 2.6 更新节点（管理员）

> 说明：该接口只更新 Node 的“展示/路由相关元数据”（例如 `access_host`），**不涉及** Raft membership 变更与节点移除。

`PATCH /api/admin/nodes/{node_id}`

请求：

```json
{
  "node_name": "node-1",
  "access_host": "example.com",
  "api_base_url": "https://node-1.internal:8443"
}
```

返回：Node（略，字段同创建/查询返回）。

## 3. Endpoints（端点）

### 3.1 创建端点

`POST /api/admin/endpoints`

请求（VLESS/Reality）：

```json
{
  "node_id": "01J...",
  "kind": "vless_reality_vision_tcp",
  "port": 443,
  "reality": {
    "dest": "example.com:443",
    "server_names": ["example.com"],
    "fingerprint": "chrome"
  }
}
```

请求（SS2022）：

```json
{
  "node_id": "01J...",
  "kind": "ss2022_2022_blake3_aes_128_gcm",
  "port": 8388
}
```

返回（通用）：

```json
{
  "endpoint_id": "01J...",
  "node_id": "01J...",
  "tag": "vless-vision-01J...",
  "kind": "vless_reality_vision_tcp",
  "port": 443,
  "meta": {}
}
```

### 3.2 查询端点列表（管理员）

`GET /api/admin/endpoints`

返回：

```json
{
  "items": [
    {
      "endpoint_id": "01J...",
      "node_id": "01J...",
      "tag": "vless-vision-01J...",
      "kind": "vless_reality_vision_tcp",
      "port": 443,
      "meta": {}
    }
  ]
}
```

### 3.3 查询单个端点（管理员）

`GET /api/admin/endpoints/{endpoint_id}`

返回：Endpoint（略，字段同创建返回）。

### 3.4 更新端点（管理员）

> 约束：`kind/node_id/tag` 不可变；如需迁移到其它节点，请通过“新建 endpoint → 迁移 grants → 删除旧 endpoint”完成。

`PATCH /api/admin/endpoints/{endpoint_id}`

请求（VLESS/Reality）：

```json
{
  "port": 443,
  "reality": {
    "dest": "example.com:443",
    "server_names": ["example.com"],
    "fingerprint": "chrome"
  }
}
```

请求（SS2022）：

```json
{ "port": 8388 }
```

返回：Endpoint（略，字段同创建返回）。

### 3.5 删除端点（管理员）

`DELETE /api/admin/endpoints/{endpoint_id}`

返回：`204 No Content`。

### 3.6 旋转 shortId

`POST /api/admin/endpoints/{endpoint_id}/rotate-shortid`

返回：

```json
{
  "endpoint_id": "01J...",
  "active_short_id": "0123456789abcdef",
  "short_ids": ["0123...", "..."]
}
```

## 4. Users（用户）

### 4.1 创建用户

`POST /api/admin/users`

请求：

```json
{
  "display_name": "alice",
  "cycle_policy_default": "by_user",
  "cycle_day_of_month_default": 1
}
```

返回：

```json
{
  "user_id": "01J...",
  "display_name": "alice",
  "subscription_token": "sub_...",
  "cycle_policy_default": "by_user",
  "cycle_day_of_month_default": 1
}
```

### 4.2 查询用户列表（管理员）

`GET /api/admin/users`

返回：

```json
{
  "items": [
    {
      "user_id": "01J...",
      "display_name": "alice",
      "subscription_token": "sub_...",
      "cycle_policy_default": "by_user",
      "cycle_day_of_month_default": 1
    }
  ]
}
```

### 4.3 查询单个用户（管理员）

`GET /api/admin/users/{user_id}`

返回：User（略，字段同创建返回）。

### 4.4 更新用户（管理员）

`PATCH /api/admin/users/{user_id}`

请求：

```json
{
  "display_name": "alice",
  "cycle_policy_default": "by_user",
  "cycle_day_of_month_default": 1
}
```

返回：User（略，字段同创建返回）。

### 4.5 删除用户（管理员）

`DELETE /api/admin/users/{user_id}`

返回：`204 No Content`。

### 4.6 重置订阅 token

`POST /api/admin/users/{user_id}/reset-token`

返回：

```json
{ "subscription_token": "sub_..." }
```

## 5. Grants（授权）

### 5.1 创建授权（分配端点给用户）

`POST /api/admin/grants`

请求：

```json
{
  "user_id": "01J...",
  "endpoint_id": "01J...",
  "quota_limit_bytes": 10737418240,
  "cycle_policy": "inherit_user",
  "cycle_day_of_month": null,
  "note": "alice@node-1"
}
```

返回：

```json
{
  "grant_id": "01J...",
  "user_id": "01J...",
  "endpoint_id": "01J...",
  "enabled": true,
  "quota_limit_bytes": 10737418240,
  "cycle_policy": "inherit_user",
  "cycle_day_of_month": null,
  "note": "alice@node-1",
  "credentials": {
    "vless": { "uuid": "xxxxxxxx-xxxx-....", "email": "grant:01J..." }
  }
}
```

### 5.2 更新授权（启用/禁用/改配额）

`PATCH /api/admin/grants/{grant_id}`

请求：

```json
{
  "enabled": false,
  "note": "alice@node-1",
  "quota_limit_bytes": 0,
  "cycle_policy": "by_user",
  "cycle_day_of_month": 1
}
```

返回：Grant 当前状态（略）。

### 5.3 查询授权列表（管理员）

`GET /api/admin/grants`

返回：

```json
{
  "items": [
    {
      "grant_id": "01J...",
      "user_id": "01J...",
      "endpoint_id": "01J...",
      "enabled": true,
      "quota_limit_bytes": 10737418240,
      "cycle_policy": "inherit_user",
      "cycle_day_of_month": null,
      "note": "alice@node-1",
      "credentials": { "vless": { "uuid": "xxxxxxxx-xxxx-....", "email": "grant:01J..." } }
    }
  ]
}
```

### 5.4 查询单个授权（管理员）

`GET /api/admin/grants/{grant_id}`

返回：Grant（略，字段同列表项）。

### 5.5 删除授权（管理员）

`DELETE /api/admin/grants/{grant_id}`

返回：`204 No Content`。

### 5.6 查询用量（可代理到目标节点）

`GET /api/admin/grants/{grant_id}/usage`

> Milestone 1 无 Stats 数据源时，推荐返回 `501 Not Implemented`（error.code=`not_implemented`），由前端展示 “N/A”；用量将在 Milestone 4 接入。

返回：

```json
{
  "grant_id": "01J...",
  "cycle_start_at": "2025-12-01T00:00:00+08:00",
  "cycle_end_at": "2026-01-01T00:00:00+08:00",
  "used_bytes": 123456789,
  "owner_node_id": "01J...",
  "desired_enabled": true,
  "quota_banned": false,
  "quota_banned_at": null,
  "effective_enabled": true,
  "warning": null
}
```

### 5.7 查询异常提示（管理员）

`GET /api/admin/alerts`

返回：

```json
{
  "partial": false,
  "unreachable_nodes": [],
  "items": [
    {
      "type": "quota_enforced_but_desired_enabled",
      "grant_id": "01J...",
      "endpoint_id": "01J...",
      "owner_node_id": "01J...",
      "desired_enabled": true,
      "quota_banned": true,
      "quota_banned_at": "2025-12-01T00:00:00Z",
      "effective_enabled": false,
      "message": "quota enforced on owner node but desired state is still enabled",
      "action_hint": "check raft leader/quorum and retry status"
    }
  ]
}
```
