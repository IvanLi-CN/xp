# Raft quorum 丢失：单节点 leader 灾难恢复（xp-ops）（#c2v6d）

## 状态

- Status: 待实现
- Created: 2026-02-05
- Last: 2026-02-05
- PR: -

## 背景 / 问题陈述

当 Raft 集群失去多数派（quorum）时：

- 无法选举 leader；
- follower 的写入会被转发到（不可达的）leader，导致管理动作与配置同步全部失败；
- 在 2-voter 的集群中，一台 voter 被重置/永久丢失后，剩余节点将永远无法自行恢复 leader。

线上场景中，`hinet` 节点已重置且数据丢失，属于“多数派永久不可恢复”的灾难恢复，需要一条**正规、可审计**的流程来把幸存节点恢复为可写的 leader，再把被清空节点以 join 流程重新纳入集群。

## 目标 / 非目标

### Goals

- 提供 `xp-ops` 的灾难恢复命令：在“quorum 永久丢失”时，将幸存节点的 Raft membership 收缩为**单节点 voter**，使其可重新选举为 leader。
- 配置来源：以 `/etc/xp/xp.env` 为准（尤其是 `XP_DATA_DIR / XP_NODE_NAME / XP_API_BASE_URL`），并将 `NodeMeta.api_base_url` 更新为配置文件中的值，避免继续指向旧域名。
- 默认创建本地备份，且要求显式确认（`-y/--yes`），防止误触发。
- 不直接调用/修改 Cloudflare；不新增 Web UI / 公开管理员 API 的“恢复”入口。

### Non-goals

- 不尝试“保留被重置节点上的独有状态”：该节点已丢失数据，可能存在不可逆数据丢失。
- 不在本计划内实现自动化的脑裂检测或自动恢复；该操作必须由管理员显式执行。
- 不在本计划内改动 join/证书流程，仅提供恢复 leader 的前置条件。

## 范围（Scope）

### In scope

- `xp-ops xp recover-single-node`：
  - 读取 `/etc/xp/xp.env`；
  - 默认备份 `${XP_DATA_DIR}/raft`；
  - 重写本机 Raft 持久化中的 membership（必要时同步更新 snapshot meta）；
  - 输出明确警告与下一步操作指引（例如重启服务）。
- 增加可复现的本地回归：基于 `scripts/dev/subscription-3node-compose/` 模拟 “quorum 丢失 → 单节点恢复 → 被清空节点重新 join”。
- 文档更新：补充 ops 文档中的灾难恢复章节与安全告警。

### Out of scope

- Web UI / 公开管理员 API：不提供任何“写 Raft/membership”的入口。
- 云侧 DNS/Tunnel 自动变更：生产环境仍仅由 `xp-ops` 常规 deploy/sync 流程管理。

## 需求（Requirements）

### MUST

- `xp-ops` 提供命令：
  - `xp-ops xp recover-single-node -y`
  - 支持 `--dry-run`
  - 支持 `--no-backup`（不推荐，但允许）
- 备份策略：
  - 默认创建 `${XP_DATA_DIR}/raft.bak-<timestamp>/`（或等价目录），内容为当前 `raft/` 的完整拷贝。
- 安全策略：
  - 未提供 `-y/--yes` 时必须失败退出（避免误用）。
  - 必须输出“该操作不安全、仅在 quorum 永久丢失时使用”的提示。
- 行为：
  - 将 Raft membership voters 收缩为“仅当前节点”。
  - 当前节点在 membership 的 `NodeMeta` 必须被更新为 `XP_API_BASE_URL` 对应的值。
  - 若 snapshot meta 存在，必须同步更新其 `last_membership`，避免重启后回滚到旧 membership。
- 回归验证：
  - 在 docker compose 3 节点环境中，模拟停止 2 个节点后，运行恢复命令，然后验证存活节点最终成为 leader；
  - wipe 一个节点数据后，能通过 join token 重新加入集群并正确发现 leader。

### SHOULD

- 若 Raft 元数据缺失（例如 `raft/state_machine.json` 不存在），应以可读错误失败退出。
- 若当前节点不在现有 membership nodes map 中，应失败退出（避免写入无效 membership）。

## 验收标准（Acceptance Criteria）

- Given 3 节点 compose 集群已启动
  When 停止 2 个节点导致 quorum 丢失
  And 对剩余节点执行 `xp-ops xp recover-single-node -y` 并重启
  Then 剩余节点最终成为 leader（`/api/cluster/info.role=leader` 且 `leader_api_base_url` 指向自身）

- Given 其中一个节点被 wipe（数据目录清空）
  When 由恢复后的 leader 生成 join token 并对该节点执行 `xp join`
  Then 该节点可重新加入集群，且其 `/api/cluster/info.leader_api_base_url` 指向新的 leader

## 实现里程碑（Milestones）

- [ ] M1: `xp-ops` 新增 recover-single-node 命令（含默认备份 + dry-run + 防呆）
- [ ] M2: 在 compose 3 节点环境增加回归脚本覆盖该场景
- [ ] M3: ops 文档补充灾难恢复章节

## 风险 / 开放问题

- 风险：这是不安全的共识恢复；若被重置节点在丢失前存在仅它持有的已提交状态，将永久丢失。
- 风险：误执行会破坏原集群 membership；必须通过 `-y/--yes` + 默认备份降低误用成本。
- 开放问题：恢复后是否要提供“恢复成多 voter”的正规流程（当前通过 join 重新入群即可）。
