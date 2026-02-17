# Node 全局流量：Unlimited / Monthly limit + 用量对齐（IDC）（#t9p6c）

## 状态

- Status: 待实现
- Created: 2026-02-17
- Last: 2026-02-17

## 背景 / 问题陈述

- 目前系统不能直接设置“节点全局月流量上限”，且配额强约束在部分路径上会改写用户/管理员设定（例如写 Raft 修改 `Grant.enabled`），不符合预期。
- 现实运营需要：根据 IDC 提供的统计数据，随时调整节点的“限额与用量”，并立即/尽快影响接入权限。

## 目标 / 非目标

### Goals

- Node 支持两种节点流量模式：
  - `unlimited`：不触发封禁；
  - `monthly limit`：按月窗口计算节点总用量，超过阈值硬切断并在可用后恢复。
- Web Admin UI（Node details）可随时调整：
  - 节点限额（limit）
  - 节点当前周期用量（used，绝对值 set；用于 IDC 对齐）
- 强约束只通过本地 `usage.json` 的 enforcement 标记 + reconcile gate 生效；**禁止**写 Raft 改用户/管理员设定（例如不再改 `Grant.enabled` / grant group）。

### Non-goals

- 不引入“全局审计型封禁记录表”（仅要求强约束可靠 + UI 可观察）。
- 不实现“只拒绝新连接但尽量保留既有连接”的软策略。

## 范围（Scope）

### In scope

- Backend：
  - `Node.quota_limit_bytes`（0=unlimited）
  - `usage.json` 增加 node 用量与 `exhausted` 标记
  - inbound stats 汇总（uplink+downlink）用于节点用量
  - Admin API：
    - nodes quota status（聚合）
    - quota usage override（跨节点转发）
  - quota worker / reconcile：不改用户设定，按 enforcement 标记控制运行态
- Web：
  - Node details：mode + limit + used override
  - Quota 输入解析支持 `TiB/PiB`，并将 `TB/PB` 兼容解释为 `TiB/PiB`

### Out of scope

- 用户详情/授权矩阵的信息架构重做。
- BigInt / 超过 `Number.MAX_SAFE_INTEGER` 的精确 bytes 输入与回显。

## 需求（Requirements）

### MUST

- 节点流量两态：
  - unlimited：`quota_limit_bytes == 0`
  - monthly limit：`quota_limit_bytes > 0` 且 `quota_reset.policy == "monthly"` 且 `tz_offset_minutes` 必须显式设置（禁止依赖节点 local timezone）
- UI 可随时设置节点限额与用量（used 为绝对值）。
- 超限动作：硬切断（RemoveUser + 不再 AddUser）；恢复：进入新周期或 used 下调到阈值以下后恢复接入。
- 不改用户设定：quota worker 不写 `SetGrantEnabled`；不改 `Grant.enabled` / grant group。

### SHOULD

- `quota-usage` override 默认同步 baseline（避免下一次 tick 把历史 totals 再加回来）。
- quota status 返回 `warning` 以提示配置缺失/不可计算等情况。

## 验收标准（Acceptance Criteria）

- Given 我在 Node details 将 mode 设为 `Monthly limit` 并设置 `limit + reset(day+tz)`，
  When 保存成功，
  Then quota status 能展示该节点的 `used/remaining/limit` 与 `next reset`。

- Given 节点用量超过阈值，
  When quota worker 检测到超限，
  Then 节点接入被硬切断（运行态），且用户/管理员设定不被修改（例如 `Grant.enabled` 不变化）。

- Given 我在 Node details 通过 IDC 对齐将 used 下调到阈值以下，
  When 保存成功，
  Then 系统立即或在一个 tick 内恢复接入权限（AddUser 回来）。

## 非功能性验收 / 质量门槛（Quality Gates）

- Backend：`cargo test`、`cargo clippy -- -D warnings`
- Web：`cd web && bun run lint && bun run typecheck && bun run test`

## 里程碑（Milestones）

- [ ] M1: Backend 数据结构（Node limit + usage v2）与 quota/reconcile enforcement 改造（不写 Raft 改 Grant.enabled）
- [ ] M2: Admin API：node quota status + quota usage override（含跨节点转发）
- [ ] M3: Web Node details：mode/limit/used override + 单测/校验
- [ ] M4: 最小验证（backend+web）与回归测试补齐

## 风险与开放问题（Risks & Open Questions）

- 需要谨慎处理 inbound tag baseline 与 xray 重启/计数回退，避免倒扣或重复计量。
- quota worker 与 reconcile 的触发频率需避免对 xray API 产生不必要压力（优先复用单次 QueryStats 结果）。

