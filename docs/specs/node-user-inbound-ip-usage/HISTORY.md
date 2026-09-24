# History

- The topic originated in legacy Spec `#6e7e4`.
- It preserves the completed inbound-IP usage contract under a stable slug-only path.

## 变更记录（Change log）

- 2026-03-08: 创建规格并冻结采集口径、窗口策略、API/UI 语义与 Geo 数据源。
- 2026-03-08: 根据当前实现回写 M1-M3 进度，并同步 API / Xray / ops 文档与运维示例。
- 2026-03-09: 完成 fast-track 收敛，补齐 Storybook 回归修复、用户节点 tab 切窗保持选中、IP/时间跨视图高亮联动、PR 截图引用、CI 全绿与最终状态回写。
- 2026-03-11: PR #110 跟进修正 IP usage 顶部高亮摘要 badge 的对比度与边界，让深色面板上的 pinned IP / Time 标签不再发虚，并在 review 后统一补正 light/dark 主题下的 IP badge 前景色与组件级样式回归测试。
- 2026-03-12: 为 PR #110 补充 pinned IP + Time 高亮摘要的 Storybook 截图证据，并同步到规格证据区与 PR 正文。
- 2026-03-12: 根据最新 review 修复顶部摘要 badge 的 light/dark 主题感知配色，并补上基于主题 token 的 AA 对比度回归测试，覆盖 pinned IP / Time 的前缀文案与数值文案，避免“看起来修了但仍然发虚”的回归。
- 2026-03-22: PR #115 跟进修正 online stats 误报边界；当 `GetStatsOnlineIpList` 缺失且该分钟 online count 为 `0` 或未生成时，按空样本处理，仅在 online count 为非零或 stats 能力确实不可用时返回 warning。
