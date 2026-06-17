# Xray 业务入站 Stale TCP Reclaim 与升级落地（#t8m4q）

## 状态

- Status: 待实现
- Created: 2026-06-16
- Last: 2026-06-16

## 背景 / 问题陈述

- 现网节点的业务 TCP 会话在对端异常消失、WAN 状态切换或长时间半断开后，可能在 Xray / 内核侧停留过久，影响入站资源回收。
- 当前静态 Xray 基线只打开用户统计，没有固化 reclaim 相关 policy；动态业务 inbound 也没有显式 `sockopt`。
- 旧节点仅升级 `xp` 时，静态 `/etc/xray/config.json` 不会自动补齐新默认值，导致 rollout 不完整。

## 目标 / 非目标

### Goals

- 固化一套对全部现有业务连接生效的 stale TCP reclaim 默认值。
- 仅对动态业务 inbound 注入 `sockopt` reclaim 默认，避免误伤控制面 loopback listener。
- 让旧节点通过一次 `xp-ops upgrade` 同步完成 `xp-ops` / `xp` 升级、静态 Xray config 收敛与 `xray` 重启。
- 以共享测试机真实 Xray 验证作为 merge-ready 前的硬门槛。

### Non-goals

- 不修改业务 endpoint 协议、鉴权、订阅输出、quota 语义或 admin/Web API。
- 不给 `api` 或 `mesh-proxy` 静态 listener 增加 `sockopt`。
- 不新增 ADR；本次结论沉淀到 spec 与 current-truth docs。

## 需求（Requirements）

### MUST

- `xp-ops init` 生成的 `/etc/xray/config.json` 必须在 `policy.levels.0` 中同时写入：
  - `handshake=4`
  - `connIdle=300`
  - `uplinkOnly=2`
  - `downlinkOnly=5`
  - `statsUserUplink=true`
  - `statsUserDownlink=true`
  - `statsUserOnline=true`
- `xp` 动态下发业务 inbound 时，必须只对以下 endpoint 注入固定 `socket_settings`：
  - `VlessRealityVisionTcp`
  - `Ss2022_2022Blake3Aes128Gcm`
- 业务 inbound `socket_settings` 必须固定为：
  - `tcp_keep_alive_idle=300`
  - `tcp_keep_alive_interval=30`
  - `tcp_user_timeout=10000`
- `xp-ops upgrade` 必须先锁定 target release；若 `xp-ops` 需要升级，则先升级自身，再由新二进制继续完成后半段。
- 后半段必须升级 `xp`、备份并重写 `/etc/xray/config.json`、重启 `xray`。
- `xray` restart 失败时，必须恢复旧 config，并做一次 rollback restart 尝试，不能把节点留在半收敛状态。
- 共享测试机必须在同一次隔离 run 里顺序执行 `tests/xray_e2e` 与 `tests/shared_quota_xray_e2e` 的 ignored real-Xray 用例。

### SHOULD

- 相关 dev/e2e Xray fixtures 与运维文档应与新静态基线保持完全一致。
- 升级 dry-run 应明确展示“重写静态 xray config + 重启 xray”的动作。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- `xp-ops init` 写出包含 reclaim defaults 的静态 Xray 基线。
- `xp` 通过 AddInbound 下发业务 listener 时，自动附带业务专用 `socket_settings`。
- `xp-ops upgrade` 先锁定 release；若需要自升级，则安装新 `xp-ops` 并 re-exec 原命令；恢复后完成 `xp` 升级、静态 config rewrite、`xray` restart。
- `xray` 重启后依赖现有 `down -> up` full reconcile，重建全部业务 inbound，并把新的 `sockopt` 一次性带上。

### Edge cases / errors

- `api` 与 `mesh-proxy` 必须继续保持 loopback 控制面语义，不参与 stale WAN session 修复。
- `xray` restart 失败且旧 config 能恢复时，upgrade 必须失败退出，但节点应回到旧 config。
- 若恢复旧 config 后 rollback restart 仍失败，也必须明确报失败，不得吞错。

## 验收标准（Acceptance Criteria）

- Given `xp-ops init`，When 写出 `/etc/xray/config.json`，Then `policy.levels.0` 同时包含四个 reclaim timeout 与三项 `statsUser*` 开关。
- Given VLESS REALITY 或 SS2022 业务 endpoint，When 构建 AddInbound 请求，Then `socket_settings` 精确包含 `300 / 30 / 10000`。
- Given `api` 或 `mesh-proxy` 静态 listener，When 本次变更完成，Then 它们的 listener 范围与无 `sockopt` 语义保持不变。
- Given 旧版本节点执行 `xp-ops upgrade`，When upgrade 成功，Then 新 `xp`、新静态 config 与 `xray` restart 都已完成。
- Given `xray` restart 失败，When upgrade 返回错误，Then 原静态 config 已恢复，并执行过一次 rollback restart 尝试。
- Given 共享测试机隔离 run，When 顺序执行 `cargo test --test xray_e2e -- --ignored` 与 `cargo test --test shared_quota_xray_e2e -- --ignored`，Then reconcile/grant、流量 roundtrip、quota ban/unban 等现有行为不回归。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认所有现有业务用户都通过 Xray `level 0` 下发。
- 已确认 `api` 与 `mesh-proxy` 只承载 loopback 控制面流量。
- 已确认 `xp` 在 `xray down -> up` 后具备 full reconcile 能力。

## 非功能性验收 / 质量门槛

- `cargo test`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- 共享测试机真实 Xray 验证通过后，才能视为 `merge-ready / Step 5C Ready`。

## 文档更新

- `docs/desgin/xray.md`
- `docs/ops/README.md`
- `docs/specs/6e7e4-node-user-inbound-ip-usage/contracts/cli.md`

## 实现里程碑（Milestones）

- [ ] M1: 静态 Xray 基线与仓内 fixtures 对齐 reclaim defaults
- [ ] M2: 业务 inbound `socket_settings` 默认值落地并有单元测试
- [ ] M3: `xp-ops upgrade` 两阶段 rollout + xray config rollback 落地
- [ ] M4: shared testbox real-Xray 验证与 current-truth docs 同步

## 风险与开放问题

- `xp-ops` 自升级后的 re-exec 需要保持 release 锁定语义，避免“latest”在中途漂移。
- shared testbox real-Xray 验证依赖远端 Docker/LXC 能力稳定可用。

## 假设

- 现网 rollout 接受通过 `xp-ops upgrade` 触发一次 `xray` restart。
