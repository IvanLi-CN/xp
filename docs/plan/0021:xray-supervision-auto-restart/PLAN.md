# Xray 探活与恢复信号（Down/Up + health + 触发 reconcile）（#0021）

## 状态

- Status: 已完成
- Created: 2026-01-21
- Last: 2026-01-23

## 背景 / 问题陈述

- `xp` 的 reconcile 与配额统计依赖本机 `xray` 的 gRPC API（`HandlerService` / `StatsService`）。
- 当 `xray` 异常退出或 gRPC 不可用时：
  - 控制面无法把“期望状态”及时落到 `xray` 运行态；
  - 配额统计与封禁会持续失败或退避；
  - 若 `xray` 未被外部 supervisor 拉起，则数据面会长期不可用。
- 已观察到的典型故障原因之一：`xray` 进程在内存压力下被 OOM killer 终止。
- 需要让 `xp` 能够**及时发现 `xray` 不可用**，并在 `xray` 恢复后尽快触发一次 full reconcile；重启由 init system 负责（见“约束与风险”与“实现前置条件”）。

## 目标 / 非目标

### Goals

- 当 `xray` 退出或 gRPC 不可用时，`xp` 能快速检测并进入恢复流程。
- `xray` 的重启由 init system 负责（systemd/OpenRC）；`xp` 不直接托管 `xray` 进程。（“由 `xp` 间接触发重启”的能力见 #0022）
- 在 `xray` 恢复后，`xp` 必须立即触发一次 `reconcile.request_full()`，不依赖 30s 兜底周期。
- 提供最小可观测性：
  - 结构化日志（状态切换、失败原因、恢复次数（down->up）、上次成功时间等）；
  - `/api/health` 中提供 `xray.status` 等可监控字段（见契约）。
- 不引入外部依赖服务；保持资源约束与运行稳定性。

### Non-goals

- 不改变 Xray 的协议实现与配置语义（仍以“基础配置静态 + 动态注入入站/用户”为主）。
- 不实现跨节点的 `xray` 自愈编排（仅处理本机 `xray`）。
- 不在本计划中引入完整 metrics/alerting 系统（仅提供最小 health 信号与日志）。
- 不引入或升级依赖（按仓库现有依赖完成）。

## 用户与场景

- 运维人员在单机或集群节点上长期运行 `xp + xray`：
  - 场景 A：`xray` 崩溃/被 OOM killer 终止，希望自动恢复并尽快重建运行态。
  - 场景 B：`xray` 启动中或短暂故障导致 gRPC 不可用，希望 `xp` 合理退避并在恢复时立即 reconcile。
  - 场景 C：`xray` 持续 crash-loop（如配置损坏），希望 `xp` 在故障期间避免忙等与日志刷屏，并持续给出可定位信号。

## 需求（Requirements）

### MUST

- MUST: `xp` MUST 周期性探活本机 `xray`（以“gRPC 连接 + 轻量调用”为准），并维护一个内部的 `xray` 在线状态机（Up/Down/Unknown）。
- MUST: 当探活连续失败达到阈值（可配置）时，`xp` MUST 将 `xray.status` 置为 `down` 并记录 `down_since`。
- MUST: `xp` MUST 避免“`xray` down 期间的日志刷屏”：默认只在状态切换（unknown↔up↔down）或达到节流阈值时记录 warn/error。
- MUST: `xray` 从 Down → Up 的首次恢复 MUST 触发一次 `reconcile.request_full()`（不等待周期性兜底）。
- MUST: `xp` MUST 不负责启动/拉起 `xray` 进程；`xray` 的启动与重启由 init system 负责。
- MUST: `GET /api/health` MUST 维持兼容（仍返回 `{"status":"ok"}`），并在此基础上**增量**加入 `xray` 字段用于监控（见契约）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）        | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）                           | 负责人（Owner） | 使用方（Consumers）  | 备注（Notes）                  |
| ------------------- | ------------ | ------------- | -------------- | -------------------------------------------------- | --------------- | -------------------- | ------------------------------ |
| `xp` runtime config | CLI / Config | external      | Modify         | [contracts/cli.md](./contracts/cli.md)             | core            | operators            | 探活参数（interval/threshold） |
| `GET /api/health`   | HTTP API     | external      | Modify         | [contracts/http-apis.md](./contracts/http-apis.md) | core            | monitors / operators | 追加 `xray.*` 字段（向后兼容） |

## 约束与风险（Constraints & Risks）

约束：

- `xp` 常驻内存目标：≤32MiB（RSS，不含 `xray`）；新增监测/恢复逻辑不应引入重型常驻结构。
- 运行环境覆盖：Arch/Debian/Alpine（systemd/OpenRC 皆存在的现实）；但本计划默认以“无外部依赖”实现为优先。
- `xp` 仅能直接访问 `XP_XRAY_API_ADDR` 指向的本机 gRPC service；不引入跨节点操作。

风险：

- `xray` crash-loop：需要明确节流/采样策略，否则会造成日志膨胀与资源抖动。
- 与外部 supervisor 的职责重叠：本计划明确由 init system 托管 `xray`，`xp` 不拉起 `xray`，避免“双启动/端口冲突”。
- 仅凭 gRPC 探活可能误判（例如 `xray` 进程存在但 gRPC 配置错误）；需要在验收中覆盖该场景并明确行为（持续重试/停止重试/告警）。

## 验收标准（Acceptance Criteria）

### Core path

- Given: `xp` 正常运行，且 `xray` 处于 Up；When: `xray` 进程异常退出导致 gRPC 不可用；Then:
  - 在推荐配置（`XP_XRAY_HEALTH_INTERVAL_SECS=2`、`XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=3`）下，`xp` 在 `10s` 内将 `xray.status` 标记为 `down`；
  - `xray` 在 init system 的托管下重启，并在 `10s` 内恢复 gRPC 可用性；
  - `xray` 恢复可用后（gRPC 可用），`xp` 在 `10s` 内触发一次 full reconcile；
  - `/api/health` 的 `xray.status` 从 Down 变为 Up，并更新 `last_ok_at`。

### Edge / failure cases

- Given: `xray` 持续 crash-loop；When: init system 反复重启但 `xray` 仍无法稳定提供 gRPC；Then:
  - `xp` 不会出现忙等（无高频循环/高频日志刷屏）；
  - `/api/health` 能持续反映 `xray.status=down`，并提供足够定位的信息（至少包含失败计数与最近一次失败时间）。
- Given: `xray` 由外部 supervisor（systemd/OpenRC）管理且会自动重启；When: `xray` 重启；Then:
  - `xp` 能在 `xray` 恢复后的 `10s` 内触发 full reconcile（由 Up 边沿触发）。

## 测试与质量门槛（Quality Gates）

- Rust：
  - 必须新增单元测试覆盖：状态机转换、Up 边沿触发 reconcile 的逻辑。
  - 必须新增集成测试（或等价测试）覆盖：模拟 `xray` gRPC 服务不可达 → 可达 → `xp` 状态变更 + Up 边沿触发 reconcile。
  - 必须通过：`cargo test`、`cargo fmt`、`cargo clippy -- -D warnings`。
- Web：None（本计划不涉及 web 交互）。

## 项目文档更新（Docs to update）

- `docs/desgin/api.md`：更新 `GET /api/health` 的响应体说明（保留 `{ "status": "ok" }`，增量追加 `xray.*`）。
- `docs/desgin/workflows.md`：补充“xray Down/Up 边沿触发 reconcile”的口径与流程图/文字说明。
- `docs/ops/README.md`：补充新的 `xp` 环境变量/参数说明与推荐部署方式（与 systemd/OpenRC 的职责边界）。
- `docs/ops/systemd/xray.service`：确认并明确推荐的 `Restart=` 策略（目标：OOM/崩溃可自动恢复；不引入 crash-loop 忙等）。
- `docs/ops/openrc/xray`：若 OpenRC 环境需要自动恢复，补齐 supervise/重启策略的推荐写法（仅文档示例层面）。

## 实现前置条件（Definition of Ready）

- 已确认：`xray` 由 init system 托管并负责自动重启；`xp` 不托管/不拉起 `xray` 进程。
- 已确认：“及时”的目标为 `10s`（Down 检测 + init system 重启恢复 gRPC + Up 后触发 reconcile）。
- 已确认：允许在 `GET /api/health` 中增量追加 `xray.*` 字段（保持现有 `status` 不变）。

## 开放问题（Open Questions）

None

## 假设（Assumptions）

- 假设：探活以 `XP_XRAY_API_ADDR` 为准，且 `xray` 基础配置确保 gRPC API 与该地址一致（见 `docs/desgin/xray.md`）。
- 假设：`/api/health` 只做**状态输出**，不在请求路径上做实时 gRPC 探活（避免阻塞与放大故障）。

## 里程碑（Milestones）

- [x] M1: `xray` 探活与状态机（Up/Down/Unknown）+ Up 边沿触发 reconcile
- [x] M2: 完善 ops 文档：systemd/OpenRC 的自动重启建议与边界条件
- [x] M3: 测试与文档补齐（单测/集测 + ops/workflows 文档更新）

## Change log

- 2026-01-23: 增加 `xray` 探活状态机与 `/api/health` 的 `xray.*` 字段；`down -> up` 触发 full reconcile；补齐 ops 与设计文档。

## 方案概述（Approach, high-level）

- 在 `xp` 运行时新增一个 `XraySupervisor`（或等价模块）：
  - 以固定间隔执行轻量探活（connect + 1 次 cheap call）。
  - 维护一个可共享的 `XrayHealth` 状态（供日志与 HTTP health 输出）。
  - 在 Down 状态时继续探活并做日志节流（避免刷屏）。
  - 在 Up 边沿触发：`reconcile.request_full()`（用于绕过 reconcile 退避与周期兜底）。
- `/api/health` 读取 `XrayHealth` 的**缓存状态**并输出，不在 handler 中直接做网络 I/O。

## 参考（References）

- 现状入口：`src/main.rs`（`run_server`：spawn reconcile/quota/http）
- reconcile：`src/reconcile.rs`（xray 不可用时的退避）
- quota：`src/quota.rs`（xray stats 读取失败的行为）
- health endpoint：`src/http/mod.rs`（`GET /api/health`）
- Xray 集成约束：`docs/desgin/xray.md` / `docs/desgin/workflows.md`
- 运维服务示例：`docs/ops/systemd/xray.service` / `docs/ops/openrc/xray`
