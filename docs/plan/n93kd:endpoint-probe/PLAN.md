# Endpoint 探测：可用性/延迟（24h）（#n93kd）

## 状态

- Status: 已完成
- Created: 2026-02-07
- Last: 2026-02-08

## 背景 / 问题陈述

- 需要一个集群级探测机制：通过 **HTTPS 请求公共固定内容页面**（如 gstatic / Cloudflare）来测试 **所有 endpoints**，记录真实延迟，并在 Admin UI 展示最近 **24 小时**（每小时一格）的可用性。
- 关键约束：
  - 探测必须在 **所有节点同时进行**（允许自我测试），且必须使用 **相同的探测配置**（targets/timeouts）。
  - 不得做本机回环：禁止对 `access_host` 为 `localhost` / `127.0.0.1` / `::1` 的 endpoint 发起探测（记录失败即可）。
  - `probe` 用户需要自动获得全部 endpoints 的使用权限。

## 目标 / 非目标

### Goals

- Admin UI：
  - Endpoint list 直接展示：
    - 最新一次探测延迟（ms，canonical target）；
    - 24 格“每小时可用性条”（类似 `||||||||`），可点击进入统计页。
  - Endpoint list 提供 **Test all now** 按钮，触发一次 **cluster-wide** 的探测（所有 endpoints）。
  - Endpoint details 提供 **Test now** 按钮，触发一次 **cluster-wide** 的探测（所有 endpoints）。
  - 手动触发后跳转到一次 run 的详情页，展示 per-node 进度（运行中自动刷新）。
  - run 详情页同时展示 per-endpoint 延迟（p50, ms），并在运行中随数据写入自动刷新。
  - 统计页展示 last-24h summaries，并可查看 per-node 样本。
- Backend：
  - 每小时自动探测，保留 24 小时数据。
  - 支持手动触发，尽量让各节点在同一时刻开始。
  - 结果通过 Raft 持久化，由 leader 统一对 UI 提供查询。
- 探测流量使用独立的 `probe` 用户（自动 grants 到所有 endpoints）。

### Non-goals

- 不做 24 小时之外的长期留存（未来扩展）。
- 不引入重型图表库（保持 UI 轻量）。
- 不做“绕过 endpoint proxy 的直连探测”（探测必须走 endpoint 代理路径）。

## 范围（Scope）

### In scope

- 持久化 probe history（per endpoint / per hour / per node；保留 last 24h）。
- 增加系统用户 `probe`，并确保其对所有 endpoints 都有 grants。
- 实现探测 runner：
  - 使用 Xray client config 创建本地 SOCKS；
  - 通过 SOCKS 对固定内容 target 发起 HTTPS 请求并计时。
- 增加 Admin APIs：
  - trigger probe run（cluster-wide）；
  - query probe summary + history。
- Web UI：endpoint list + detail + stats 页面改造。

### Out of scope

- 在 UI 中暴露 probe 用户的任何凭据。
- SLA / 告警 / 通知（后续计划）。

## 需求（Requirements）

### MUST

- 所有节点并发运行探测；允许 self-test。
- 所有节点必须使用 **相同探测配置**；手动触发需校验 `config_hash` 一致性。
- 不得做本机回环探测：
  - `access_host` 为 `localhost` / `127.0.0.1` / `::1` 时直接拒绝（记录错误，不发起请求）。
  - self-test 仍走 `access_host`，不得 special-case 成 loopback。
- 探测 target 使用 HTTPS 固定响应页面：
  - Required: `https://www.gstatic.com/generate_204`（期望 `204`，作为 canonical latency）。
  - Optional: `https://www.cloudflare.com/robots.txt`（期望 `200` + prefix check）。
- Endpoint list 展示：
  - 24 格每小时可用性；
  - 最新 canonical latency（ms）。
- 点击可用性条进入统计页，并可查看 per-hour + per-node 结果。
- `probe` 用户自动获得全部 endpoints 的使用权限。

### SHOULD

- 将 partial outage（部分节点成功）与 total down 区分展示。
- 限制每个节点的探测并发，避免一次启动过多 Xray 进程。
- 每节点同一时刻只允许运行 1 次 probe run（mutex/lock）。

## 验收标准（Acceptance Criteria）

- Given 集群有 N 个 nodes / M 个 endpoints，
  When 触发一次 probe run（手动或定时），
  Then
  - 每个 node 都会尝试探测每个 endpoint（同配置）；
  - leader 端可查询到 last-24h 的 merged samples（per endpoint/hour/node）。
- Endpoint list 的 24 格状态含义：
  - `unknown`：缺失数据
  - `up`：所有节点成功
  - `degraded`：部分节点成功
  - `down`：全部节点失败
- Endpoint details 有 **Test now**，点击后触发 cluster-wide probing。
- Endpoint list 有 **Test all now**，点击后触发 cluster-wide probing。
- 手动触发后有 run 详情页可查看进度，直到完成。
- run 详情页展示每个 endpoint 的 p50 latency（ms）与该小时 bucket 状态，运行中自动刷新。
- Stats page 能加载并展示某 endpoint 的 last-24h summaries，并可查看某小时的 per-node samples。
- 对 loopback host 的 endpoint 不发起探测请求，UI 中展示对应错误。

## 测试与验证（Testing）

- Rust：
  - `cargo test`
  - 覆盖：
    - hourly bucket pruning（保留 last 24）
    - Raft command merge/append 行为（跨 node 不互相覆盖）
    - loopback host rejection
- Web：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - zod schema 覆盖 probe summary/history parsing

## 风险与开放问题

- 依赖节点上存在 `xray` binary 才能执行探测。
  - 缓解：缺失时记录清晰错误；后续可抽象 runner 以支持替代方案。
- mixed-version cluster 可能导致 probe config 不一致；手动触发需 fail-fast（hash mismatch）。

## 里程碑（Milestones）

- [x] M1: Backend persisted state + Raft command + APIs（summary/history）
- [x] M2: probe 用户 bootstrap + per-node runner + hourly scheduler + internal trigger
- [x] M3: Web UI：24h bar + latest latency + stats page + detail page Test now

## 交付记录（Delivery）

- PR: #72
- CI: `ci` / `xray-e2e` / `pr-label-gate` green

## 变更记录 / Change log

- 2026-02-07: 已实现并验证（backend + web + tests），PR #72。
- 2026-02-07: Endpoint list 增加 Test all now 按钮，便于手动全量触发探测。
- 2026-02-07: 手动触发后跳转到 run 详情页，展示 per-node 探测进度。
- 2026-02-08: run 详情页增加 per-endpoint 延迟列表（p50, ms），运行中自动刷新。
- 2026-02-08: run 详情页调整为 endpoints 优先展示；node runner 进度移至折叠区。
