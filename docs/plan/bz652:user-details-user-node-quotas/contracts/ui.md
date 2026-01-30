# UI（User details: tabs）

本文件冻结 `UserDetailsPage` 的信息架构与交互口径，确保实现阶段不再偏离“两个标签页：用户信息 / 接入点配置（矩阵）”的目标。

## Page header

- Title: `User`
- Description: `<user_id> — <display_name>`（等价信息即可，保留 monospace 风格）
- Actions:
  - `Back`（返回 Users 列表）
  - 不在此页暴露 “Grant group / New grant group” 入口（淡化 group 口径；该页只做本用户配置）

## Tabs

仅两个标签页：

1) `User`
2) `Node quotas`（矩阵型接入点配置）

默认打开 `User`。

## Tab: `User`

包含（沿用现有能力与字段）：

- `Profile`：display name、quota reset policy、reset day of month、tz_offset_minutes + Save changes
- `Subscription`：token 展示 + format 选择 + Copy URL + Fetch + Reset token
- `Danger zone`：Delete user（含确认弹窗）

不包含：

- 接入点/Grant 编辑器（移动到 `Node quotas` tab）

## Tab: `Node quotas`（矩阵型接入点配置）

目的：以“节点 × 协议”的矩阵，配置该用户可用的接入点（access points），并且不出现 group 口径（group 仅作为实现细节）。

布局（对齐 `GrantNewPage` 的密度与结构）：

- 顶部工具条：
  - Filter nodes…（按 node_name / node_id 过滤）
  - Selected `x / y` 计数（与 `GrantNewPage` 一致）
  - `Reset`（清空过滤条件）
- 主操作按钮：`Apply changes`
- 节点配额编辑（Quota）：
  - 在矩阵行（node）标题区域提供 `Quota: <value> (edit)` 的入口（复用既有 `NodeQuotaEditor` 的交互与解析口径，支持 `MiB/GiB` 输入）。
- 矩阵（复用 `GrantAccessMatrix` 交互口径）：
  - 列：protocols（VLESS / SS2022）
  - 行：nodes（node_name + node_id）
  - 单元格：checkbox on/off；如果同一 node+protocol 有多个 endpoint，可在 cell 内选择具体 endpoint

保存语义：

- “硬切（hard cut）”：`Apply changes` 会把本用户接入点集合覆盖为当前矩阵选择。
- 若当前选择为空：删除本用户的 managed group（等价于本用户无接入点）。
- 节点配额编辑是“即时保存”：
  - 修改 node quota 后立即调用后端写入接口；
  - 若 managed group 已存在且该节点有已选 endpoint，则同步更新对应 members 的 `quota_limit_bytes`，保持实际生效口径一致（不要求用户额外再点一次 `Apply changes`）。
