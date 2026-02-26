# 节点服务可观测升级（#9vmap）

## 状态

- Status: 已完成
- Created: 2026-02-26
- Last: 2026-02-26

## 背景 / 问题陈述

- 当前 Web 仅能看到节点静态元数据，缺少 `xp/xray/cloudflared` 运行态与近期趋势。
- 已有 `xray` 探活与自动重启机制，但关键事件仅在日志中，无法在管理界面集中查看。
- 运维需要在节点列表快速识别异常，并在详情页查看组件状态与关键事件时间线。

## 目标 / 非目标

### Goals

- 在节点列表展示每个节点的服务状态摘要与 7 天状态指示器（30 分钟粒度）。
- 在节点详情展示 `xp/xray/cloudflared` 组件状态、最近关键事件，并支持 SSE 实时更新。
- 在后端新增节点运行态聚合 API（含跨节点拉取、`partial/unreachable_nodes` 语义）。
- 为 cloudflared 增加监控与可选自动重启能力，并接入事件记录。
- 将运行态历史与事件持久化到本地 `${XP_DATA_DIR}/service_runtime.json`（不进 Raft）。

### Non-goals

- 不引入 Prometheus/Loki 等外部监控系统。
- 不把运行态历史写入 Raft。
- 不改动 Endpoint/User/Grant 业务模型。

## 范围（Scope）

### In scope

- Backend: 运行态采集、持久化、聚合 API、SSE 转发、internal local API。
- Web: `NodesPage` 摘要列与趋势条、`NodeDetailsPage` 组件卡片 + 事件流。
- Ops: `xp-ops` 写入 cloudflared 重启最小权限（polkit/doas）与 env 模板。
- Docs: API/ops/workflows/env 文档同步。

### Out of scope

- UI 移动端专门改版。
- 外部告警渠道（邮件/Webhook/IM）联动。

## 需求（Requirements）

### MUST

- 新增 `GET /api/admin/nodes/runtime` 返回节点运行态摘要列表。
- 新增 `GET /api/admin/nodes/{node_id}/runtime` 返回节点详情运行态（含最近事件）。
- 新增 `GET /api/admin/nodes/{node_id}/runtime/events` SSE，事件至少包含 `hello/snapshot/event/node_error/lagged`。
- 新增 internal API：
  - `GET /api/admin/_internal/nodes/runtime/local`
  - `GET /api/admin/_internal/nodes/runtime/local/events`
- 组件状态必须支持枚举：`disabled/up/down/unknown`；节点摘要支持：`up/degraded/down/unknown`。
- 历史窗口固定 `7d/30min`（336 slots），事件保留 7 天，重启后可恢复。
- cloudflared 未启用时必须显示 `disabled`，且不触发重启请求。
- cloudflared 启用后，连续失败达到阈值时触发重启并记录事件。

### SHOULD

- 事件应包含状态变更与重启请求结果（成功/失败）。
- 远端节点不可达时列表应返回 `partial=true` 且填充 `unreachable_nodes`。

### COULD

- None

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员进入节点列表：前端并行拉取静态节点信息 + 运行态摘要，渲染摘要与趋势条。
- 管理员进入节点详情：拉取运行态详情并建立 SSE 流，实时更新组件状态与事件列表。
- leader 节点调用 runtime 聚合接口时，使用 internal signature 并发请求其他节点 local runtime。

### Edge cases / errors

- 远端节点超时/证书错误：汇总结果降级为 `partial`，本地与可达节点仍返回。
- SSE 远端断流：发送 `node_error`；前端展示连接错误并保留最后快照。
- cloudflared 配置为 `none`：组件状态固定 `disabled`。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Node runtime admin APIs | HTTP API | internal | New | ./contracts/http-apis.md | backend | web | 列表/详情/SSE |
| Node runtime internal APIs | HTTP API | internal | New | ./contracts/http-apis.md | backend | backend | local 汇总转发 |
| cloudflared runtime config | CLI | internal | Modify | ./contracts/cli.md | ops | xp/xp-ops | 新增 `XP_CLOUDFLARED_*` |
| service_runtime.json | File format | internal | New | ./contracts/file-formats.md | backend | backend | 本地持久化 |
| runtime SSE events | Events | internal | New | ./contracts/events.md | backend | web | hello/snapshot/event/node_error/lagged |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/events.md](./contracts/events.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/file-formats.md](./contracts/file-formats.md)

## 验收标准（Acceptance Criteria）

- Given 集群包含本地+远端节点，When 打开节点列表，Then 每个节点都显示 `summary` 与 7 天状态指示器。
- Given 打开节点详情，When 组件状态变化，Then 页面通过 SSE 自动更新组件卡片和事件列表。
- Given cloudflared 未启用，When 打开详情，Then cloudflared 状态为 `disabled` 且无重启事件。
- Given cloudflared 已启用并故障，When 连续失败达到阈值，Then 记录重启请求与结果事件。
- Given `xp` 进程重启，When 重新打开详情，Then 可看到重启前 7 天窗口内历史槽位与事件。
- Given 任一远端节点不可达，When 请求 `/api/admin/nodes/runtime`，Then 返回 `partial=true` 且 `unreachable_nodes` 包含该节点。

## 实现前置条件（Definition of Ready / Preconditions）

- API 字段与状态枚举已冻结。
- 7 天历史窗口与事件保留策略已冻结。
- cloudflared “未启用=disabled” 语义已确认。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: runtime 状态机、持久化裁剪、事件生成。
- Integration tests: runtime 列表聚合、remote unreachable、SSE 代理。
- E2E tests: 节点列表与详情页基础交互回归。

### UI / Storybook (if applicable)

- Stories to add/update: `NodesPage`、`NodeDetailsPage` 相关故事与 mock。

### Quality checks

- Backend: `cargo fmt` / `cargo clippy -- -D warnings` / `cargo test`
- Web: `bun run lint` / `bun run typecheck` / `bun run test`

## 文档更新（Docs to Update）

- `docs/desgin/api.md`: runtime API 契约。
- `docs/desgin/workflows.md`: 节点服务状态观测流程。
- `docs/ops/README.md`: cloudflared 监控/自动重启配置说明。
- `docs/ops/env/xp.env.example`: `XP_CLOUDFLARED_*` 参数说明。

## 计划资产（Plan assets）

- Directory: `docs/specs/9vmap-node-service-observability/assets/`
- In-plan references: None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: Backend runtime model + persistence + cloudflared supervisor
- [x] M2: Runtime admin/internal APIs + SSE forwarding + tests
- [x] M3: Web 页面改造 + Storybook/Vitest + 文档同步

## 方案概述（Approach, high-level）

- 在 `xp` 运行时新增节点运行态聚合模块，融合 `xray` 与 `cloudflared` 健康快照。
- 运行态以本地文件持久化，按固定时间窗裁剪，避免写入 Raft。
- 通过 internal signature 跨节点拉取 local runtime，实现 leader 视角聚合。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：跨发行版服务状态命令差异导致误判，需在测试机回归。
- 需要决策的问题：None
- 假设：`XP_CLOUDFLARED_RESTART_MODE=none` 代表 cloudflared 未启用。

## 变更记录（Change log）

- 2026-02-26: 创建规格并冻结首版接口、状态枚举、窗口策略。
- 2026-02-26: 完成后端运行态聚合/持久化、前端 Nodes/NodeDetails 改造、cloudflared 运维配置与文档同步。

## 参考（References）

- `docs/plan/0021:xray-supervision-auto-restart/PLAN.md`
- `docs/plan/0023:xray-restart-via-init-system/PLAN.md`
- `docs/plan/ma8jj:cloudflared-openrc-supervision/PLAN.md`
