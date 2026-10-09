# 集群长期历史数据仓库演进历史

> 这里记录影响后续实现的关键决策；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- Production sampling showed a reused 1.36 GiB WAL with only 22 current frames.
- Automatic checkpointing alone retains peak allocation; a retained-journal limit releases it.
- Safe reset preserves active frames, physical quota accounting and durable payloads.

- Live recovery exposed a refresh failure after concatenating two valid 64-gap responses.
- Preserve each wire page's bound and merge both inside the atomic checkpoint transaction.

- Production writes exposed migration rewind; two-store tests proved arrival-time hash drift.
- Canonical v3 negotiation and prefixed metadata preserve mixed-version safety.
- An independent bounded worker preserves migration progress across late writes.
- Retained-data verification is distinct from complete-history convergence.

- Production v3.43.1 recovery exposed a tombstone-first page that preempted the armed connections
  gap. The follow-up corrects eligible handoff selection and preserves recovery binding when a
  different stream finishes.

- Container recovery reads existing identity through explicit `--data-dir`, without `xp.env`.

- Recovery of a stale retained-anchor checkpoint is an explicit one-time generation on the existing
  `history.sqlite3`; it never creates a second database, changes quota, deletes source outbox rows,
  or rewrites permanent gaps.
- Deep verification uses additive SQLite 4096-sequence Merkle blocks with resumable dirty-block
  rebuilding and v1 peer fallback.

- 选择 SQLite 作为普通节点和仓库的统一本地存储；迁移必须可回退且不改变普通节点数据策略。
- 选择 Zstandard level 1 作为新同步唯一压缩算法；小 payload 或压缩无收益时使用 identity。既有 GZIP 仅用于嵌入式 Web 静态资源。
- 将目标节点注册的公网 HTTPS `api_base_url` 作为 History 同步唯一 direct path；公网失败时保留
  durable checkpoint/outbox，等待有界重试，不打开 History Mesh、Reverse 或 dynamic relay。
- 将临时传输故障和有界 outbox 满载定义为可恢复积压；只有 source 与 ready 仓库均无法再提供已过期
  cursor 范围时才定义为永久 gap。
- 选择 eventual consistency、source/observer 双身份、tombstone 和 anti-entropy，而不是 quorum 或 last-write-wins。
- 将 raw IP 限制为短期细节并长期匿名聚合，避免仓库无限膨胀和不必要的隐私暴露。

- Expired armed-recovery repair pages retain existing IDs and add bounded current IDs.
  Refresh commits gaps and checkpoint atomically, preserves generation, and re-reads the same
  summary cursor after draining so a partial refresh cannot skip current segments.

- Tiered exports now use canonical SQLite retention-start ordering and continuation.
- Old raw-time continuations use bounded idempotent restart to prevent skipped rows.
- Original observation time, source identity and recovery generation remain unchanged.

## Key Reasons / Replacements

- 本主题新增一个长期数据边界，不 supersede 既有 node history、traffic 或 Mesh Spec；它们作为输入和兼容约束继续有效。
- Issue #248 的多 Ticket Initiative 采用 SQLite 基座、控制面、传输、复制和管理集成五个顺序 Wave，以降低跨模块公共契约变更风险。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- Issue #248: https://github.com/IvanLi-CN/xp/issues/248
