# History

- The topic originated in legacy Spec `#much7`.
- It records the completed participant-based denominator change for endpoint probes.

## 变更记录（Change log）

- 2026-03-11: 创建 much7 规格，冻结“离线节点不参与 endpoint probe 分母”的实现口径。
- 2026-03-11: 完成 participant 持久化、legacy fallback、Web 文案/聚合与 targeted Rust/Vitest tests。
- 2026-07-28: stats 与 live run 的节点结果改为名称优先、详情可跳转、名称排序，并保留 ID 回退语义。
- 2026-11-02: 临时 Xray 探测进程绑定 XP 生命周期，避免 XP 异常退出后遗留进程持续消耗低内存节点。
