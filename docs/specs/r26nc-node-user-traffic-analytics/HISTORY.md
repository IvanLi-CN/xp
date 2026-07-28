# 节点与用户 Traffic 统计演进历史（#r26nc）

> 这里记录影响长期理解的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-07-28: 将 Node 与 User Traffic 统一为 UTC 时间轴；保留 49 小时五分钟 rollup 与 90 天 daily rollup，取消 hourly rollup。
- 2026-07-28: 使用共享 Xray counter delta 同时更新节点和用户-节点统计；周期总览使用恒定空间的当前周期累加器。
- 2026-07-29: 采样缺口不再跨桶差分；用户 fan-out 按 UTC 桶时间对齐并在任一节点缺失时保持 null，删除用户时向集群节点清理本地历史。

## Key Reasons / Replacements

- 本 spec 扩展既有 `k7m2n` 节点历史 fallback 的 daily 数据来源，但不改变 runtime fallback 的职责。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `../k7m2n-node-history-fallback/HISTORY.md`
