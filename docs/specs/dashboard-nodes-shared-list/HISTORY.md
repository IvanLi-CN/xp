# History

- The topic originated in legacy Spec `#gj4xg`.
- It preserves the shared-list contract after the legacy ID-based catalog was retired.

## 变更记录（Change log）

- 2026-03-03: 初始规格创建，冻结“共享列表 + 图标纯链接 + 响应式卡片”口径。
- 2026-03-03: 完成共享列表实现与页面接入，新增/更新单测并补齐 e2e mock 路由。
- 2026-03-03: 修复 `ResizeObserver` 缺失时的降级渲染风险，补充对应单测。
- 2026-03-03: PR #93 checks 全部通过（`pr-label-gate` / `ci` / `xray-e2e`），完成快车道收敛。
- 2026-03-04: 根据反馈将桌面表格改为三列合并（Node/Endpoint/Runtime），并在窄宽度下自动切换为列表卡片；补充组件级 Story 与 PR 截图资产。
