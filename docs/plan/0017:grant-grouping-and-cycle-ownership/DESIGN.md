# Grant groups UI 设计（#0017）

本设计文档用于描述 **grant-groups（整组提交）** 的管理端 UI 形状，确保与 #0017 冻结的契约一致：

- 不提供/不依赖 `grants` API（前后端交互只用 `grant-groups`）。
- 所有修改以 **整组提交（replace/rename）** 落地；请求体不重复传递组级字段。
- Grant 不承载“流量重置/周期/时区”；重置配置在 **User / Node**，节点行可选择参考 user 或 node（见 #0014）。

## Screens

### 1) Grant groups · List

- Light: `./assets/grant-groups-hifi-light.svg`
- Dark: `./assets/grant-groups-hifi-dark.svg`

交互要点：

- 列表以 `group_name` 展示（唯一、可改名）。
- 支持搜索（按 `group_name`）。
- “New group” 入口跳转到 #0016 定义的创建流程。

### 2) Grant groups · Details / Editor（整组编辑）

- Light: `./assets/grant-group-details-hifi-light.svg`
- Dark: `./assets/grant-group-details-hifi-dark.svg`

交互要点：

- 页面级 `Save changes` = `PUT /api/admin/grant-groups/{group_name}`：
  - 请求体只出现一次 `rename_to`（若改名）；
  - 请求体只出现一次 `members[]`（完整期望状态，replace 语义）。
- UI 内部允许对成员行进行增删改（enabled/note 等），但**不做逐条保存**；只允许整组保存。
- 删除组：`DELETE /api/admin/grant-groups/{group_name}`（原子删除组内所有成员）。

### 3) User details · Quota reset（重置配置归属：User）

- Light: `./assets/user-details-quota-reset-hifi-light.svg`
- Dark: `./assets/user-details-quota-reset-hifi-dark.svg`

交互要点：

- Policy：`monthly | unlimited`
- Monthly：只设置 `day_of_month`（**仅日期，不配置时分秒**），重置发生在该时区当日 `00:00`
- Timezone：默认 UTC+8，允许配置（以 offset 展示）

### 4) Node details · Quota reset（重置配置归属：Node）

- Light: `./assets/node-details-quota-reset-hifi-light.svg`
- Dark: `./assets/node-details-quota-reset-hifi-dark.svg`

交互要点：

- Policy：`monthly | unlimited`
- Monthly：只设置 `day_of_month`（**仅日期，不配置时分秒**）
- Timezone：默认服务器本地时区（Local），允许显式指定 offset
