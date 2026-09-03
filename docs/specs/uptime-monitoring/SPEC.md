# 服务监控（Service Monitoring）

> 本文是当前规范。实现状态见 `./IMPLEMENTATION.md`，主题背景见
> `./HISTORY.md`，术语见 `./CONTEXT.md`。

## Context and Scope

### 背景 / 问题陈述

XP 已有节点、流量、连接数和 endpoint probe 历史，但没有管理员可配置的
远程服务可用性监控。普通节点的短期窗口不适合长期查询，
Raft 也不应承载高频 Observation。

本主题定义多节点远程检查、History Repository 长期持久化、分阶段 rollup，
以及独立服务监控工作台的稳定边界。

## 目标 / 非目标

### Goals

- 支持 HTTP、HTTPS、PING、TCPING 公网目标检查。
- 多个 Observer Node 从各自网络位置观察同一 Monitor。
- 使用现有 Source Delivery Journal 和 History Repository 保存完整已知历史。
- 提供独立的列表、配置、当前状态、历史图表和最近检查界面。
- 在 systemd、OpenRC、Docker/Compose 上保持同一能力合同。

### Non-goals

- v1 不含 Incident、通知、维护窗口、公开状态页或 WebSocket。
- 不支持私网目标、任意请求方法、headers、认证、脚本、代理，
  也不持久化正文。
- 不改写 endpoint probe、普通节点现有窗口或通用 Repository 同步协议。
- 不补跑错过 Slot，也不用隐式重试、降频或减少 Observer 改变统计分母。

## Related ADRs

- [ADR 0006](../../adr/0006-pause-monitoring-when-observations-cannot-be-captured.md)
- [ADR 0007](../../adr/0007-immutable-monitor-revisions-and-slotted-execution.md)
- [ADR 0008](../../adr/0008-repository-first-observation-persistence.md)
- [ADR 0009](../../adr/0009-observer-policy-and-draft-cluster-tests.md)
- [ADR 0010](../../adr/0010-same-origin-draft-test-forwarding.md)

## Requirements

### Control plane and scheduling

- REQ-MONITOR-REVISION: Monitor 使用 ULID；编辑创建递增不可变 revision，并在下一个
  UTC Slot 生效。
- 生命周期为 `active`、`paused`、`deleted`；删除只停止未来执行，
  历史自然过期。
- 最小周期为 60 秒，允许 `1m`、`5m`、`15m`、`1h`；Slot UTC 对齐。
- REQ-MONITOR-POLICY: Observer Policy 持久化为 `{ mode: exclude|include, node_ids }`。默认是
  `exclude` 空集合，解析为当前已注册且具备方法能力的全部 Observer Node；
  `exclude` 非空时只排除列出的 ID，`include` 时只选择列出的 ID 且不能为空。
  离开集群的 ID 保留在 policy 中，作为显式缺口而不静默删除。旧的
  `observer_node_ids: null` 迁移为空 exclude，旧非空数组迁移为 include。
- REQ-MONITOR-SNAPSHOT: 每个 Slot 固定其 Observer Node 集合，并将该快照连同 Observation
  进入 History Repository；未执行的已知 Slot 以压缩缺口范围持久化并进入同一 stream。进程重启不补跑，
  计划检查不自动重试。
- 默认 Observation Budget 是 300 个 `monitor × observer` Slot/分钟。
- 超预算写入返回 `observation_budget_exceeded`，不得静默改变配置。
- 每节点计划检查并发上限为 32；Ad Hoc run 使用独立限制。

### Check semantics and safety

- REQ-MONITOR-HTTP: HTTP 默认 GET、可选 HEAD；默认接受 200-399，
  支持状态范围、3 次重定向和 literal contains。
- REQ-MONITOR-HTTPS: HTTPS 必须校验证书、主机名与有效期；TLS 失败不能降级。
- 正文校验最多读取 64 KiB，配置文本最多 256 字节；正文不持久化。
- PING 每 Slot 发 3 个 Echo，至少一个响应即成功，记录丢包和延迟。
- TCPING 只执行一次 TCP connect，成功即关闭，不做 TLS 或应用层请求。
- 连接默认超时 5 秒，总超时 10 秒，单 Monitor 最大 30 秒。
- 创建、执行、重定向都重新解析地址；
  仅允许公网 IPv4/IPv6 与显式公网端口。
- loopback、私网、link-local、multicast、未指定和 metadata 地址必须拒绝。
- REQ-MONITOR-SAFETY: 执行器不使用环境代理、外部 ping 命令或 TCPING 作为 ICMP
  fallback。

### Persistence, rollup, and quality

- REQ-MONITOR-PERSISTENCE: 每个 Observation 先写 Source Delivery Journal，再投递到 History
  Repository。
- journal 无法接收记录时停止新检查并显示 `capture_suspended`；停止期间的已知 Slot 以有界、
  连续范围记录为 gap，不得探测后丢弃或把缺口伪装成未排期。
- uptime observation backlog 上限为 64 MiB 或 100,000 条，以先到者为准；压缩 gap
  range backlog 也以 100,000 条为界；两者都在 80% 停止新采集，并低于 60% 后恢复。
- Observation stream 为 `service_monitor_observation-v1`，
  复用签名、cursor、ack 与 fork 防护。
- REQ-MONITOR-ROLLUP: Repository 以 source epoch、stream、sequence 幂等接收，并保存完整已知
  并集。
- 保存 7 天分钟级、90 天五分钟级、随后小时级，默认最长两年。
- child 层必须先成功 rollup 并校验，再删除；
  rollup 必须可重放且不重复累计。
- bucket 保存 expected、executed、success、failure、unsupported、suspended 与延迟分布。
- 可用率为 `success / (success + failure)`；覆盖率为 executable / expected。
- unsupported、suspended、未采集 Slot 不计入可用率分母，
  但必须影响覆盖率或 quality。
- 当前状态：全成功 `up`、全失败 `down`、混合 `degraded`、无结果 `unknown`。
- 当前状态只使用最新、同 revision 且 Observer Set 完整的 Slot；缺少任一 Observer 或最新完整
  Slot 超过约两个周期时为 `unknown`；`capture_suspended` 是独立采集状态。
- 查询必须返回 complete、partial 或 local_only，
  以及 coverage、watermark、gap、skew、freshness。

### API and Web

- REQ-MONITOR-API: API 位于 `/api/admin/monitors`，沿用 admin token 与现有 Leader/follower
  写语义。
- 支持 list、create、get、patch、delete、run、run status、monitor status、history。
- 写请求使用 `expected_revision`；delete 不接受 purge；
  run 生成不计统计的 ad-hoc Observation。
- REQ-MONITOR-DRAFT-TEST: `POST /api/admin/monitor-draft-tests` 创建一次 leader-coordinated
  的临时
  Draft Cluster Test。创建时冻结 target、Observer Policy 与解析后的 Observer Set，
  每个节点以确定性 0--750ms 偏移错峰执行；`GET .../:run_id` 返回 queued、running、
  succeeded、failed、unsupported 或 interrupted 以及逐节点结果。run 只保留 15 分钟
  于协调节点的 `uptime.sqlite3`，不得写 Observation、journal、Repository 或 rollup。
  测试状态永远不禁用或改变 Monitor 创建请求；协调 Leader 变化导致无法恢复时返回
  `interrupted`，管理员可重新测试。浏览器从 follower 打开工作台时，create 与 status
  请求必须保持当前 origin：follower 先校验 admin token，再用签名内部请求转发到 Draft
  Test Coordinator；bearer token 不跨节点，浏览器不能收到跨 origin redirect。未能创建
  的请求返回 `leader_unavailable`，已创建但无法恢复的 run 返回 `interrupted`。
- history 自动选择 `1m`、`5m`、`1h`，
  最多约 1,500 点，并支持有界最近检查分页。
- REQ-MONITOR-WORKSPACE: Web 新增一级导航“服务监控”与 `/monitors`、new、detail、edit
  路由。
- 总览显示状态、目标、方法、延迟、最近 6 小时可用率/覆盖率、72 个五分钟连续性格、
  Observer 数和 quality。
- 详情显示状态时间线、延迟与统计图、Observer 结果、
  最近检查和 quality 横幅。
- 列表 30 秒轮询，详情状态 15 秒轮询；不引入 WebSocket。

## Verification

- VER-MONITOR-STATUS covers: REQ-MONITOR-SNAPSHOT: 两个 HTTP Observer 在同 Slot 都成功时，
  当前状态为 `up`，
  executed 与 success 都为 2。
- 同 Slot 一个成功、一个超时时状态为 `degraded`，错误为 `timeout`，
  两个分母各自正确。
- VER-MONITOR-REVISION covers: REQ-MONITOR-REVISION: URL 或条件修改后，下一个 Slot 使用新
  revision，旧 Observation 保留其 revision。
- journal 不可写时，不发送目标请求，并暴露 `capture_suspended` 或 quality gap。
- VER-MONITOR-SAFETY covers: REQ-MONITOR-SAFETY: PING 无能力时标记 unsupported，不改用
  TCPING。
- 私网初始解析或重定向地址在请求前被拒绝。
- VER-MONITOR-QUALITY covers: REQ-MONITOR-PERSISTENCE, REQ-MONITOR-ROLLUP: Repository 不可用时
  历史返回 partial/local_only、watermark 与 gap，不伪装完整。
- 计划统计不包含 Ad Hoc Observation；过期状态显示 unknown 而非 down。
- 预算超过 300 Slot/分钟时，Leader 拒绝配置变更。
- VER-MONITOR-WORKSPACE covers: REQ-MONITOR-WORKSPACE, REQ-MONITOR-API: Web 可完成 CRUD、暂停、
  立即检查和历史查看，并明确呈现 quality。

- VER-MONITOR-POLICY covers: REQ-MONITOR-POLICY: 空 exclude 解析为全部当前节点，include 必须非空，
  旧数组迁移保留离开节点 ID。
- VER-MONITOR-DRAFT covers: REQ-MONITOR-DRAFT-TEST: Draft Test 使用冻结策略和节点集合，15 分钟后为
  interrupted，且不写 Observation 或 Repository；follower ingress 的 create/status
  保持同源、不出现 HTTP redirect，管理员 token 不进入内部转发 payload；同一幂等键的
  重试只创建一个 run。
- VER-MONITOR-TARGET covers: REQ-MONITOR-HTTP, REQ-MONITOR-HTTPS: HTTP/HTTPS executor 遵守方法、
  证书和响应体边界。

## 非功能性验收 / 质量门槛（Quality Gates）

- Rust 单测覆盖公共地址、执行器、状态/预算、journal、rollup、retention
  和 API contract。
- 集成测覆盖 SQLite migration、source 重放、Repository repair/quality 和重定向场景。
- Storybook 覆盖可复用状态组件；Playwright 覆盖主要 CRUD 与详情流程。
- systemd、OpenRC、Docker/Compose 均验证 ICMP capability 与持久化重启行为。
- 运行 `cargo fmt --check`、`cargo test`、`cargo clippy -- -D warnings`、
  Web 检查和 style budget。

## 实现前置条件（Definition of Ready / Preconditions）

- 复用现有 History Repository stream、cursor、签名、ready、quality
  与 retention 扩展点。
- 在实现前固定 API fixtures、错误枚举、histogram buckets 和 SQLite migration 编号。
- 在运维文档中写清三种部署路径的 ICMP capability、journal 水位与
  Repository prerequisite。

## 实现里程碑（Milestones）

1. 状态、SQLite schema、journal stream、签名与 rollup 数据结构。
2. 四类 executor、公网地址保护、ICMP capability 与 scheduler。
3. Repository receiver、quality、rollup、retention 和 API。
4. Web 工作台、Storybook、E2E、部署矩阵和视觉证据。

## 参考（References）

- `../cluster-history-repositories/SPEC.md`
- `../cluster-history-repositories/IMPLEMENTATION.md`
- `./contracts/api.md`
- `./CONTEXT.md`

## Visual Evidence

- [Service monitoring overview](./assets/service-monitor-overview.png)
- [Cluster test workspace](./assets/service-monitor-create-cluster-test.png)
- [Editor workspace on wide screens](./assets/service-monitor-editor-wide.png)
- [Editor result table on mobile](./assets/service-monitor-editor-mobile-results.png)
