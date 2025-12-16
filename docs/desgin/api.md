# xp · HTTP(S) API 设计（MVP）

> 说明：这里定义接口形状与约定，便于后续实现与前端/CLI 对接。  
> 所有写请求在 follower 上会被转发到 leader（或返回 leader 地址供客户端重试）。

## 1. 通用约定

- Base URL：`<node.api_base_url>`（必须是 `https://...` 的完整 origin）
- 编码：UTF-8
- 时间：RFC3339（带时区）
- ID：ULID 字符串

### 1.1 认证

- 管理员 API：`Authorization: Bearer <admin_token>`
- 订阅 API：基于 `subscription_token`（见 `subscription.md`）

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

## 2. 集群与节点

### 2.1 获取集群状态

`GET /cluster/info`

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

`POST /admin/cluster/join-tokens`

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

`POST /cluster/join`

请求：

```json
{
  "join_token": "base64url(...)",
  "node_name": "node-1",
  "public_domain": "example.com",
  "api_base_url": "https://node-1.internal:8443",
  "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\n...\n-----END CERTIFICATE REQUEST-----\n"
}
```

返回：

```json
{
  "node_id": "01J...",
  "signed_cert_pem": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n",
  "cluster_ca_pem": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n"
}
```

## 3. Endpoints（端点）

### 3.1 创建端点

`POST /admin/endpoints`

请求（VLESS/Reality）：

```json
{
  "node_id": "01J...",
  "kind": "vless_reality_vision_tcp",
  "port": 443,
  "public_domain": "example.com",
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

### 3.2 旋转 shortId

`POST /admin/endpoints/{endpoint_id}/rotate-shortid`

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

`POST /admin/users`

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
  "subscription_token": "sub_..."
}
```

### 4.2 重置订阅 token

`POST /admin/users/{user_id}/reset-token`

返回：

```json
{ "subscription_token": "sub_..." }
```

## 5. Grants（授权）

### 5.1 创建授权（分配端点给用户）

`POST /admin/grants`

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
  "enabled": true,
  "credentials": {
    "vless": { "uuid": "xxxxxxxx-xxxx-....", "email": "grant:01J..." }
  }
}
```

### 5.2 更新授权（启用/禁用/改配额）

`PATCH /admin/grants/{grant_id}`

请求：

```json
{
  "enabled": false,
  "quota_limit_bytes": 0,
  "cycle_policy": "by_user",
  "cycle_day_of_month": 1
}
```

返回：Grant 当前状态（略）。

### 5.3 查询用量（可代理到目标节点）

`GET /admin/grants/{grant_id}/usage`

返回：

```json
{
  "grant_id": "01J...",
  "cycle_start_at": "2025-12-01T00:00:00+08:00",
  "cycle_end_at": "2026-01-01T00:00:00+08:00",
  "used_bytes": 123456789
}
```
