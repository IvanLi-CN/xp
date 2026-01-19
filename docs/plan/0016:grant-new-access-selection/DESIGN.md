# Grant 新建页（Create grant group）UI 设计（#0016）

本设计文档用于把 `GrantNewPage` 从“创建单条 grant”调整为“创建 1 个 grant group（N members）”，与 #0017 的契约一致：

- 前后端交互只使用 `grant-groups`；
- 支持多选矩阵 → 一次请求创建整组；
- 请求体不重复传递组级字段（`group_name` 只出现一次）。

## Screens

- Light: `./assets/grant-new-create-group-hifi-light.svg`
- Dark: `./assets/grant-new-create-group-hifi-dark.svg`

## 交互要点

- `group_name` 默认自动生成，但允许修改；冲突返回 `409` 时允许改名后重试。
- CTA：
  - 0 选中：禁用
  - 1 选中：`Create group`
  - N>1：`Create group (N members)`
- 提交：单次 `POST /api/admin/grant-groups`，服务端原子创建整组。
