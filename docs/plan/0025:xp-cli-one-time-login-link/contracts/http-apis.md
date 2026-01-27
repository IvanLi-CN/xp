# HTTP API

## Admin UI 入口（GET /login）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 请求（Request）

- Query:
  - `login_token`: string（optional）一次性登录 token（JWT，短期有效；详见“管理员鉴权规则”）

### 响应（Response）

- Success: `200 text/html`（SPA `index.html`；由前端在页面加载后消费 `login_token`）
- Error: 同现状（由反代/服务端静态资源层决定；本计划不改变）

### 兼容性与迁移（Compatibility / migration）

- 不携带 `login_token` 时，行为与现状一致（显示登录页，手工输入 token）。
- 携带 `login_token` 且消费完成后，前端必须从 URL 中移除该参数（避免长期留在地址栏/历史记录中）。

## 管理员鉴权规则（ANY /api/admin/*）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: bearer（admin scope）

### 请求（Request）

- Headers:
  - `Authorization: Bearer <token>`

其中 `<token>` 允许两种形态：

1) `admin_token`（现有）：与配置的 `XP_ADMIN_TOKEN` 字符串完全匹配。
2) `login_token`（新增）：短期有效、可验证的 JWT；用于“登录链接”。

### `login_token` 形状（Schema）

编码：JWT（JWS Compact Serialization）。

Header（固定）：

- `typ`: `JWT`
- `alg`: `HS256`

Claims（payload）字段：

- `cluster_id`: string（必须等于服务端 cluster_id）
- `exp`: number（Unix timestamp seconds）
- `iat`: number（Unix timestamp seconds）
- `jti`: string（ULID）

验证规则：

- `exp > now`（未过期）
- `cluster_id` 匹配当前集群
- JWT 签名校验通过（HS256；key = `admin_token`）
  - TTL 固定为 1 小时（`exp - iat <= 3600`）

### 响应（Response）

- Success: 与各具体 `/api/admin/*` endpoint 相同
- Error: `401`（错误格式见 `docs/desgin/api.md`）

### 错误（Errors）

- `401/unauthorized`: missing or invalid authorization token（retryable: no）

### 兼容性与迁移（Compatibility / migration）

- `admin_token` 继续可用；`login_token` 仅作为补充。
- 本计划默认不保证 strict single-use（如需必须另行设计一致性与清理策略）。
