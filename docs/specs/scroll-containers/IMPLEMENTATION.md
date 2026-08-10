# Web 滚动容器规范实现状态

> 当前有效规范以 `./SPEC.md` 为准；这里记录实现覆盖与当前状态，关键演进原因见 `./HISTORY.md`。

## Current Status

- Implementation: 已完成组件基线记录；采用范围按面板改造逐步收敛。
- Lifecycle: active
- Catalog note: `ScrollArea` 是有界纵向滚动的唯一现成组件。

## Coverage / rollout summary

- `web/src/components/ui/scroll-area.tsx` 使用 Radix 的 root、viewport、vertical scrollbar 和真实 thumb。
- `web/src/components/ui/scroll-area.stories.tsx` 提供长列表状态，供 light/dark 主题下人工检查滚动条外观。
- `web/src/components/ResourceNavigation.tsx` 的对象二级导航以固定十行高度采用
  `ScrollArea`；活动对象沿用最近边界、无动画的 viewport 定位。
- 历史原生 `overflow-*` 面板不要求纯文档变更后的立即迁移；新增或实质改造时应按 `SPEC.md` 采用该组件。

## Remaining Gaps

- 横向或双向滚动仍没有通用 `ScrollArea` API；需要该能力的领域应在独立规格中定义后再扩展组件。

## Related Changes

- `web/src/components/ui/scroll-area.tsx`
- `web/src/components/ui/scroll-area.stories.tsx`
- `web/src/components/ResourceNavigation.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`
