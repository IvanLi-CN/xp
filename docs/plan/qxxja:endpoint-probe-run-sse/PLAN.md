# Endpoint probe run：SSE 推送进度与结果（UI 改造）（#qxxja）

## 状态

- Status: 待实现
- Created: 2026-02-08
- Last: 2026-02-08

## 背景 / 问题陈述

- 现有 endpoint probe run 详情页在 run 运行中以轮询刷新为主（见计划 #n93kd 的交付记录），但需求要求必须改为 **SSE 推送**（前端不得以 polling 为主）。
- run 详情必须以 **endpoint** 为核心（展示每个 endpoint 的在线/延迟与实时进度），node 维度只作为辅助信息展示。
- run 页 UI 需要更紧凑：默认只保留 `Endpoint` + `Result(状态+延迟)` 两列，时间信息放进 `Result` 的 tooltip（hover 才显示）。

## 目标 / 非目标

### Goals

- Backend：
  - 提供 cluster-wide 的 SSE stream，推送：
    - per-node runner progress（`endpoints_done/total` + status）；
    - per-endpoint probe sample（包含 latency 与 checked_at）；
    - run overall status（running/finished/failed）。
  - SSE 保持 admin auth（沿用 Bearer token；不引入 cookie 登录）。
- Web：
  - `EndpointProbeRunPage` 不再使用 `refetchInterval` 轮询来更新进度与结果；
  - 手动触发（Test all now/Test now）跳转到 run 页后，进度与结果 **实时更新**；
  - Run 页默认表格仅两列：`Endpoint` / `Result`；`Result` hover 显示 checked_at。
- Dev/测试环境：
  - `scripts/dev/subscription-3node-compose` 的 `seed` 生成足够的 endpoints 覆盖：
    - 至少 1 个节点拥有 ≥2 个 endpoints（用于验证列表/进度）；
    - 至少覆盖两种 endpoint kind（`ss2022` + `vless reality`），用于验证“每种类型都有接入点”。

### Non-goals

- 不修改 last-24h history 的持久化结构与汇总口径（沿用 #n93kd）。
- 不引入 WebSocket（SSE 足够）。

## 需求（Requirements）

### MUST

- 前端不得通过 polling（`refetchInterval` / 定时 `refetch`）来更新 probe run 的进度与结果。
- run 页显示 **endpoint** 结果（状态+延迟），且 `checked_at` 仅通过 tooltip 展示。
- SSE 支持 Bearer token 鉴权（不在 URL query 里传 token）。
- run 完成/失败后 SSE 正确收尾（停止发送 running updates）。

### SHOULD

- SSE keepalive/ping，避免中间代理断开连接。
- 任意单个 node 的断连/失败应可在 UI 反映（node 状态），但不影响其他 node 的事件推送与整体展示。

## 验收标准（Acceptance Criteria）

- Given 手动触发一次 probe run，
  When 打开 run 详情页，
  Then
  - 页面无需手动刷新，进度与 endpoint latency 会随着 SSE 事件实时更新；
  - 浏览器 network 中不存在固定周期的 status/endpoints polling 请求（除首次加载/静态数据加载外）。
- Run 页 UI：
  - 默认仅 `Endpoint` + `Result` 两列；
  - `Result` 的 tooltip 显示 `checked_at`（如可用）。
- Dev compose 环境 `seed` 后：
  - endpoints 数量覆盖两种 kind；
  - 至少一个节点拥有 ≥2 endpoints。

## 测试与验证（Testing）

- Rust：`cargo test`
- Web：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
- Manual：
  - 在 docker compose 或测试服务器集群中触发 `Test all now`，确认 run 页 SSE 实时更新与延迟展示符合预期。

## 里程碑（Milestones）

- [ ] M1: Backend SSE（internal events + cluster fan-out）
- [ ] M2: Web run 页 SSE client + UI 精简
- [ ] M3: Compose seed 补齐 endpoints kinds
