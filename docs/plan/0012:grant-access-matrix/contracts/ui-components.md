# UI Components Contracts（#0012）

本文件用于冻结本计划新增/改动的 UI 组件契约（props / events / state 约定），以便实现阶段可直接按契约落地并编写单测/Storybook。

## `GrantAccessMatrix`

### Purpose

以二维表格方式编辑“允许的接入点”选择集：行=节点（Node），列=协议（Protocol / Endpoint.kind）。

### Inputs (props)

- `rows`: `{ protocolId: string; label: string }[]`
  - （更新）`rows` 表示节点列表（见下方字段名建议）
- `columns`: `{ nodeId: string; label: string }[]`
  - （更新）`columns` 表示协议列表（见下方字段名建议）
- `cells`: `Record<string /* protocolId */, Record<string /* nodeId */, CellState>>`

`CellState`：

- `value`: `"on" | "off" | "disabled"`
- `reason?`: string
  - 当 `value="disabled"` 时用于解释原因（例如 `"no endpoint"` / `"multiple endpoints"`）
- `meta?`: object
  - 供展示用途的可选信息（例如 `endpoint_id`、`port`、`tag`、`grant_id`）

### Outputs (events)

- `onToggleCell(protocolId, nodeId): void`
- `onToggleRow(protocolId): void`
- `onToggleColumn(nodeId): void`
- `onToggleAll(): void`

### Interaction rules (normative)

对 row/column/all 的批量开关，点击行为固定为：

- 若该组存在任意 `value="on"` 的单元格：将该组所有可编辑单元格设为 `off`
- 否则：将该组所有可编辑单元格设为 `on`
- 不提供“反选”（invert）
- `disabled` 单元格不被批量操作改变

### Naming note (for implementation phase)

为避免“rows/columns”语义与维度混淆，实现阶段建议将 inputs 重命名为更直白的：

- `nodes`: `{ nodeId: string; label: string }[]`
- `protocols`: `{ protocolId: string; label: string }[]`
- `cells`: `Record<nodeId, Record<protocolId, CellState>>`

并将事件签名同步为 `onToggleCell(nodeId, protocolId)` / `onToggleRow(nodeId)` / `onToggleColumn(protocolId)`。

### Accessibility

- 表格结构语义清晰（`table`/`thead`/`tbody`），行头与列头可通过屏幕阅读器识别。
- 单元格可通过键盘聚焦并切换（Space/Enter 触发等价于点击）。
