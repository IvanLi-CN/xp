# 节点 runtime fan-out 超时容忍（#jtnnd）

## 状态

- Status: 已完成
- Created: 2026-03-11
- Last: 2026-03-11

## 背景 / 问题陈述

- `/api/admin/nodes/runtime` 会从当前面板节点串行拉取远端节点的本地 runtime 概览。
- 在 Cloudflare 边缘偶发高延迟时，现有 `3s` 超时会把仍可达的节点过早判成 `unreachable`，导致 Nodes / Dashboard 出现误导性的 partial 结果。
- 既有 `9vmap` 规格已经处于 `已完成`，本次补丁不能通过回写历史规格来表达新实现。

## 目标 / 非目标

### Goals

- 仅提高节点 runtime 列表 fan-out 的请求超时容忍度，降低短时边缘抖动下的误判。
- 保持单节点详情、控制类接口与其他聚合接口继续快速失败，不把整个管理面一起拖慢。
- 用新的补丁规格承接这次实现，避免修改已完成历史 spec。

### Non-goals

- 不修改 runtime 数据模型、UI 占位语义或 `unknown` 历史条的展示方式。
- 不调整 alerts、quota、IP usage、endpoint probe 等其他 cluster admin 请求的超时策略。
- 不改动历史 `9vmap` 规格正文。

## 范围（Scope）

### In scope

- `src/http/mod.rs` 中 `/api/admin/nodes/runtime` fan-out 超时从 `3s` 提升到更宽松的容忍值。
- 相关最小验证，确保 partial / unreachable 行为仍正确。
- 新补丁规格与索引同步。

### Out of scope

- 新增并发 fan-out、重试、熔断或配置化超时。
- 修改节点详情页、用户页或探测控制接口的失败时延。

## 需求（Requirements）

### MUST

- `/api/admin/nodes/runtime` 的远端 fan-out 请求必须比默认 `3s` 更能容忍短时边缘高延迟。
- 仅 runtime 列表 fan-out 使用更长超时；其他 cluster admin 请求仍维持原有快速失败行为。
- 误判缓解后，现有 partial / `unreachable_nodes` 响应语义不得变化。

## 验收标准（Acceptance Criteria）

- Given 某远端节点在 `3s` 内未返回但可在更短暂扩展窗口内返回， When 请求 `/api/admin/nodes/runtime`， Then 该节点不应因瞬时高延迟被过早归入 `unreachable_nodes`。
- Given 节点详情、alerts、quota、IP usage 与 endpoint probe 仍使用既有快速失败口径， When 远端节点不可达， Then 这些路径的错误返回时延不因本补丁明显增加。
- Given 历史 `9vmap` 已完成， When 本补丁落地， Then 变更记录应写入新的补丁 spec，而不是回写旧 spec。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 收敛 runtime fan-out 超时调整范围，仅保留列表聚合路径
- [x] M2: 本地完成相关 Rust 格式化与 targeted tests
- [x] M3: 以新补丁 spec 同步实现范围，避免修改历史 spec

## 风险 / 开放问题 / 假设

- 风险：runtime fan-out 仍是串行请求；若后续节点数继续增长，仅放宽超时不能完全解决慢尾问题。
- 假设：当前主要误判来源是短时边缘网络抖动，而不是节点本地 runtime 数据缺失。

## 变更记录（Change log）

- 2026-03-11: 创建补丁规格，冻结“只放宽 runtime 列表 fan-out 超时，不修改历史 spec”的范围。
- 2026-03-11: 完成 runtime fan-out 超时收敛实现与 targeted tests（M1/M2）。
- 2026-03-11: 完成 spec 索引同步，并以新规格承接本次补丁（M3）。
