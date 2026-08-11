# Web 滚动容器规范实现状态

> 当前有效规范以 `./SPEC.md` 为准；这里记录实现覆盖与当前状态，关键演进原因见 `./HISTORY.md`。

## Current Status

- Implementation: 已完成组件基线记录；采用范围按面板改造逐步收敛。
- Lifecycle: active
- Catalog note: `ScrollArea` 是有界纵向滚动的唯一现成组件。

## Coverage / rollout summary

- `web/src/components/ui/scroll-area.tsx` 使用 Radix 的 root、viewport、vertical scrollbar 和真实 thumb。
- `web/src/components/ui/scroll-area.stories.tsx` 提供长列表状态，供 light/dark 主题下人工检查滚动条外观。
- `web/src/components/ResourceNavigation.tsx` 的对象二级导航以自然内容高度、最多十行采用
  `ScrollArea`；连续内容列通过 `w-0 min-w-full` 固定在 viewport 宽度内，活动对象沿用最近边界、
  无动画的 viewport 定位，资源组以单组手风琴方式展开。
- `web/src/components/ResourceNavigationChildLink.tsx` 将长名称位移限制在名称自身的 overflow
  viewport，不扩展 `ScrollArea` 的横向能力；reduced motion 下改用项目 Tooltip。
- `web/src/components/ResourceNavigation.stories.tsx` 覆盖长名称、完整选中胶囊、外层无横向
  overflow 及 reduced-motion fallback。
- 历史原生 `overflow-*` 面板不要求纯文档变更后的立即迁移；新增或实质改造时应按 `SPEC.md` 采用该组件。

## Remaining Gaps

- 横向或双向滚动仍没有通用 `ScrollArea` API；需要该能力的领域应在独立规格中定义后再扩展组件。

## Related Changes

- `web/src/components/ui/scroll-area.tsx`
- `web/src/components/ui/scroll-area.stories.tsx`
- `web/src/components/ResourceNavigation.tsx`
- `web/src/components/ResourceNavigationChildLink.tsx`
- `web/src/components/ResourceNavigation.stories.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`
