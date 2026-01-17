# HTTP API

## AdminServiceConfig（GET /api/admin/config）

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: Bearer admin token（`Authorization: Bearer <token>`）

### 请求（Request）

- Headers:
  - `Accept: application/json`
  - `Authorization: Bearer <token>`
- Query: None
- Body: None

### 响应（Response）

- Success: `AdminServiceConfig`
  ```json
  {
    "bind": "127.0.0.1:62416",
    "xray_api_addr": "127.0.0.1:10085",
    "data_dir": "./data",
    "node_name": "node-1",
    "access_host": "",
    "api_base_url": "https://127.0.0.1:62416",
    "quota_poll_interval_secs": 10,
    "quota_auto_unban": true,
    "admin_token_present": true,
    "admin_token_masked": "****1a2b"
  }
  ```

- Error: `BackendErrorResponse`
  ```json
  {
    "error": {
      "code": "unauthorized",
      "message": "missing or invalid authorization token",
      "details": {}
    }
  }
  ```

### 字段说明（Schema）

- `bind`: string（SocketAddr）
- `xray_api_addr`: string（SocketAddr）
- `data_dir`: string（路径，允许相对路径）
- `node_name`: string
- `access_host`: string（允许空；语义为“订阅/客户端连接 host”，允许 IP）
- `api_base_url`: string（URL）
- `quota_poll_interval_secs`: number（整数）
- `quota_auto_unban`: boolean
- `admin_token_present`: boolean
- `admin_token_masked`: string（脱敏展示；长度与实际 token 一致，全部为 `*`；当 `admin_token_present=false` 时可为空字符串）

### 错误（Errors）

- `401/unauthorized`: missing or invalid authorization token（retryable: no）
- `500/internal`: unexpected server error（retryable: yes）

### 示例（Examples）

- Request（请求）:
  - `GET /api/admin/config`
- Response（响应）:
  - 见 “Success” 示例

### 兼容性与迁移（Compatibility / migration）

- 新增接口，无迁移需求。

## AdminNodes（GET /api/admin/nodes）

- 范围（Scope）: internal
- 变更（Change）: Modify
- 鉴权（Auth）: Bearer admin token

### 请求（Request）

- Headers: `Accept: application/json`, `Authorization: Bearer <token>`
- Query: None
- Body: None

### 响应（Response）

- Success:
  ```json
  {
    "items": [
      {
        "node_id": "01H...",
        "node_name": "node-1",
        "access_host": "example.com",
        "api_base_url": "https://127.0.0.1:62416"
      }
    ]
  }
  ```
- Error: `BackendErrorResponse`

### 错误（Errors）

- `401/unauthorized`: missing or invalid authorization token（retryable: no）
- `500/internal`: unexpected server error（retryable: yes）

### 兼容性与迁移（Compatibility / migration）

- 破坏性变更：`public_domain` 字段移除，统一使用 `access_host`。

## AdminNode（GET /api/admin/nodes/:node_id）

- 范围（Scope）: internal
- 变更（Change）: Modify
- 鉴权（Auth）: Bearer admin token

### 请求（Request）

- Headers: `Accept: application/json`, `Authorization: Bearer <token>`
- Path: `node_id`

### 响应（Response）

- Success:
  ```json
  {
    "node_id": "01H...",
    "node_name": "node-1",
    "access_host": "example.com",
    "api_base_url": "https://127.0.0.1:62416"
  }
  ```
- Error: `BackendErrorResponse`

### 兼容性与迁移（Compatibility / migration）

- 破坏性变更：`public_domain` 字段移除，统一使用 `access_host`。

## AdminNodePatch（PATCH /api/admin/nodes/:node_id）

- 范围（Scope）: internal
- 变更（Change）: Modify
- 鉴权（Auth）: Bearer admin token

### 请求（Request）

- Headers: `Accept: application/json`, `Authorization: Bearer <token>`, `Content-Type: application/json`
- Body:
  ```json
  {
    "node_name": "node-1",
    "access_host": "example.com",
    "api_base_url": "https://127.0.0.1:62416"
  }
  ```

### 响应（Response）

- Success: 同 `AdminNode` schema
- Error: `BackendErrorResponse`

### 兼容性与迁移（Compatibility / migration）

- 破坏性变更：`public_domain` 字段移除，不再接受 `public_domain` 写入。
