# OpenRC low-memory runtime recovery (#r7m2q)

## 状态

- Status: 已完成

## 背景 / 问题陈述

Alpine/OpenRC 节点可能运行在 `256MB` 且无 swap 的小容量服务器上。`xp + xray + cloudflared` 在资源抖动时容易出现短探活误判、重复 `rc-service restart`、`supervise-daemon` 残留多实例，最终导致 `xp` HTTP 端口能接 TCP 但不返回响应。

本规格延续 legacy `docs/plan/0021:xray-supervision-auto-restart/PLAN.md`、`docs/plan/0023:xray-restart-via-init-system/PLAN.md` 与 `docs/plan/ma8jj:cloudflared-openrc-supervision/PLAN.md` 的运行时自愈主题，并把小内存节点作为产品默认兼容目标。

## 目标 / 非目标

### Goals

- 在 `256MB` 无 swap OpenRC 节点上，xray 首次确认 down 后仍能在 `30-60s` 内触发自动恢复。
- 连续恢复失败时使用指数退避，避免固定频率 restart storm。
- cloudflared 默认可监控，但默认不由 `xp` 主动 restart，避免 Tunnel 层重复拉起。
- `/api/health` 保持向后兼容，并追加 restart/backoff 与 cloudflared 状态信息。

### Non-goals

- 不改变 `/api/health` 始终返回 HTTP 200 的存活语义。
- 不引入完整 metrics/alerting 系统。
- 不直接修改任何线上节点配置；线上 rollout 由运维步骤单独执行。
- 不让自动恢复永久硬熔断；持续失败只退避。

## 行为规格

- xray 探活默认值调整为 `XP_XRAY_HEALTH_INTERVAL_SECS=5`、`XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=4`，默认 down 判定约 `20s`，首轮 restart 在下一个 supervisor tick 内触发。
- xray restart timeout 默认 `20s`，避免 OpenRC 小机上 `rc-service restart` 在 5 秒内未完整收尾时被过早判失败。
- restart 退避以 `XP_XRAY_RESTART_COOLDOWN_SECS` / `XP_CLOUDFLARED_RESTART_COOLDOWN_SECS` 为初始间隔，连续 down 时按指数增长并封顶 `300s`；探活恢复 up 后重置退避。
- OpenRC restart 前后审计同服务 `supervise-daemon` 与精确可执行路径匹配的 worker 进程；发现重复实例时记录 warning，并只在 pidfile 能确认 active supervisor 时自动清理其他 `supervise-daemon <service>` PID；标准 host-managed 安装通过 `xp-openrc-kill-supervisor` helper 清理 root-owned supervisor 残留，避免授予 `xp` 任意 root kill 权限。
- cloudflared 新增 monitor-only 语义：`XP_CLOUDFLARED_MONITOR_MODE` 控制状态探测，`XP_CLOUDFLARED_RESTART_MODE` 仅控制是否主动 restart；旧部署只设置 restart mode 时继续按该 mode 监控，保持向后兼容；显式 `XP_CLOUDFLARED_MONITOR_MODE=none` 优先表示关闭监控。
- host-managed env backfill 默认写入 cloudflared monitor mode 为当前 init system，restart mode 为 `none`。

## 验收标准

- `GET /api/health` 继续包含 `status: "ok"` 与既有 `xray.*` 字段。
- `xray` 和 `cloudflared` health 均暴露 restart attempts、last restart、next restart、backoff 和 automatic restart enabled 状态。
- xray 连续失败后首轮 restart 不等待退避间隔；后续 restart 间隔按指数退避。
- OpenRC 进程审计只按当前服务的 `supervise-daemon <service>` 执行自动清理；找不到 active supervisor pidfile 时只告警不清理；worker 进程仅告警，避免误伤使用同一可执行路径的 operator-managed 实例。
- 旧 `XP_CLOUDFLARED_RESTART_MODE=openrc|systemd` 配置在缺少 `XP_CLOUDFLARED_MONITOR_MODE` 时仍保持 cloudflared 监控。
- 旧 `XP_CLOUDFLARED_RESTART_MODE=none` 配置在缺少 `XP_CLOUDFLARED_MONITOR_MODE` 时保持 opt-out，不由 env backfill 自动打开监控。
- cloudflared monitor-only 模式可报告 down，但不会调用 restarter。
- 单元测试覆盖默认值、退避、monitor-only 与 health contract。

## 文档更新

- `docs/desgin/api.md`
- `docs/ops/README.md`
- `docs/ops/openrc/xp`

## 里程碑

- [x] M1: 低内存默认值与 monitor-only 配置口径
- [x] M2: xray/cloudflared 指数退避 restart 状态机
- [x] M3: `/api/health` 兼容扩展与文档同步
