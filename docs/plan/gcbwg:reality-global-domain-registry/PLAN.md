# Global REALITY Domain Registry（全集群共享域名池）+ Endpoint 绑定 + 自动 Rebuild Inbound（#gcbwg）

## 状态

- Status: 待实现
- Created: 2026-02-17
- Last: 2026-02-17

## 背景 / 问题陈述

- VLESS REALITY 的 `server_names` 是 TLS SNI（伪装域名）候选集合；在生产环境中，我们希望：
  - 统一维护一份**全集群共享**的域名池，避免每个 endpoint 重复手工填写与漂移；
  - 支持按节点启用/禁用（同一域名在不同网络环境可用性不同）；
  - Subscription 输出时随机挑选一个可用 SNI，提升抗封锁与容错。
- 现状：
  - endpoint 的 `server_names` 仅能逐个配置，不便全局管理；
  - reconcile 对已存在 inbound 仅 `add_inbound`（already-exists 不更新配置），导致 `server_names/dest` 变更后可能**不生效**。

## 目标 / 非目标

### Goals

- Backend：
  - 新增全局域名池（按节点开关、可排序、可 CRUD），并通过 Raft 持久化与全集群共享；
  - endpoint 支持 `server_names_source = manual/global`：
    - `global`：`server_names/dest` 由“全局域名池 + 节点过滤 + 顺序(primary)”派生；
    - `manual`：保持现有手动 `server_names`（但后端会强制 `dest = primary + ":443"` 自洽）；
  - 当域名池或 endpoint reality 配置变化时，reconcile 能自动检测配置变化并对本节点相关 inbound **rebuild**（remove+add），保证配置立即生效。
- Web Admin：
  - 新增 Settings 页面管理全局域名池（批量添加、排序、按节点启用/禁用）；
  - Endpoint New/Details 支持选择 `serverNames source`，global 模式下 `server_names` 只读展示派生结果。

### Non-goals

- 不保证 “中国大陆直连一定可用”；域名池的可用性需要在真实节点网络里通过 probe 验证与逐步调整。
- 不改变 endpoint probe 的 primary 语义：仍使用 `server_names[0]`。

## 需求（Requirements）

### MUST

- 数据结构：
  - 新增全局 `reality_domains[]`（domain_id / server_name / disabled_node_ids）；
  - `RealityConfig` 增加 `server_names_source`（default=manual）。
- 规则：
  - `server_names[0]` 是 primary：
    - `dest` 固定派生为 `${server_names[0]}:443`
    - probe 继续使用 primary
  - `server_name` 校验严格一致（前端 + 后端都拒绝：空白、URL、path、port、wildcard、单字母 TLD 等）。
- 订阅输出：
  - 沿用现有策略：对每个 VLESS endpoint 从 `server_names` 随机挑选 1 个输出到 raw `sni=` 与 clash `servername`。
- 工程可靠性：
  - reconcile 必须能在配置变化时对 inbound rebuild，避免 “state 变了但 xray 未更新”。

### SHOULD

- 全局域名池升级后自动 seed 2-3 个 OneDrive 域名候选（确定性 domain_id，避免多节点迁移差异）：
  - `public.sn.files.1drv.com`
  - `public.bn.files.1drv.com`
  - `oneclient.sfx.ms`

## 验收标准（Acceptance Criteria）

- 全局域名池：
  - 支持新增/删除/排序域名，支持按节点启用/禁用；
  - v4->v5 升级后能看到 seed 域名（当原先没有该字段时）。
- Endpoint：
  - 新建 VLESS endpoint 默认 `server_names_source=global`，且能成功创建（前提：该节点至少有 1 个启用域名）；
  - Details 页可切换 manual/global；global 下 server_names 只读展示派生列表。
- 生效性：
  - 调整域名池（顺序或节点开关）后，本节点相关 inbound 会被自动 rebuild，xray 配置立即生效；
  - 不发生周期性 flapping（仅在配置 hash 变化时 rebuild）。

## 测试与验证（Testing）

- Rust：`cargo test`
- Web：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - `cd web && bun run test`

## 风险与注意事项（Risks）

- State schema bump（v5）要求全集群同时升级；旧版本会因 schema_version mismatch 无法启动。
- reconcile rebuild 会带来短暂中断（remove+add），需要确保仅在必要时触发，并避免由非确定性序列化导致的重复 rebuild。

## 里程碑（Milestones）

- [ ] M1: docs freeze + schema/migration/AC 冻结（本计划文档）
- [ ] M2: Backend state + admin API（CRUD/reorder）+ endpoint meta normalization
- [ ] M3: Reconcile 配置变化检测 + inbound rebuild
- [ ] M4: Web Settings 页 + Endpoint source selector + storybook/mocks
- [ ] M5: Rust/Web tests 全绿 + PR checks 结果明确
