# Web 滚动容器规范演进历史

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录影响长期理解的关键决策，而不是任务流水账。

## Decision Trace

- 有界纵向滚动以已有的 Radix `ScrollArea` 实现为唯一基线，而不是为每个面板定义浏览器专属 scrollbar CSS。
- 当前组件只安装纵向 scrollbar；横向与双向需求保持为显式领域设计，不能从该组件的存在推断为已支持。
- 已有原生 overflow 面板采用渐进迁移，避免无业务价值的全量样式重写。
- 仅纵向的消费者必须约束连续内容列宽度，防止 Radix viewport 的内部 table 布局被不可换行后代撑宽。
- 长名称只允许在自身 overflow viewport 内位移；该效果不会把当前 `ScrollArea` 扩展为横向滚动容器。
- 资源导航的十行规则是可视上限而非固定高度；内容不足十行时保持自然高度，展开状态同时限制为单个资源组。

## Key Reasons / Replacements

- 该规范补足了现有组件与各页面局部实现之间缺少的统一约束。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
