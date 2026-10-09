# ADR 0019: Atomic History Handoff Completion and Recovery Preflight

Status: accepted. Supersedes [ADR 0018](0018-history-repository-recovery-generation.md).

## Context

生产 101 已完成 tiered export，但 predecessor 将 consumed generation 和交接审计提交后，遗漏 receiver watermark 的持久化。

导出完成后立即删除 lease，还可能让 retention 删除后续 repair 所需原文。

直接重置 generation 会丢失单次跨越的安全边界。

## Decision

保留 ADR 0018 的原地恢复、单次跨越、增量 v3 摘要和永久 gap 语义。

交接完成必须在同一事务提交 receiver watermark、gap、checkpoint 和 generation consumption。

失败恢复全部内存状态。

签名本地 recovery 只修复有精确持久审计证据的旧失配。

须同时满足：完成 export、非零 consumed generation、匹配审计、无 active handoff，以及精确 predecessor watermark。

dry-run 展示实际水位及 proposed repair，apply 原子提交已声明缺口与新 generation。

不重启旧 generation，不推断缺失原文存在。

完整 checkpoint、容量及集群事实绑定进 fingerprint。

最终 tiered page 保留既有 lease，截止时间不晚于原到期时间与完成后 15 分钟。

完成不创建或延长 lease。

自然到期恢复普通 retention。

选择有界 repair grace，以已有最多四个 lease 的 admission 边界限制保留成本，不改变 retention 配置。

恢复前通过注册公网 HTTPS 查询每个当前 voter 的签名 preflight。

要求版本、term、leader 和 membership revision 一致，由 leader 完成 linearizable quorum 检查。

缺失、旧版本、不一致、joint membership 或无法证明 quorum 均 fail closed。

fingerprint 同时绑定 repository membership。

提交前重新检查本地集群视图和 Ready peer。

只读 preflight 不更新持久化 transport 状态，不处理无关 learner。

## Consequences

现有 SQLite 与控制快照原地前向修复，无额外数据库、手工 checkpoint、源 outbox 删除、full VACUUM、配额或卷扩容。

旧节点必须滚动升级完毕，随后以新 signed dry-run 的 fingerprint apply。

状态变化须重新预览。

repair grace 到期后仍可留下真实永久 gap；不会以缺乏原文证据的水位修复宣称完整历史。生产 Ready 五分钟稳定窗口与双仓深校验仍需独立验收。
