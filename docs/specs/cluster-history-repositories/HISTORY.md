# 集群长期历史数据仓库演进历史

> 这里记录影响后续实现的关键决策；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 选择 SQLite 作为普通节点和仓库的统一本地存储；迁移必须可回退且不改变普通节点数据策略。
- 选择 Zstandard level 1 作为新同步唯一压缩算法；小 payload 或压缩无收益时使用 identity。既有 GZIP 仅用于嵌入式 Web 静态资源。
- 将 Reality Mesh 与 Cloudflare Tunnel 定义为同级 direct path；`XP_MESH_PROXY_URL` 保持本机公网出站兼容语义，不参与仓库拓扑。
- 选择 eventual consistency、source/observer 双身份、tombstone 和 anti-entropy，而不是 quorum 或 last-write-wins。
- 将 raw IP 限制为短期细节并长期匿名聚合，避免仓库无限膨胀和不必要的隐私暴露。

## Key Reasons / Replacements

- 本主题新增一个长期数据边界，不 supersede 既有 node history、traffic 或 Mesh Spec；它们作为输入和兼容约束继续有效。
- Issue #248 的多 Ticket Initiative 采用 SQLite 基座、控制面、传输、复制和管理集成五个顺序 Wave，以降低跨模块公共契约变更风险。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- Issue #248: https://github.com/IvanLi-CN/xp/issues/248
