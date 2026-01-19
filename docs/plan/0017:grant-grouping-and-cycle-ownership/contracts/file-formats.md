# File formats Contracts（#0017）

本文件冻结 #0017 涉及的持久化文件格式变更口径（主要是 `state.json`）。

## PersistedState（state.json）

### Current (schema_version=1)

`state.json` 顶层包含：

- `schema_version`
- `nodes`（map）
- `endpoints`（map）
- `users`（map）
- `grants`（map）

其中 `Grant` 在 schema v1 上包含 `cycle_policy` / `cycle_day_of_month`。
此外，schema v1 的 `User` 包含 `cycle_policy_default` / `cycle_day_of_month_default`，用于 `cycle_policy=inherit_user` 的默认行为。

### Target (schema_version=2)

目标：

- `Grant`：
  - 增加 `group_name`（A1, name-as-key）
  - 移除 `cycle_policy` / `cycle_day_of_month`
- `User`：
  - 移除 `cycle_policy_default` / `cycle_day_of_month_default`
  - 增加 `quota_reset`（按月|无限 + 时区，默认 UTC+8）
- `Node`：
  - 增加 `quota_reset`（按月|无限 + 时区，默认服务器时区 Local）
- `PersistedState`：
  - 新增 `user_node_quotas`：存储用户在某个节点上的配额与“重置参考来源”（参考 user 或 node，默认 user）

#### Grant JSON shape

```json
{
  "grant_id": "<string>",
  "group_name": "<string>",
  "user_id": "<string>",
  "endpoint_id": "<string>",
  "enabled": true,
  "quota_limit_bytes": 0,
  "note": null,
  "credentials": { "vless": { "uuid": "...", "email": "..." } }
}
```

#### Quota reset config ownership

冻结口径：

- User 与 Node 均有自己的“流量重置配置”，但 Grant 不承载。
- User 默认 UTC+8；Node 默认服务器时区（Local），但均可配置。

配额封禁（enforcement）：

- 当 `policy="unlimited"`：禁用封禁（无限流量）。
- 当 `policy="monthly"`：仅当对应的 `quota_limit_bytes > 0` 才启用封禁；`quota_limit_bytes == 0` 视为无限流量。

#### User JSON shape（新增 quota_reset）

```json
{
  "user_id": "<string>",
  "display_name": "<string>",
  "subscription_token": "<string>",
  "quota_reset": {
    "policy": "monthly",
    "day_of_month": 1,
    "tz_offset_minutes": 480
  }
}
```

说明：

- `tz_offset_minutes` 对 User 必须存在（默认 `480`，UTC+8）。
- `policy="monthly"` 的重置发生在该时区 **day_of_month 当天 00:00**（“只设置日期”，不配置时分秒）。

#### Node JSON shape（新增 quota_reset）

```json
{
  "node_id": "<string>",
  "node_name": "<string>",
  "public_domain": "<string>",
  "api_base_url": "<string>",
  "quota_reset": {
    "policy": "monthly",
    "day_of_month": 1,
    "tz_offset_minutes": null
  }
}
```

说明：

- `tz_offset_minutes` 对 Node 可为 `null` / 缺省，表示“服务器本地时区（Local）”（默认）。
- `policy="monthly"` 的重置发生在该时区 **day_of_month 当天 00:00**（“只设置日期”，不配置时分秒）。

#### User × Node quotas JSON shape（新增 user_node_quotas）

用于持久化 `GET/PUT /api/admin/users/{user_id}/node-quotas*` 的状态（见 #0014/#0017 contracts）。

```json
{
  "user_node_quotas": {
    "<user_id>": {
      "<node_id>": {
        "quota_limit_bytes": 0,
        "quota_reset_source": "user"
      }
    }
  }
}
```

- `quota_limit_bytes`: `>= 0`，`0` 表示“无限流量/不做配额封禁”
- `quota_reset_source`: `"user" | "node"`，默认 `"user"`

### Migration rules (v1 -> v2)

实现阶段必须提供“可测试、可重复执行”的迁移逻辑，至少覆盖：

1. 读取旧 `schema_version=1`；
2. 转换为新结构；
3. 写回 `schema_version=2`；
4. 再次加载不报 `schema_version mismatch`。

#### group_name migration (default)

默认：对旧 grants 生成 `group_name`（避免为空），例如：

- `group_name = "legacy-" + user_id`（按用户聚合 legacy grants，推荐）
- 或 `group_name = "legacy-" + grant_id`（每条 grant 独立成组，兼容但分组价值低）

#### cycle_* migration

- v1 中 grant 上的 `cycle_policy` / `cycle_day_of_month` 在 v2 中不再保留；
- 迁移时需将“按月重置日（day_of_month）”与（新增的）“时区”写入 User/Node 的 `quota_reset`：
  - User：
    - `quota_reset.policy = "monthly"`（v1 无 “unlimited” 概念；仍可通过 `quota_limit_bytes=0` 达到不封禁）
    - `quota_reset.day_of_month = user.cycle_day_of_month_default`
    - `quota_reset.tz_offset_minutes = 480`（UTC+8）
  - Node：
    - `quota_reset.policy = "monthly"`
    - `quota_reset.day_of_month`：若该节点存在任意“有效周期来源=ByNode”的 grants 且日一致，则取该日；否则用默认 `1`
    - `quota_reset.tz_offset_minutes = null`（服务器时区 Local）
- `quota_reset_source`（参考 user/node）必须迁移到 `user_node_quotas`（新结构）：
  - 对每个 `(user_id, node_id)`（用户在该节点上存在至少 1 条 grant）：
    - `quota_reset_source` 取该 user-node 下 grants 的**有效周期来源**（ByUser/ByNode）；
    - 若存在冲突（同一 user-node 下出现 ByUser 与 ByNode 混用），迁移必须失败并返回可定位的错误（不允许静默丢失）。
- 若历史数据存在“同一 user-node 在不同 grants 上配置了不同的周期日”或“同一 node 被不同 grants 写入不同 day”：
  - 迁移必须失败并返回可定位的错误（建议拒绝启动，要求人工修复）；
  - 不允许选择优先级并静默丢失，避免周期语义不可预期。
