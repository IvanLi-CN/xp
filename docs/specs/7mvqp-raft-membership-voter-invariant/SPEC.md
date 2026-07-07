# Raft membership voter invariant (#7mvqp)

## 状态

- Status: 已完成
- Created: 2026-07-07
- Last: 2026-07-07

## 背景 / 问题陈述

- Raft membership 曾允许节点已经存在于 `membership.nodes`，但没有出现在 `voter_ids`。
- 这种长期 learner 分化会让管理员看到“三个节点”，但实际只有两个 voter；任一 voter 失联后集群无法选主，写入也无法提交。
- Join API 如果在 `add_learner` 后提前返回成功，会把“节点存在但不能投票”暴露成稳定生产状态。

## 目标 / 非目标

### Goals

- 稳定 membership invariant：`membership.nodes` 中每个节点都必须是 voter。
- `POST /api/cluster/join` 只有在新节点进入 voters 后才返回成功。
- leader 启动和运行时周期性检查 `membership.nodes - voter_ids`，发现遗留 learner 时尝试 promote。
- 无 leader/quorum 时只报告需要恢复，不自动重写 Raft 磁盘状态。
- 运维文档明确 2-voter 拓扑不可接受，生产建议至少 3 个稳定 voter。

### Non-goals

- 不新增 `can_vote`、`voter=false`、观察者、只读节点、非投票节点或 UI 配置。
- 不支持长期 learner 作为 v1 生产角色。
- 不在无 quorum 时自动执行危险恢复。
- 不把失联节点强行保留在 membership 里等待以后恢复。

## 功能与行为规格

- Join leader 校验 token、签发证书、写入 state node，等待 learner 追平，并同步执行 `add_voters`。
- learner 追平或 `add_voters` 失败时，Join 返回错误，并尽力移除刚加入的 Raft learner 与 state node。
- 旧版本留下的 `membership.nodes - voter_ids` 由 leader-side guard 自动调用 `add_voters` 修复。
- follower、candidate 或无 leader 状态不能伪修复 membership，只输出明确日志信号。
- 删除/恢复节点路径必须维持稳定 voter invariant：恢复被删除的 voter 时必须重新加入 voters。

## 验收标准

- `POST /api/cluster/join` 返回 200 时，新节点已经被提交为 voter。
- 模拟 `add_voters` 失败时，join 不返回成功，并且不留下稳定 state node。
- guard 能识别遗留 non-voter membership node，并在 leader 上调用 `add_voters`。
- guard 在 follower/no leader 状态不修改 membership。
- 仓库内不存在新增的“是否投票”配置、字段、环境变量或 UI 控件。

## 文档更新

- `docs/desgin/cluster.md` 定义稳定节点全部 voter、无可配置投票权。
- `docs/ops/README.md` 定义丢失 quorum 后的显式恢复与重新 join 合同。
