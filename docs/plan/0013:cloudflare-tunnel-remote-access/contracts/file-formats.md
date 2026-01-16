# File formats Contracts（#0013）

本文件用于冻结运维 CLI 在本机落盘的“自启动服务文件”形状与路径约定（systemd/OpenRC），以便实现阶段可以稳定生成/更新，并可被运维人员检查。

## systemd (Arch/Debian)

### Unit names

- `xp.service`（运行 xp，普通用户）
- `cloudflared.service`（运行 cloudflared，普通用户；仅当启用 Cloudflare Tunnel 时生成/启用）
- `xray.service`（运行 xray，普通用户；默认生成/启用）

### Install paths

- `/etc/systemd/system/<name>.service`

### Required unit properties (normative)

- `User=` / `Group=`：必须为普通用户（默认：
  - `xp.service` → `xp:xp`
  - `xray.service` → `xray:xray`
  - `cloudflared.service` → `cloudflared:cloudflared`
  ）
- `WorkingDirectory=`：
  - `xp.service` → `/var/lib/xp`
  - `xray.service` → `/var/lib/xray`
- `Environment=XP_DATA_DIR=...`：`xp.service` MUST 明确指定（默认 `/var/lib/xp/data`）
- `EnvironmentFile=-/etc/xp/xp.env`：`xp.service` SHOULD 支持（可选）
- `ExecStart=`：
  - `xp.service` MUST 使用 `/usr/local/bin/xp run`
  - `xray.service` MUST 使用 `/usr/local/bin/xray run -c /etc/xray/config.json`
  - `cloudflared.service` MUST 使用绝对路径并避免把敏感 token 放进命令行参数（建议：`/usr/bin/cloudflared --no-autoupdate --config /etc/cloudflared/config.yml tunnel run`）
- `Restart=always`（或等价策略），并设置合理的 `RestartSec=...`
- `After=network-online.target` / `Wants=network-online.target`（cloudflared/xray 至少需要网络）

## Xray config file（/etc/xray/config.json）

本计划要求：`xp-ops` MUST 生成 `/etc/xray/config.json` 的最小可用模板，且不得擅自开放任何入站。

约束（normative）：

- `api.listen` MUST 为 `127.0.0.1:10085`（仅回环）
- `api.services` MUST 包含 `HandlerService` 与 `StatsService`
- `inbounds` MUST 为 `[]`（业务入站由 `xp` 通过 gRPC 动态下发）
- 必须启用 `stats: {}` 与 `policy.levels.0.statsUserUplink=true`、`statsUserDownlink=true`（满足配额统计需求）

参考的基础配置形状：见 `docs/desgin/xray.md`。

## OpenRC (Alpine)

### Service names

- `xp`
- `cloudflared`（仅当启用 Cloudflare Tunnel 时生成/启用）
- `xray`（默认生成/启用）

### Install paths

- `/etc/init.d/<name>`
- `/etc/conf.d/<name>`（若使用 OpenRC 约定的配置文件；用于设置 `command_args`/`command_user` 等变量）

### Required service properties (normative)

- service 以普通用户运行（`command_user` 或等价机制）
- 具备 `depend()` 声明：至少 `need net`
- 具备 restart 策略（由 OpenRC 管理）
- `command` / `command_args`（或等价实现）必须等价于：
  - `xp`：`/usr/local/bin/xp run`
  - `xray`：`/usr/local/bin/xray run -c /etc/xray/config.json`
  - `cloudflared`：`/usr/local/bin/cloudflared --no-autoupdate --config /etc/cloudflared/config.yml tunnel run`（仅当启用 Cloudflare Tunnel）

## Update strategy

- 生成文件需幂等：重复执行 `xp-ops init --enable-services` 不应产生不可控的差异。
- 任何覆盖写入必须先备份（或提供 `--force` 明确覆盖）；实现阶段二选一并固化。
