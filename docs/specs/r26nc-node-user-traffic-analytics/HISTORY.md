# 节点与用户 Traffic 统计演进历史（#r26nc）

> 这里记录影响长期理解的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-07-28: 将 Node 与 User Traffic 统一为 UTC 时间轴；保留 49 小时五分钟
  rollup 与 90 天 daily rollup，取消 hourly rollup。
- 2026-07-28: 使用共享 Xray counter delta 同时更新节点和用户-节点统计；周期总览
  使用恒定空间的当前周期累加器。
- 2026-07-29: 采样缺口不再跨桶差分；用户 fan-out 按 UTC 桶时间对齐并在任一节点
  缺失时保持 null，删除用户时向集群节点清理本地历史。
- 2026-07-30: 将 Traffic 与 TCP 的 SVG tooltip palette、虚线 axis pointer 和静态
  line emphasis guard 收敛为共享底座；保留 IP usage 的跨视图联动语义，不将其降级为
  静态 hover。
- 2026-07-30: palette 在恢复持久化主题后于绘制前重算，避免首屏沿用根节点切换前的
  CSS token 颜色。
- 2026-07-30: 视觉证据从带有截图装饰风险的 standalone 组件画布切换为 Node details
  完整页面 Storybook fallback；使用真实 hover 和原生移动端 section select，避免人为色框。

## Key Reasons / Replacements

- 本 spec 扩展既有 `k7m2n` 节点历史 fallback 的 daily 数据来源，但不改变 runtime fallback 的职责。
- ECharts tooltip 默认样式与 CSS token 字符串不能作为 SVG 主题契约；新静态图表
  必须使用共享 palette、tooltip surface 和 emphasis guard，并通过真实 pointer hover
  证明路径仍可见。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `../k7m2n-node-history-fallback/HISTORY.md`
- `../../solutions/web/echarts-svg-tooltip-theming-hover-stability.md`
