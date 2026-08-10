# Web 滚动容器规范

> 当前有效规范以本文为准；实现覆盖与当前状态见
> `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- Web 管理界面同时包含页面级滚动、表格横向溢出和受限高度的独立列表。若每个面板自行使用浏览器默认滚动条或伪造滚动条，主题与浏览器之间的视觉和交互会失去一致性。
- `web/src/components/ui/scroll-area.tsx` 已提供基于 Radix 的真实纵向滚动容器；本规范固定它作为有界面板的滚动条基线。

## 目标 / 非目标

### Goals

- 为需要独立纵向滚动的有界面板建立唯一的视觉与交互基线。
- 明确当前 `ScrollArea` 的能力边界，避免将它误用为通用双向滚动容器。
- 保留页面级滚动和已获专门布局保障的横向溢出方案。

### Non-goals

- 不统一修改浏览器或操作系统的页面级滚动条。
- 不改变数据表在各自规格中定义的横向滚动或响应式降级策略。
- 不在没有设计、API 和 Storybook 覆盖的情况下为 `ScrollArea` 增加双向滚动、强制常显轨道或新的主题变量。

## 范围（Scope）

### In scope

- 导航子项、筛选结果、日志片段和其他具有固定或最大可视高度的独立纵向滚动面板。
- `ScrollArea` 组件、其 Storybook 状态以及采用该组件的新建或实质改造面板。

### Out of scope

- 文档、应用主体和无固定高度页面的浏览器滚动。
- 代码预览、表格等需要横向滚动的专用表面。
- 虚拟列表的渲染策略和数据分页策略。

## 需求（Requirements）

### MUST

- 新建或实质改造的有界纵向滚动面板必须使用 `@/components/ui/scroll-area`，除非其所属规格明确要求横向滚动、虚拟化或其他专用交互。
- 调用方必须提供有限的高度或最大高度；`ScrollArea` 不负责决定行数、面板高度或业务数据的截断规则。
- 采用 `ScrollArea` 时必须保留当前组件的真实 Radix viewport 与 scrollbar，不得用静态、伪造或不可拖拽的滚动条替代。
- 仅纵向滚动的调用方必须约束连续内容列的宽度，使其不被不可换行的后代撑过 viewport；
  Radix viewport 内部布局不得因此产生未声明的横向溢出。
- 当前基线只渲染一个纵向 `ScrollBar`：轨道宽度为 `w-2.5`（10px），带透明左边框和
  `p-[1px]` 内边距；拇指为圆角 `bg-border`。调用方不得以局部 CSS 覆盖这些尺寸、颜色或圆角。
- 必须保留 `ScrollArea` 当前的轨道可见性生命周期；调用方不得强制滚动条常显或常隐。
- 面板内的可聚焦元素和活动项必须能够进入 viewport；程序化定位应使用最近边界且不使用平滑动画，除非所属交互规格另有说明。
- 不得对采用本组件的面板添加 `::-webkit-scrollbar`、`scrollbar-color` 或 `scrollbar-width` 覆盖。

### SHOULD

- 有界滚动面板应只承载一个连续内容列，避免嵌套独立纵向滚动区。
- 采用组件时应保留内容容器的边界圆角，避免 viewport、轨道和面板边框出现错位。
- 组件变更或新增典型使用场景时，应更新 `UI/ScrollArea` Storybook story，确认 light/dark 主题下轨道、拇指和长内容滚动均可辨识。

### COULD

- 某个领域若需要双向滚动，可在独立组件和规格中扩展；扩展必须明确横向轨道、角落处理、键盘/触摸行为与 Storybook 覆盖。

## 功能与行为规格（Functional/Behavior Spec）

### 选择滚动模型

1. 无固定高度的页面内容保持浏览器原生页面滚动。
2. 仅需要在固定可视区内纵向查看连续内容时，使用 `ScrollArea`。
3. 需要横向查看不可换行内容、表格列或代码时，使用该表面已有的专用滚动实现；当前 `ScrollArea` 不承担横向滚动。

### 有界纵向面板

- `ScrollArea` root 保持相对定位和 `overflow-hidden`，viewport 充满可用空间并继承面板圆角。
- 连续内容列必须将可用宽度作为上限；遇到 Radix viewport 内部的 table 布局时，可使用
  `w-0 min-w-full` 等等价约束，保证 `scrollWidth <= clientWidth`，而不是依赖裁切掩盖横向溢出。
- 滚动条尺寸与拇指长度由真实内容和 Radix 原语计算；鼠标滚轮、触控滚动与拖拽拇指必须作用于同一个 viewport。
- 内容量未溢出时，不应以占位轨道、伪拇指或额外留白暗示可滚动。
- 名称在自身 `overflow-hidden` viewport 内通过 transform 展示被隐藏内容，不构成 `ScrollArea`
  的横向滚动能力；该位移不得改变连续内容列宽度、外层 `scrollWidth` 或纵向 scrollbar 行为。

### 边界与例外

- 现有原生 `overflow-*` 实现无需仅为迁移而重写；当其所属面板被实质改造或需要一致滚动外观时，再按本规范接入 `ScrollArea`。
- 业务规格可以提高可视行数、要求活动项自动定位或规定溢出方向，但不得改变本组件的纵向轨道基线。

## 接口契约（Interfaces & Contracts）

None。该规范不改变后端接口，也不扩展 `ScrollArea` 的公开 TypeScript API。

## 验收标准（Acceptance Criteria）

- Given 一个具有有限高度且内容超出的新建导航或列表面板，
  When 它只需要纵向滚动，
  Then 它使用 `ScrollArea`，并在组件的正常可见性生命周期内提供当前定义的真实纵向轨道和可拖拽拇指。

- Given 一个内容未溢出的 `ScrollArea` 面板，
  When 在 light 或 dark 主题查看，
  Then 页面不通过伪滚动条、额外占位或局部浏览器 scrollbar CSS 表示可滚动。

- Given 一个需要横向检查代码或表格列的表面，
  When 其内容超出宽度，
  Then 它遵循该表面的专用规格，而不假设当前 `ScrollArea` 提供横向轨道。

- Given 一个仅纵向滚动且包含不可换行长名称的 `ScrollArea`，
  When 名称发生截断或在自身 viewport 内位移，
  Then 外层 viewport 仍满足 `scrollWidth <= clientWidth`，活动项边界完整处于 viewport 内，
  且不出现横向轨道。

## 验收清单（Acceptance checklist）

- [x] 有界纵向滚动的组件基线已明确。
- [x] 页面级、横向和历史实现的边界已明确。
- [x] 纵向消费者的内容宽度约束与名称内部位移边界已明确。
- [x] 接口影响已明确为 `None`。
- [x] 相关验收条件可用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 组件行为或调用模式改变时，覆盖有界内容、溢出内容和连续内容列宽度约束。
- E2E tests: 关键工作流需要滚动定位活动项时，覆盖活动项可见性。

### UI / Storybook

- Stories: `UI/ScrollArea` 保持一个溢出的长列表状态。
- Visual review: 在 light/dark 主题检查轨道、拇指和活动项完整边界，在桌面及移动视口检查长名称
  不会扩展横向 overflow，且触控滚动不被外层容器阻断。

### Quality checks

- `bunx --no-install dprint check docs/specs/scroll-containers docs/specs/README.md`

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：将双向滚动需求强行套入当前纵向组件会造成缺失的横向轨道或可访问性退化。
- 假设：Radix Scroll Area 继续作为项目的有界纵向滚动原语；若替换该依赖，必须同时更新本规范与 Storybook 基线。

## 参考（References）

- `web/src/components/ui/scroll-area.tsx`
- `web/src/components/ui/scroll-area.stories.tsx`
- `docs/plan/0019:subscription-preview-formatting/PLAN.md`
