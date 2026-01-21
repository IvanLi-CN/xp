# 文件格式（File formats）

## Deploy settings（`/etc/xp-ops/deploy/settings.json`）

- 范围（Scope）: external
- 变更（Change）: Modify
- 编码（Encoding）: utf-8（JSON，pretty-printed，末尾换行）

### Schema（结构）

JSON object（字段均为可选；缺失表示使用 TUI 默认值或运行时推导）：

- `node_name`: string
- `access_host`: string
  - 兼容别名：`public_domain`
- `cloudflare_enabled`: boolean
- `account_id`: string
- `zone_id`: string
- `hostname`: string
- `origin_url`: string
- `api_base_url`: string
- `xray_version`: string（例如 `latest` 或 semver）
- `enable_services`: boolean

注意：

- Cloudflare token **不写入**本文件；token 持久化使用 `/etc/xp-ops/cloudflare_tunnel/api_token`（敏感文件）。
- `save_token`（历史字段）若存在将被忽略；TUI 不再提供该开关，token 与其他字段一样参与保存链路。
- `xp` 二进制路径不在本文件中配置：`xp-ops tui` 默认使用 `/usr/local/bin/xp`（可用发布安装脚本完成初次安装）。

### Examples（示例）

```json
{
  "node_name": "node-1",
  "access_host": "node-1.example.net",
  "cloudflare_enabled": true,
  "account_id": "123",
  "zone_id": "456",
  "hostname": "node-1.example.com",
  "origin_url": "http://127.0.0.1:62416",
  "xray_version": "latest",
  "enable_services": true
}
```

### 兼容性与迁移（Compatibility / migration）

- 兼容读取旧字段 `public_domain`（映射到 `access_host`）。
- 本计划变更点：
  - “写入时机”从手动保存扩展到 deploy 前自动保存；
  - `save_token` 被弃用并忽略（不再写出）。

## Cloudflare API token（`/etc/xp-ops/cloudflare_tunnel/api_token`）

- 范围（Scope）: external
- 变更（Change）: Modify
- 编码（Encoding）: utf-8（文本；写入前会 trim；不要求末尾换行）
- 权限（Permissions）: `0600`（root-managed）

### 写入规则（Write rules）

- 当 `xp-ops tui` 执行保存（显式保存或 deploy 前 autosave）时：
  - 若 token 输入框非空：写入该文件（并设置权限为 `0600`）。
  - 若 token 输入框为空：保持现有 token 不变（不清空/不删除该文件）。

### Examples（示例）

```
<CLOUDFLARE_API_TOKEN>
```
