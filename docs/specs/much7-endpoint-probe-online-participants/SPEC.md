# Endpoint probe 按在线参与节点计分母（#much7）

## 状态

- Status: 已完成
- Created: 2026-03-11
- Last: 2026-03-11

## 背景 / 问题陈述

- endpoint probe 的小时桶原先把“当前集群节点总数”当作固定分母。
- 当某个节点在某小时根本没有接受/启动 probe run 时，该离线节点仍被算进分母，导致其他在线节点已经完整上报时，整小时仍会被判成 `missing`。
- 线上表现是：Endpoints 列表左侧 24h bar 显示 `missing`，但同一小时仍有 `latency_ms_p50` 与 per-node 样本，误导运维判断。

## 目标 / 非目标

### Goals

- 让 endpoint 健康判断只基于“该小时真正参与 probe run 的节点”。
- 离线节点若从未接受该小时 run，不得把该小时 endpoint bucket 强制打成 `missing`。
- 保持现有 `up / degraded / down / missing` 四个状态，不引入新状态值。
- 兼容现有 WAL 形状与旧 24h 历史数据，不要求 destructive reset。

### Non-goals

- 不通过运行时 API 反推“离线”定义；参与资格只由 probe run 是否被节点接受/启动决定。
- 不新增 probe UI 开关或 cluster-wide offline 策略配置。
- 不修改 endpoint probe 的真实探测目标、连通性判定或 self-test skip 语义。

## 范围（Scope）

### In scope

- `src/state.rs` 新增按小时记录 probe participants 的持久化状态，并在 24h 窗口内裁剪。
- 复用现有 `AppendEndpointProbeSamples { hour, from_node_id, samples }` 命令：空样本批次也要登记 participant。
- `src/endpoint_probe.rs` 在 fanout 前先发送一次空样本 append，声明“本节点已参与该小时 run”。
- `src/http/mod.rs` 的 endpoint 列表 / endpoint probe history 改用“参与节点数”做分母；legacy 小时桶在读路径上用同小时全 endpoint 样本 union 回填 participant 集合。
- Web stats / live run 页面切换到 participant 语义与文案。
- 相关 Rust/Vitest tests 与 ops/spec 文档同步。

### Out of scope

- 修改 probe run HTTP API 的启动/权限模型。
- 为历史任意更长窗口回补 participant 数据；仅修复当前 24h 保留窗口。
- 调整 Cloudflare / Xray / 节点 runtime 的离线检测逻辑。

## 需求（Requirements）

### MUST

- 一个节点只有在接受/启动某小时的 probe run 后，才计入该小时分母。
- 一个节点如果已登记参与，但缺少某个 endpoint 的样本，该 endpoint 小时桶必须继续是 `missing`。
- endpoint probe history 必须暴露 participant 计数；旧 `expected_nodes` 字段作为兼容别名保留一版。
- legacy 24h 数据在新版本部署后必须立即按“同小时样本 union”恢复合理分母，不要求等待 24 小时自然刷满。
- live run 页面聚合状态时，只能把 requested/progress 节点算作参与节点；`busy` / `not_found` / 传输错误节点不得进入分母。

## 功能 / 行为规格

### 后端状态与写入

- 新增 `endpoint_probe_participants_by_hour: BTreeMap<String, BTreeSet<String>>`。
- `AppendEndpointProbeSamples` apply 时，先把 `from_node_id` 写入 `endpoint_probe_participants_by_hour[hour]`，再写 endpoint 样本。
- participants 与 endpoint probe history 都只保留最近 24 个小时桶。

### 历史读取与聚合

- 某小时 participant 集合 = `endpoint_probe_participants_by_hour[hour]` 与“同小时所有 endpoint 样本里出现过的 node_id”并集。
- `probe_status_for_counts` 的分母改为 `participating_nodes`：
  - `participating_nodes == 0` -> `missing`
  - `sample_count == 0` -> `missing`
  - `sample_count < participating_nodes` -> `missing`
  - 其余继续按 tested 样本计算 `up / degraded / down`
- Endpoints 列表的 summary slot 与 `/probe-history` 必须共享同一 participant 语义，避免页面间口径漂移。

### Web 行为

- stats 页面把 `Expected nodes` 改为 `Participating nodes`。
- stats 页面每个小时条的 `Reported x/y` 使用该 slot 的 participant 分母，而不是固定集群节点数。
- live run 页面用 requested/progress 节点数（加 SSE 已回报样本节点兜底）计算临时聚合状态，保证离线/忙碌节点不会把 live 结果误判为 `missing`。

## 验收标准（Acceptance Criteria）

- Given 3 节点集群中有 1 个节点离线且没有接受某小时 probe run，When 另外 2 个节点都成功上报同一 endpoint，Then 该小时状态为 `up`，不是 `missing`。
- Given 3 节点集群中有 1 个节点离线，When 另外 2 个参与节点对同一 endpoint 一成一败，Then 该小时状态为 `degraded`。
- Given 一个节点已经登记参与某小时 run，When 它缺少某个 endpoint 的样本，Then 该 endpoint 小时桶仍为 `missing`。
- Given 历史 24h 数据来自旧版本且没有 participant metadata，When 新版本读取 `/api/admin/endpoints/{id}/probe-history`，Then 分母从同小时全 endpoint 样本 union 推导，避免把离线但未参与的当前节点硬算进分母。
- Given live run 页面同时存在 2 个 requested 节点和 1 个 `busy` 节点，When 2 个 requested 节点都回报同一 endpoint 成功，Then 页面显示 `Up`，不是 `Missing`。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 增加 per-hour participant 持久化与兼容写入路径
- [x] M2: 切换 endpoint summary/history 与 web 页面到 participant 分母
- [x] M3: 补齐 legacy fallback、Rust/Vitest tests、ops/spec 文档同步

## 测试与验证

- Rust：`cargo test endpoint_probe -- --nocapture`
- Web：`cd web && bun run test src/utils/endpointProbeStatus.test.ts src/views/EndpointProbeStatsPage.test.tsx src/views/EndpointProbeRunPage.test.tsx`

## 风险 / 开放问题 / 假设

- 风险：rolling upgrade 期间，旧版本节点无法提前登记 participant；当前实现通过“participant map + 同小时样本 union”并集，尽量降低跨版本窗口的误判。
- 风险：顶层 history 响应仍保留单个 `participating_nodes` 兼容字段；真正用于逐小时展示的分母在 slot 级别。
- 假设：离线节点的正确定义是“该小时从未接受/启动 probe run”，而不是运行时组件状态本身。

## 变更记录（Change log）

- 2026-03-11: 创建 much7 规格，冻结“离线节点不参与 endpoint probe 分母”的实现口径。
- 2026-03-11: 完成 participant 持久化、legacy fallback、Web 文案/聚合与 targeted Rust/Vitest tests。
