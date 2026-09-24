# History

- The topic originated in legacy Spec `#wvrmn`.
- It records the completed transition to membership-only access.

## Change log

- 2026-03-01: implemented (PR #86)
- 2026-03-02: AccessMatrix 多 endpoint 树交互做 post-fix（父子勾选对齐、折叠态保留全选、列宽固定不抖动且树展开保持 in-flow 以允许行高变化）；继续补强为 `colgroup + table-fixed` 固定列宽，移除 Node ID 与空态提示文案展示，折叠/展开图标切换为 Iconify 文件夹并加过渡动画，同时将树引导线对齐到图标中心并弱化配色，修正折叠态复选框与右侧标题的垂直对齐（PR #90）。
- 2026-03-02: AccessMatrix 树形勾选状态修正：多选单元格部分选中时，行/列/全局勾选框均正确显示 `indeterminate` 而非错误“全选”；同时修复单 endpoint 未勾选场景的元数据显示，避免展示 `port ?`（PR #90）。
- 2026-03-02: AccessMatrix 状态一致性修正：当 endpoint 列表变更时，仅保留仍存在于当前 options 的已选 endpoint，避免 stale endpoint_id 导致单元格误显示为选中或提交无效 endpoint（PR #90）。
- 2026-04-29: 增加协议级自动授予语义：用户保存某协议全选后，新增同协议 endpoint 自动补齐 membership；Access API 响应暴露 `auto_assign_endpoint_kinds`。
- 2026-04-29: Access tab 增加协议全选与节点全选的上下文提示，明确 apply 后未来 endpoint 自动授予范围。
