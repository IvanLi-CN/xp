# HTTP APIs Contracts（#0017）

本文件冻结 #0017 涉及的 Admin HTTP APIs 的 schema 变更口径。

## Auth（统一）

所有 Admin APIs 使用：

- Header: `Authorization: Bearer <adminToken>`
- Response: `application/json`

错误返回口径沿用现有 `backendError` 约定（status/code/message）。

## Grant groups（Change: New）

> 硬约束：管理端交互**只允许**以 “grant group” 为单位进行读写与提交：
>
> - 前端提交必须是“整组提交（submit whole group）”，不做逐条 grant 的交互式操作；
> - 必须支持“组改名（rename）”且具备原子语义；
> - 请求体不得出现“必须相同的字段”在多个成员中重复传递（组级字段只出现一次）。
>
> 因此：不提供 `/api/admin/grants` 系列接口给前端使用；前后端只对接 `/api/admin/grant-groups`。

### Schema（shared）

#### AdminGrantGroup（group metadata）

```ts
type AdminGrantGroup = {
  group_name: string; // human-readable key, unique (A1)
};
```

#### AdminGrantGroupMember（group member）

`cycle_policy` / `cycle_day_of_month` **不再出现在**任何 group member 的返回中（周期归属见本文后续章节）。

```ts
type AdminGrantGroupMember = {
  // identity (used for diff / idempotency)
  user_id: string;
  endpoint_id: string;

  // mutable fields
  enabled: boolean;
  quota_limit_bytes: number; // int, >=0
  note: string | null;

  // response-only (optional): credential material, kept for compatibility with existing output flows
  credentials?: {
    vless?: { uuid: string; email: string };
    ss2022?: { method: string; password: string };
  };
};

type AdminGrantGroupDetail = {
  group: AdminGrantGroup;
  members: AdminGrantGroupMember[];
};
```

#### group_name rules（A1, must）

本计划已定案采用 **A1（name-as-key）**：

- `group_name` 是分组的唯一标识（identity），同时也是 UI 展示名（display）。
- `group_name` 必须全局唯一；重名返回 `409`.
- 为避免 URL/编码/归一化歧义，建议将 `group_name` 限制为 slug（实现阶段在前端与后端共同校验）：
  - regex: `[a-z0-9][a-z0-9-_]*`
  - 长度：`1..=64`（可调整，但需冻结）
- **组改名**：通过 group-level API 在服务端一次性批量重写该组内所有 grants（单事务/单 raft 命令），保证原子语义。

### List grant groups（GET /api/admin/grant-groups）

- Method: `GET`
- Path: `/api/admin/grant-groups`
- Response (success):
  ```ts
  ```

type AdminGrantGroupSummary = AdminGrantGroup & {
member_count: number;
};

type AdminGrantGroupsResponse = { items: AdminGrantGroupSummary[] };

````
### Get grant group（GET /api/admin/grant-groups/{group_name}）

- Method: `GET`
- Path: `/api/admin/grant-groups/{group_name}`
- Response (success): `AdminGrantGroupDetail`
- Errors:
  - `404` when `group_name` not found

### Create grant group（POST /api/admin/grant-groups）

> 约束：为避免“空分组”语义，本接口要求一次性提交该组的完整成员集合。

- Method: `POST`
- Path: `/api/admin/grant-groups`
- Request:
  - Body:
    ```ts
    type AdminGrantGroupCreateRequest = {
      group_name: string;
      members: Array<{
        user_id: string;
        endpoint_id: string;
        enabled: boolean;
        quota_limit_bytes: number; // int, >=0
        note?: string | null;
      }>;
    };
    ```
- Validation:
  - `members.length >= 1`（不允许创建空分组）
  - `members` 内 `(user_id, endpoint_id)` 不允许重复
  - `(user_id, endpoint_id)` 必须全局唯一：若该 pair 已存在于其他 group，返回 `409`
- Response (success): `AdminGrantGroupDetail`
- Errors:
  - `400 invalid_request`：payload 校验失败（重复条目、quota 非法等）
  - `404`：涉及到的 user/endpoint 不存在
  - `409 conflict`：`group_name` 重名或成员 pair 冲突

### Replace / Rename grant group（PUT /api/admin/grant-groups/{group_name}）

> “整组提交”的核心接口：一次请求提交该组的**完整期望状态**（含改名与成员集合），服务端原子应用变更。

- Method: `PUT`
- Path: `/api/admin/grant-groups/{group_name}`
- Request:
  - Body:
    ```ts
    type AdminGrantGroupReplaceRequest = {
      // optional: rename group_name (A1)
      rename_to?: string;
      members: Array<{
        user_id: string;
        endpoint_id: string;
        enabled: boolean;
        quota_limit_bytes: number; // int, >=0
        note?: string | null;
      }>;
    };
    ```
- Semantics:
  - 该请求是“replace”：请求中的 `members` 集合是该组最终应存在的授权集合；
  - 若存在 `rename_to`，则对该 group 执行改名（批量重写 `group_name`），并与成员 diff 一并原子提交；
  - 服务端计算 diff 并原子执行 create/update/delete（实现阶段以单个 raft 命令 / 单事务落地）。
- Response (success):
  ```ts
  type AdminGrantGroupReplaceResponse = {
    group: AdminGrantGroup; // group_name is the new one after rename (if any)
    created: number;
    updated: number;
    deleted: number;
  };
````

- Errors:
  - `400 invalid_request`：payload 校验失败（重复条目、quota 非法等）
  - `404`：group_name / user / endpoint 不存在
  - `409 conflict`：
    - `rename_to` 重名
    - 成员 pair 与其他 group 冲突
    - 并发写冲突（实现阶段如引入 version/etag）

### Delete grant group（DELETE /api/admin/grant-groups/{group_name}）

- Method: `DELETE`
- Path: `/api/admin/grant-groups/{group_name}`
- Semantics:
  - 原子删除该组及其所有成员 grants（实现阶段以单个 raft 命令落地）。
- Response (success): `{ deleted: number }`
- Errors:
  - `404` when `group_name` not found

## Legacy grants APIs（Change: Delete）

为满足“不得存在 grants API”的约束，以下接口在实现阶段需要被删除（或彻底内部化，不允许 web 调用）：

- `GET /api/admin/grants`
- `POST /api/admin/grants`
- `PATCH /api/admin/grants/{grant_id}`

## Quota reset ownership（Change: Modify）

> 冻结口径（已定案）：
>
> - Grant 不承载“流量重置/周期/时区”等配置；
> - **User** 与 **Node** 均有自己的“流量重置配置”；
> - 在 grant 配置 UI（节点行）上，允许对每个节点选择“参考用户配置”或“参考节点配置”（默认参考用户配置）。

### Shared schema: QuotaResetConfig

> 说明：`unlimited` 表示“无限流量/不做配额封禁”。

```ts
type QuotaResetPolicy = "monthly" | "unlimited";

type QuotaResetConfig =
  | { policy: "unlimited" }
  | {
      policy: "monthly";
      day_of_month: number; // 1..=31
    };
```

语义：

- `policy: "monthly"`：在所选时区的**每月第 `day_of_month` 天 00:00** 触发重置，并执行配额封禁逻辑（若设置了限额）。
- `policy: "unlimited"`：不做配额封禁（无限流量）；`day_of_month` 不适用。
- 若 `day_of_month=31` 但当月无 31 日，则按“当月最后一天 00:00”处理。

配额封禁（enforcement）：

- 当 `policy="unlimited"` 时：始终不封禁（无限流量）。
- 当 `policy="monthly"` 时：仅当对应的 `quota_limit_bytes > 0` 才启用封禁；`quota_limit_bytes == 0` 视为无限流量。

### AdminUser schema delta（reset config + tz）

用户侧默认时区为 UTC+8，但允许配置：

```ts
type AdminUserQuotaReset = QuotaResetConfig & {
  tz_offset_minutes: number; // default: 480
};
```

实现阶段：`AdminUser` 需要包含/支持该字段（路径沿用现有 User APIs）。

### AdminNode schema delta（reset config + tz）

节点侧默认按“服务器时区”（Local），但允许配置：

```ts
type AdminNodeQuotaReset = QuotaResetConfig & {
  // null/omitted means server-local timezone
  tz_offset_minutes?: number | null;
};
```

实现阶段：`AdminNode` 需要包含/支持该字段（路径沿用现有 Node APIs）。

### User × Node: reset source selector（per-row）

每个用户对每个节点可选择“参考用户配置”或“参考节点配置”（默认参考用户配置）：

```ts
type QuotaResetSource = "user" | "node";
```

该字段的存储与写入口与“User × Node 配额设置”合并（见计划 #0014），冻结为以下接口与字段：

#### List user node quotas（GET /api/admin/users/{user_id}/node-quotas）

- Method: `GET`
- Path: `/api/admin/users/{user_id}/node-quotas`
- Response (success):
  ```ts
  type AdminUserNodeQuota = {
    user_id: string;
    node_id: string;
    quota_limit_bytes: number; // int, >=0
    quota_reset_source: QuotaResetSource; // default: "user"
  };

  type AdminUserNodeQuotasResponse = { items: AdminUserNodeQuota[] };
  ```

#### Set user node quota（PUT /api/admin/users/{user_id}/node-quotas/{node_id}）

- Method: `PUT`
- Path: `/api/admin/users/{user_id}/node-quotas/{node_id}`
- Request:
  - Body:
    ```ts
    type AdminUserNodeQuotaUpsertRequest = {
      quota_limit_bytes: number; // int, >=0
      quota_reset_source?: QuotaResetSource; // default: "user"
    };
    ```
- Response (success): `AdminUserNodeQuota`

#### Errors（shared）

- `401 Unauthorized`: missing/invalid admin token
- `404 Not Found`: user/node not found
- `400 Bad Request`: invalid payload (negative quota, wrong type, overflow, invalid enum, etc.)
- `500 Internal Server Error`: unexpected failures

## Users（Change: Modify）

> 变更目标：
>
> - 移除 `cycle_policy_default` / `cycle_day_of_month_default`（旧口径：grant inherit_user 时选择 by_user/by_node）；
> - 增加用户级“流量重置配置（按月|无限）+ 时区”，且默认 UTC+8；
> - grant 配置（节点行）上的“参考用户/节点”由 `UserNodeQuota.quota_reset_source` 控制（见上节）。

### Schema（AdminUser）

```ts
type AdminUserQuotaReset =
  | { policy: "unlimited"; tz_offset_minutes: number }
  | { policy: "monthly"; day_of_month: number; tz_offset_minutes: number };

type AdminUser = {
  user_id: string;
  display_name: string;
  subscription_token: string;

  // new in #0017: quota reset config (Grant 不承载)
  quota_reset: AdminUserQuotaReset;
};
```

校验/默认值：

- `day_of_month`: `1..=31`
- `tz_offset_minutes`: `[-720, 840]`（UTC-12 .. UTC+14），默认 `480`（UTC+8）
- `policy="monthly"` 的重置发生在该时区 **day_of_month 当天 00:00**（“只设置日期”，不配置时分秒）

### Create user（POST /api/admin/users）

- Method: `POST`
- Path: `/api/admin/users`
- Request body:
  ```ts
  type AdminUserCreateRequest = {
    display_name: string;
    quota_reset?: AdminUserQuotaReset; // default: monthly@day=1, tz=480
  };
  ```
- Response (success): `AdminUser`
- Errors:
  - `400 invalid_request` when payload invalid

### Patch user（PATCH /api/admin/users/{user_id}）

- Method: `PATCH`
- Path: `/api/admin/users/{user_id}`
- Request body:
  ```ts
  type AdminUserPatchRequest = {
    display_name?: string;
    quota_reset?: AdminUserQuotaReset;
  };
  ```
- Response (success): `AdminUser`
- Errors:
  - `404` when `user_id` not found
  - `400 invalid_request` when payload invalid

### List users（GET /api/admin/users）

- Method: `GET`
- Path: `/api/admin/users`
- Response (success): `{ items: AdminUser[] }`

## Nodes（Change: Modify）

> 节点侧也必须具备“流量重置配置（按月|无限）+ 时区”，默认按服务器时区（Local）。

### Schema（AdminNode）

```ts
type AdminNodeQuotaReset =
  | { policy: "unlimited"; tz_offset_minutes?: number | null }
  | { policy: "monthly"; day_of_month: number; tz_offset_minutes?: number | null };

type AdminNode = {
  node_id: string;
  node_name: string;
  public_domain: string;
  api_base_url: string;

  // new in #0017: quota reset config (Grant 不承载)
  quota_reset: AdminNodeQuotaReset;
};
```

校验/默认值：

- `day_of_month`: `1..=31`
- `tz_offset_minutes`:
  - `null` / omitted 表示服务器本地时区（Local，默认）
  - 若提供则为 `[-720, 840]`（UTC-12 .. UTC+14）
- `policy="monthly"` 的重置发生在该时区 **day_of_month 当天 00:00**（“只设置日期”，不配置时分秒）

### List nodes（GET /api/admin/nodes）

- Method: `GET`
- Path: `/api/admin/nodes`
- Response (success): `{ items: AdminNode[] }`

### Get node（GET /api/admin/nodes/{node_id}）

- Method: `GET`
- Path: `/api/admin/nodes/{node_id}`
- Response (success): `AdminNode`
- Errors:
  - `404` when `node_id` not found

### Patch node（PATCH /api/admin/nodes/{node_id}）

- Method: `PATCH`
- Path: `/api/admin/nodes/{node_id}`
- Request body:
  ```ts
  type AdminNodePatchRequest = {
    node_name?: string;
    public_domain?: string;
    api_base_url?: string;
    quota_reset?: AdminNodeQuotaReset;
  };
  ```
- Response (success): `AdminNode`
- Errors:
  - `404` when `node_id` not found
  - `400 invalid_request` when payload invalid
