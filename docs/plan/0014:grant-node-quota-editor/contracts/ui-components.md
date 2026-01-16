# UI Components

## `GrantAccessMatrix`（Modify）

### Context

在既有“节点 × 协议”二维矩阵基础上，把“流量配额”从协议格子中抽离，改为**按节点（行头）**编辑与展示：每个 node 1 个 quota 值，对该 node 下所有协议生效。

### Props（增量变更）

以下为建议形状（具体字段名以实现阶段为准，但形状需保持等价）：

- Node-level meta（新增或扩展 `GrantAccessMatrixNode`）：
  - `nodeQuota?: {`
    - `value: "known" | "mixed" | "unset";`
    - `quotaLimitBytes?: number;`（当 value=known）
    - `reason?: string;`（可选：例如 mixed 的提示）
    - `}`
- Node quota edit callback（新增）：
  - `onEditNodeQuota?: (nodeId: string, quotaInput: string) => void`
    - `quotaInput` 使用字符串，便于输入单位与做本地校验。

### Behavior（关键交互）

- 节点行头展示 `Quota: <human>`（known）或 `Quota: Mixed`（mixed）或 `Quota: —`（unset）。
- 点击 quota 区进入编辑态：
  - 非 mixed：输入框默认填充紧凑写法（如 `10GiB` / `512MiB`）
  - mixed：输入框为空
- 提交：Enter 或点击 Apply；取消：Esc 或点击外部取消（取消不写入）。
- 校验失败时阻止提交并显示错误态（错误提示与输入框关联）。
- 错误提示以输入框下方的浮层（popover/tooltip）呈现，显示/隐藏不得改变行高或推动布局（避免 layout shift）。
  - 错误浮层必须始终在输入框下方（不允许 flip 到上方）。
  - 为避免被 overflow 裁剪，错误浮层应通过 portal 渲染；当下方空间不足以完整显示浮层时，必须先自动滚动最近的可滚动容器为浮层腾出空间，再显示浮层。
  - 错误浮层配色应使用主题的语义色（error）做低对比背景与清晰文本（避免突兀的纯白背景）。
  - 文本/背景对比度需足够（建议按 WCAG AA：小字号至少 4.5:1）。
  - 视觉规格：浮层与输入框间距建议 8px；浮层容器需有明显内边距（约 6px 纵向 / 10px 横向，或等价的高度/宽度设计）；形态使用小圆角矩形（避免胶囊体）。
  - 尺寸规则：浮层容器需自适应文本内容（不写死宽度），建议 `fit-content` + padding，并设置 `max-width` 允许自动换行。

### Quota input grammar（冻结口径）

`quotaInput: string` 解析为 bytes（用于 node quota）：

- 支持单位：`MiB` / `GiB`（以及常见写法变体）。
- 大小写不敏感；允许前后空格与单位/数字间空格。
- 无单位输入默认按 `MiB` 解释（不存在 bytes 输入口径）。
- 允许小数：换算为 bytes 后按四舍五入得到整数 bytes。
- 兼容识别：`MB` / `GB` 按二进制口径解释为 `MiB` / `GiB`。
- 空字符串必须视为非法输入（必须输入数字）；如需“无限制”，使用 `0`（按 `0MiB` 解释）。
- 非法输入（负数、无法识别单位、非数字）必须给出错误态并阻止提交。
- 上限约束：当结果超过 JS `Number.MAX_SAFE_INTEGER` 时必须报错并阻止提交（避免精度损失导致写入错误）。

### Non-goals

- 不在组件内直接实现后端写入与节点合计逻辑；组件只负责收集输入与表达状态，写入与语义由 API 与 xp 落地。
