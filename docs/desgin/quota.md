# xp · 配额与周期重置（MVP 规格）

## 1. 配额口径

- 统计口径：**双向合计**\
  `used_bytes = uplink_bytes + downlink_bytes`
- 单位：字节（bytes）
- 误差容忍：**10MiB**\
  `10 MiB = 10 * 1024 * 1024 bytes`

> 误差主要来自：统计拉取间隔、并发连接的最后少量流量、以及封禁动作到实际断链的延迟。

## 2. 周期（Cycle）定义

每个 Grant 有一个“当前周期窗口”：

- `cycle_start_at`：周期起点（含）
- `cycle_end_at`：周期终点（不含），等于下一周期起点

周期参数来源（按 Grant 配置）：

- `cycle_policy`：
  - `ByNode`：使用节点本地时区的 00:00
  - `ByUser`：使用用户固定时区 `UTC+8` 的 00:00
- `cycle_day_of_month`：每月 X 日（1–31）

规则：

1. 周期起点是 **每月 X 日 00:00**（按对应时区）。
2. 若当月不存在 X 日，则周期起点为 **当月最后一天 00:00**。
3. 周期终点为下一周期起点。

示例（X=31，ByUser=UTC+8）：

- 2025-01 周期起点：2025-01-31 00:00 (+08:00)
- 2025-02 周期起点：2025-02-28 00:00 (+08:00)（当月最后一天）
- 2025-03 周期起点：2025-03-31 00:00 (+08:00)

## 3. 本地持久化状态（不进 Raft）

每个节点仅对“属于本节点的 Grants”维护本地用量状态（建议 KV/小文件持久化）：

- `grant_id`
- `cycle_start_at` / `cycle_end_at`
- `used_bytes`
- `last_uplink_total` / `last_downlink_total`：上次拉取到的累计值（用于求增量）
- `last_seen_at`：最后一次成功拉取统计的时间

> 说明：Xray 的统计值是“累计值”。`xp` 通过“当前值 - 上次值”计算增量并累计到 `used_bytes`。

## 4. 统计拉取与增量计算

### 4.1 拉取频率

- 建议默认 10s（可配置 5s–30s）
- 当节点 Grants 数较大时，可使用分批拉取/批量 QueryStats（实现细节后续定）

### 4.2 增量计算

对单个 Grant：

1. 拉取 `(uplink_total, downlink_total)`
2. 计算增量：
   - `delta_up = uplink_total - last_uplink_total`
   - `delta_down = downlink_total - last_downlink_total`
3. 若任一 delta 为负数（计数回退），视为 **Xray 重启/统计重置**：
   - 将 `(last_uplink_total, last_downlink_total)` 重置为本次拉取值
   - 本轮增量按 0 计（不倒扣）
4. `used_bytes += delta_up + delta_down`

## 5. 超限判定与封禁

当 `quota_limit_bytes > 0` 时：

- 若 `used_bytes + 10MiB >= quota_limit_bytes`：
  1. 立即对对应 inbound 执行 `RemoveUserOperation`（本机动作，尽快断开新连接）
  2. 写入 Raft：`Grant.enabled=false`（全局一致）

行为约束：

- 超限后**禁止连接**：即使用户拿到订阅，也应因 `enabled=false` 而无法建立连接（client 已被移除）。

## 6. 周期切换与自动解封

当 `now >= cycle_end_at`（进入下一周期）：

1. 计算新窗口 `(cycle_start_at, cycle_end_at)`
2. 将 `used_bytes=0`
3. 若配置允许“自动解封”（建议默认开启）：
   - 写入 Raft：`Grant.enabled=true`
   - reconcile 后会 AddUser 回到 inbound

> 是否“自动解封”可作为全局/用户/Grant 的策略开关（MVP 可先做全局开关）。

## 7. 与集群一致性的关系

- “封禁/解封”必须写入 Raft（避免管理员在其他节点看到状态不一致）。
- “用量累计”不写入 Raft：\
  管理界面展示时，管理节点可向目标节点拉取实时用量（或使用节点定期上报摘要的方式做汇总——后续可选）。
