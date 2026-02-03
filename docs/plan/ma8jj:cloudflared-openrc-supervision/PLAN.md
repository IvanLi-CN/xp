# cloudflared OpenRC 监督与自动重启（#ma8jj）

## 状态

- Status: 待实现
- Created: 2026-02-03
- Last: 2026-02-03

## 背景 / 问题陈述

- Alpine/OpenRC 环境下 `cloudflared` 作为 Cloudflare Tunnel 的常驻进程；若异常退出，会导致远程访问中断。
- 当前 OpenRC 的 `cloudflared` 服务脚本未对齐 `xray` 的 supervise/自动重启策略，存在“异常退出后无人拉起”的风险。
- 需要把 OpenRC `cloudflared` 服务脚本对齐为 `supervise-daemon + respawn_delay`，避免 crash-loop 忙等并提升自愈能力。

## 目标 / 非目标

### Goals

- 在 Alpine/OpenRC 上，`cloudflared` 由 `supervise-daemon` 托管并自动重启（带 backoff），对齐 `xray` 的策略。
- 生成的 OpenRC `cloudflared` 脚本不再使用 `command_background` / `pidfile`，避免与 supervisor 语义冲突。
- `docs/ops/openrc/cloudflared` 示例脚本同步更新为一致口径。

### Non-goals

- 不新增 `cloudflared` 运行态探活 / health endpoint（仍由 init system 负责进程托管）。
- 不调整 systemd 的 `cloudflared.service`（其已具备 Restart 策略；本计划聚焦 OpenRC）。

## 范围（Scope）

### In scope

- `src/ops/cloudflare.rs`：OpenRC `cloudflared` 脚本模板改为 `supervise-daemon` 方案。
- `docs/ops/openrc/cloudflared`：示例脚本补齐 supervise/重启策略。
- Rust 单元测试：覆盖脚本模板关键字段（包含/不包含）。

### Out of scope

- Cloudflare API provisioning 行为、配置文件格式、权限模型不变。

## 验收标准（Acceptance Criteria）

- Given 目标平台为 Alpine/OpenRC，
  When `xp-ops cloudflare provision` 生成 `cloudflared` 服务脚本，
  Then 脚本必须包含：
  - `supervisor=supervise-daemon`
  - `respawn_delay=2`
  - `respawn_max=0`
  - 且不包含 `command_background` / `pidfile`
- `docs/ops/openrc/cloudflared` 示例脚本与上述口径一致。
- 自动化验证至少包含：`cargo test`（并保持 fmt/clippy clean）。

## 风险与备注（Risks / Notes）

- `supervise-daemon` 会持续拉起进程；若配置错误导致持续崩溃，应依赖 `respawn_delay` 避免忙等并减少日志噪音。

