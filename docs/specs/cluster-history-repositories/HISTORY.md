# 集群长期历史数据仓库演进历史

> 这里记录影响后续实现的关键决策；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 无条件排除本地 standby 会阻断 Ready US 的连续接收；保留 Ready 本地备用，失败后下一周期回到 primary，源 epoch 与未 ACK 原文不变。
- 持续写入会让等待空页的摘要完成位饥饿；最后一页提交后即检查 dirty 集合并持久化完成，保留有界预算、旧 peer fallback 与永久 gap 语义。

- 修复 source delivery 在公网 primary 恢复后仍沿用本地 standby 的状态循环：若选择结果是本地仓库而
  primary 是远端，worker 回退到已注册的公网 HTTPS primary，避免已前进的 source chain 在本地接收器上
  触发 `HashChainMismatch`；pending outbox、源连续性和恢复 generation 均保持不变。

- 101 的 consumed recovery generation 继续使用已有 summary checkpoint，不重新 arm generation；
  repair 每次尝试把 pending 数量、游标、进度时间、响应身份变化、明确不可用、无进展、HTTPS
  失败和 SQLite 存储失败写入现有控制快照。无进展响应只保留 pending 并按固定预算重试，避免
  长期停在无诊断的 `64`；永久 gap、`partial` 和 `replica_converged=false` 语义不变。
- 兼容旧版 consumed summary checkpoint：64 个 pending ID 与下一页游标不一致时原地标记同页
  revisit，避免跳过同一摘要页中尚未修复的 ID；generation、源绑定、水位和永久 gap 不变。
- Follow-up 修复让同一 consumed checkpoint 在首次锚点尝试留下精确非永久 sequence gap 后，
  仅凭 source 的 `history_truncated=true` 封存该已观测范围并继续当前 pending page；未知或
  新范围仍 fail closed，不会重新 arm generation 或创建第二次 handoff。
- 后续生产观察发现保留窗口会在延迟重试期间继续前移。新增有界、独立摘要绑定的
  `retained_stream_boundaries`，只有源端声明的最小保留序号与首个返回 segment 精确相等时，
  consumed checkpoint 才能原地封存新证据范围；v1 `response_id` 和旧 peer fallback 保持不变。
- 修复服务端单个 segment 超出 256 KiB wire 预算时的静默空响应：在生成 response identity
  前返回 storage error，避免接收方把空响应误判为可消费的 repair 页；不改变历史行、配额或
  source outbox。

- A restart regression reproduced a completed handoff with its predecessor receiver watermark.
- Handoff completion now snapshots the receiver in the same transaction as gap and consumption.
- Storage-failure coverage verifies rollback and a successful retry without another crossing.
- Production repair returned expired original IDs immediately after the final export page.
- Keep the existing bounded export lease during the handoff and let it expire naturally.
- Signed recovery may finish an exactly audited predecessor partial commit before a new generation.
- Recovery apply tolerates live capacity telemetry changes and keeps the admission guard.

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
- v3 block rebuilds normalize the serialized record wrapper before hashing so replica-local
  `received_at_unix_seconds` cannot make otherwise identical retained records diverge.
- A persisted canonical payload revision marks predecessor v3 blocks dirty once and rebuilds them
  in bounded pages without re-arming recovery.

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

- History delivery throughput is now measured from durable cumulative generated, source-ACK and
  repository-apply counters. Counters commit with their journal or receiver state, remain additive
  for old peers, and expose categorized transport, response, unavailable and storage failures.
  Source and repository workers use bounded 5-second scheduling. Local source delivery drains at
  most sixteen four-segment working pages while remote protocol pages retain the 256-segment and
  1 MiB wire limits. A 300-second sample must prove both delivery stages at 10x generated segment
  and byte rates; pending depth alone is not a throughput claim.
- Runtime status also exposes the completed or elapsed 300-second window for each source/stream/
  target scope, preventing aggregate progress from masking a starved target.
- Original observation time, source identity and recovery generation remain unchanged.

## Key Reasons / Replacements

- 本主题新增一个长期数据边界，不 supersede 既有 node history、traffic 或 Mesh Spec；它们作为输入和兼容约束继续有效。
- Issue #248 的多 Ticket Initiative 采用 SQLite 基座、控制面、传输、复制和管理集成五个顺序 Wave，以降低跨模块公共契约变更风险。

[ADR 0019](../../adr/0019-history-handoff-completion-and-recovery-preflight.md) 继承 ADR 0018。

它明确旧版部分提交的审计修复、最终页有界 repair grace 和 signed voter/version/quorum preflight。生产验收不以单元测试替代。

[ADR 0022](../../adr/0022-signed-retained-stream-boundary.md) 记录 delayed consumed-generation
repair 的源端保留边界证明。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- Issue #248: https://github.com/IvanLi-CN/xp/issues/248
