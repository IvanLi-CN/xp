# Config Contracts（#0013）

本文件用于冻结运维 CLI 在本地落盘的 Cloudflare Tunnel 配置/密钥存储形状与路径约定（便于实现阶段落地与排障）。

## Storage locations

约定：`xp-ops` 以 root 运行；Cloudflare Tunnel provisioning 所需的配置与凭据默认由 root 管理并落盘在固定目录下（不复用 `XP_DATA_DIR`，避免与 `xp` 运行时数据混杂）。

默认路径（normative）：

- Settings (non-secret): `/etc/xp-ops/cloudflare_tunnel/settings.json`
- Cloudflare API token (secret): `/etc/xp-ops/cloudflare_tunnel/api_token`
- Tunnel credentials file (secret): `/etc/cloudflared/<tunnel-id>.json`
- Cloudflared config file (non-secret): `/etc/cloudflared/config.yml`

## File permissions (normative)

- `api_token` 与 tunnel credentials file MUST 为 `0600`，且不得被日志打印。
- `settings.json` 可读权限不应放宽到“所有用户可读”，避免泄露集成状态与内网 origin 信息（建议 `0640`，owner/group 由实现阶段定案）。

## Settings schema

`settings.json`：

```jsonc
{
  "enabled": true,
  "install_mode": "external",
  "origin_url": "http://127.0.0.1:62416",
  "account_id": "699d98642c564d2e855e9661899b7252",
  "zone_id": "023e105f4ecef8ad9ca31a8372d0c353",
  "hostname": "app.example.com",
  "tunnel_id": "c1744f8b-faa1-48a4-9e5c-02ac921467fa",
  "dns_record_id": "372e67954025e0ba6aaa6d586b9e0b59"
}
```

约束：

- `enabled` MUST 为 boolean：
  - `true` 表示“启用 Cloudflare Tunnel”（应安装/配置/启用并启动 `cloudflared` 服务）。
  - `false` 表示“禁用 Cloudflare Tunnel”（不要求 Cloudflare 侧资源存在；本机不应启用/启动 `cloudflared` 服务）。
- `origin_url` MUST 为本机可访问的 HTTP URL（如 `http://127.0.0.1:62416`）。
- `tunnel_id` 与 `dns_record_id` 用于幂等更新/重跑 provision；若缺失则视为“首次创建”。

## Cloudflared runtime files

### `/etc/cloudflared/<tunnel-id>.json` (secret)

`xp-ops` MUST 将 Cloudflare API 的 `result.credentials_file` 以原样 JSON 落盘为该文件。

示例：

```jsonc
{
  "AccountTag": "699d98642c564d2e855e9661899b7252",
  "TunnelID": "c1744f8b-faa1-48a4-9e5c-02ac921467fa",
  "TunnelName": "api-tunnel",
  "TunnelSecret": "bTSquyUGwLQjYJn8cI8S1h6M6wUc2ajIeT7JotlxI7TqNqdKFhuQwX3O8irSnb=="
}
```

### `/etc/cloudflared/config.yml` (non-secret)

最小形状（用于让 `cloudflared` 以 credentials file 方式运行；避免把 token 暴露在进程参数中）：

```yaml
tunnel: c1744f8b-faa1-48a4-9e5c-02ac921467fa
credentials-file: /etc/cloudflared/c1744f8b-faa1-48a4-9e5c-02ac921467fa.json
```

## Token handling rules

- 运维 CLI **允许**将 Cloudflare API token 以 root-only 形式落盘保存（见 `api_token`），便于重复执行 provisioning 与状态检查；实现阶段需要提供“写入/覆盖/清除/轮换”的明确命令与审计友好输出（不泄露明文）。
- 运维 CLI MUST 将“创建 tunnel API”返回的 `credentials_file` 写入 `/etc/cloudflared/<tunnel-id>.json` 供 `cloudflared` 使用；并生成 `/etc/cloudflared/config.yml`（引用 `tunnel` 与 `credentials-file`）。
- 若清理/重建 tunnel，应同步清理 `tunnel_id`、`dns_record_id` 与 `/etc/cloudflared/<tunnel-id>.json`。
