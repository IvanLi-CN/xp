# Endpoint probe：节点可跳过 self-test（hairpin 逃生舱）（#7ze9h）

## 状态

- Status: 待实现
- Created: 2026-02-20
- Last: 2026-02-20

## 背景 / 问题陈述

Endpoint probe 的语义是“每个节点对每个 endpoint 走真实 ingress 路径发起 HTTPS 探测”，其中包含 self-test：

- self-test 会从 endpoint 所在节点访问自身 `access_host`（域名 -> 公网 IP -> 回到自身）。

在部分网络环境/云厂商上，NAT hairpin（访问自己的公网 IP 回到自己）不可用，导致：

- endpoint 对外可用（其它节点探测 OK），但 endpoint 所在节点 self-test 永远失败；
- UI 聚合后表现为 `degraded/down`，造成误报与噪声。

## 目标 / 非目标

### Goals

- 增加一个节点本地配置：`XP_ENDPOINT_PROBE_SKIP_SELF_TEST=true`（默认 false）。
- 当开启该配置时，该节点对“自己托管的 endpoints”（`endpoint.node_id == local_node_id`）不执行真实探测：
  - 不尝试连接 `access_host`；
  - 不启动 Xray client；
  - 写入一条样本 `skipped=true`，以表示“已上报但不参与成功/失败判断”。
- 聚合状态计算需把 `skipped` 视为第三态：
  - `Up`：所有非 skipped 的测试样本均 OK；
  - `Degraded/Down`：仍按非 skipped 样本的真实结果判断；
  - `Missing`：如果某小时 bucket 只有 skipped（tested_count == 0），必须为 Missing（禁止全跳过显示 Up）。
- Admin UI 在 per-node results 明确展示 `SKIP`（而不是 OK/FAIL），避免误导。

### Non-goals

- 不尝试在产品内修复/绕过 provider 的 NAT hairpin（网络层问题应由运维处理）。
- 不引入“全局跳过某节点/某 endpoint”的策略（仅限 self-test escape hatch）。
- 不把 skip 行为纳入 probe config hash（该配置允许仅在个别节点开启）。

## 范围（Scope）

### In scope

- Probe 样本结构新增字段 `skipped`（向后兼容：`#[serde(default)]`）。
- Backend：
  - probe runner 支持按节点配置跳过 self-test，并写入 `skipped=true` 的样本；
  - probe 聚合状态计算支持 skipped；
  - history/slot 增加 `skipped_count` / `tested_count` 便于 UI 展示。
- Web：
  - SSE/live run 页面与 stats 页面都支持 skipped 语义与展示（SKIP badge）。
- Ops 文档补充配置说明（并保留“禁止 loopback special-casing”的硬规则）。

### Out of scope

- 为 skip 提供 UI 开关（该配置仅为节点本地 env/flag）。
- 对 endpoint 实际可用性做额外外部观测/告警（不在本计划内）。

## 验收标准（Acceptance Criteria）

- Given 集群有 N 个 nodes / M 个 endpoints，
  When 某个 node 开启 `XP_ENDPOINT_PROBE_SKIP_SELF_TEST=true` 并触发一次 probe run，
  Then
  - 该 node 对所有 `endpoint.node_id == local_node_id` 的 endpoints 上报样本 `skipped=true`；
  - 该小时 bucket 的 `sample_count` 仍可达到 `expected_nodes`（skipped 计入“已上报”）；
  - status 计算按 “tested_count = sample_count - skipped_count” 的规则执行：
    - tested_count==0 => Missing；
    - ok_count==tested_count => Up；
    - 其余按 Degraded/Down 真实反映；
  - UI 的 per-node results 对 skipped 样本显示 `SKIP`。

## 测试与验证（Testing）

- Rust：
  - `cargo test`
  - 覆盖 skipped 聚合规则（含 tested_count==0 -> Missing）。
- Web：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - `cd web && bun run test`

## 风险与开放问题

- 开启 skip 会降低“该节点视角”的覆盖度（少了 self-test 观测）。
  - 缓解：该配置默认关闭，仅用于明确已知 hairpin 受限的节点；其它节点仍会对该 endpoint 做真实探测。
