# 文件格式（File formats）

## logrotate config（`/etc/logrotate.d/xp-ops`）

- 范围（Scope）: external
- 变更（Change）: New
- 编码（Encoding）: utf-8

### Schema（结构）

该文件只包含“本项目日志文件”的规则块；不得包含全局通配（例如 `/var/log/*.log`）。

默认策略（normative）：

- `daily`
- `maxsize 100M`（等价于 `100MiB`）
- `maxage 7`
- 不启用 `compress`（轮换文件不压缩）

其他建议项（是否默认启用由本计划冻结）：

- `missingok`
- `notifempty`
- 处理 reopen：默认使用 `copytruncate`（避免依赖服务的 reopen 机制）。

### Examples（示例）

示例仅表达结构，具体日志路径与指令以实现冻结为准：

```text
/var/log/xray/access.log /var/log/xray/error.log {
  daily
  maxsize 100M
  maxage 7
  missingok
  notifempty
  copytruncate
}
```

### 兼容性与迁移（Compatibility / migration）

- 文件名固定为 `xp-ops`，以便幂等更新与回滚删除。
- 规则块必须可增量扩展（例如未来新增 `xp`/`cloudflared` 目标），不得破坏既有路径的轮换行为。
- 当 `xp-ops init --disable-log-rotation`：该文件必须被移除（若不存在则 no-op）。

## project-local runner（required）

本计划要求由 `xp-ops` 安装并启用“本项目专用 runner”，以避免依赖系统全局 logrotate 调度的存在与启用状态。

SCHEDULE 映射（normative）：

- `daily`：
  - systemd: `OnCalendar=daily`
  - Alpine: `/etc/periodic/daily/xp-ops-logrotate`
- `weekly`：
  - systemd: `OnCalendar=weekly`
  - Alpine: `/etc/periodic/weekly/xp-ops-logrotate`
- `monthly`：
  - systemd: `OnCalendar=monthly`
  - Alpine: `/etc/periodic/monthly/xp-ops-logrotate`

### systemd（Arch/Debian）

- unit files:
  - `/etc/systemd/system/xp-ops-logrotate.service`
  - `/etc/systemd/system/xp-ops-logrotate.timer`
- `xp-ops-logrotate.service`（oneshot）必须等价执行：
  - `logrotate -s /var/lib/xp-ops/logrotate.status /etc/logrotate.d/xp-ops`
  - 其中 `logrotate` 可执行文件路径必须在写入单位文件前被解析为绝对路径（避免 PATH 差异）。
- `xp-ops-logrotate.timer`：
  - `OnCalendar=daily`（可由 CLI 参数调整）
  - `Persistent=true`
  - `Unit=xp-ops-logrotate.service`

### Alpine periodic（OpenRC）

- periodic script:
  - `/etc/periodic/<schedule>/xp-ops-logrotate`
- script 必须只运行本项目配置：
  - `logrotate -s /var/lib/xp-ops/logrotate.status /etc/logrotate.d/xp-ops`

- runner 只能执行本项目的 logrotate 配置（例如只运行 `/etc/logrotate.d/xp-ops`）。
- runner 必须使用独立的 status 文件（不得复用 `/var/lib/logrotate/status`）。
- 当 `xp-ops init --disable-log-rotation`：
  - systemd：必须 stop/disable timer，并移除上述 unit 文件
  - Alpine：必须移除 `/etc/periodic/<schedule>/xp-ops-logrotate`
  - 必须移除 `/var/lib/xp-ops/logrotate.status`
