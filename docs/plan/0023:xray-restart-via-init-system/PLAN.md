# Xray 故障自动重启（通过 init system 间接拉起）（#0023）

## 状态

- Status: 已完成
- Created: 2026-01-23
- Last: 2026-01-23

## 背景 / 问题陈述

- 当前 `xp` 已具备 `xray` gRPC 探活与健康状态输出（见 #0021）。
- 但仅“观测 + Down/Up 边沿触发 reconcile”不足以满足运维目标：当 `xray` 进程仍存活但 gRPC 不可用、或 `xray` 持续异常时，需要能**强制重启 `xray`**以恢复服务。
- 约束：`xp` **不托管/不直接拉起** `xray` 子进程；不得引入额外常驻辅助程序；`xray` 由 `xp-ops` 部署，`xp` 作为 `xp` 用户运行。

## 目标 / 非目标

### Goals

- 当 `xp` 判定 `xray` 为 Down（达到阈值）时，`xp` MUST 通过 init system **间接触发** `xray` 重启（强制恢复工作）。
- `xp` 作为 `xp` 用户运行时，仍能触发上述重启（通过系统策略授权，而非提权常驻程序）。
- 必须有节流/冷却机制，避免 `xray` 持续故障导致重启风暴与日志刷屏。
- `xp-ops` 部署流程 MUST 默认启用并配置好所需权限与运行方式（从全局视角“一键可用”）。

### Non-goals

- `xp` 不负责 `fork/exec` 托管 `xray` 子进程（不做进程树守护）。
- 不引入新的 daemon/sidecar/helper。
- 不实现跨节点的故障编排（仅本机）。

## 用户与场景

- 场景 A：`xray` 进程仍在，但 gRPC API 挂掉/卡死；`xp` 能检测到并触发重启。
- 场景 B：`xray` 崩溃/被 OOM killer 杀死；init system 自动重启，同时 `xp` 能在恢复后触发 full reconcile。
- 场景 C：`xray` crash-loop；`xp` 不忙等、不刷屏，并保证重启请求被节流。

## 需求（Requirements）

### MUST

- MUST: `xp` MUST 支持 “重启策略”配置（默认由 `xp-ops` 设定），至少包括：
  - `systemd`：通过 `systemctl restart xray.service`
  - `openrc`：通过 `doas -n rc-service xray restart`（或等价方式；由 `xp-ops` 配置权限）
- MUST: 当 `xray` 进入 Down 时，`xp` MUST 尝试触发一次 restart（并记录日志）。
- MUST: `xp` MUST 对 restart 做冷却（cooldown）与节流（例如：最短间隔、最大频率），避免重启风暴。
- MUST: 当 restart 命令执行失败（权限/缺少命令/返回非 0）时，`xp` MUST 记录可定位信息，并继续探活与重试（仍受节流约束）。
- MUST: `xp-ops` MUST 保证服务运行用户：
  - `xp` service 以 `xp:xp` 运行；
  - `xray` service 以 `xray:xray`（或目标发行版的推荐用户）运行；
  - 且为 `xp` 用户配置“仅允许重启 xray”的最小权限。

## 验收标准（Acceptance Criteria）

### Core path (systemd)

- Given: `xp` 以 `xp` 用户运行，`xray` 由 systemd 管理；When: `xray` gRPC 不可用且达到 Down 阈值；Then:
  - `xp` 在进入 Down 后的一个探活周期内触发 `systemctl restart xray.service`（受冷却约束）；
  - `xray` 恢复 gRPC 可用后，`xp` 仍按 #0021 的行为触发一次 `reconcile.request_full()`；
  - `xp` 无需 sudo/root 即可完成上述重启触发（由部署时配置的 system policy 授权）。

### Core path (OpenRC)

- Given: `xp` 以 `xp` 用户运行，`xray` 由 OpenRC 管理；When: `xray` gRPC 不可用且达到 Down 阈值；Then:
  - `xp` 触发 `doas -n rc-service xray restart`（或等价命令），无需交互输入密码；
  - `xray` 恢复后触发 full reconcile；
  - 重启频率受冷却/节流约束。

### Edge cases

- crash-loop：`xp` 的 restart 触发应被节流，日志不刷屏。
- 权限缺失：`xp` 记录清晰错误并继续探活（不 panic、不退出主循环）。

## 测试与质量门槛（Quality Gates）

- Rust：必须新增/扩展测试覆盖：
  - restart 触发逻辑（Down 时触发一次；冷却生效）。
  - “Down -> Up 触发 reconcile”不被 restart 逻辑破坏。
- 必须通过：`cargo test`、`cargo fmt`、`cargo clippy -- -D warnings`。

## 文档更新（Docs to update）

- `docs/ops/README.md`：明确自动重启机制与权限边界。
- `docs/ops/systemd/*`：补充 systemd 下的最小权限（polkit）说明。
- `docs/ops/openrc/*`：补充 OpenRC 下的最小权限（doas）说明。

## 里程碑（Milestones）

- [x] M1: `xp` 支持 restart 策略 + 冷却/节流
- [x] M2: `xp-ops` 部署：写入 systemd/OpenRC 权限配置（polkit/doas）+ 默认启用
- [x] M3: 测试与文档补齐

## Change log

- 2026-01-23: `xp` 在 `xray` 标记为 down 后通过 init system 触发重启（systemd/OpenRC），并由 `xp-ops` 写入最小权限策略与默认配置；新增自动化测试与运维文档说明。
