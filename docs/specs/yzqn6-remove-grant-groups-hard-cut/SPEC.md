# 移除 Grant groups，切换为 user/node/endpoint 接入模型（硬切）（#yzqn6）

## 状态

- Status: 已完成
- Created: 2026-02-27
- Last: 2026-02-27

## 背景 / 问题陈述

- 现有系统仍暴露并依赖 `grant-groups` 作为 admin 侧接入配置核心。
- 这导致 UI 与实际订阅输出存在心智偏差（例如矩阵看似全空但仍有历史 grants 生效）。
- 目标是将接入口径统一到 `user/node/endpoint`（membership + grants），并硬切移除 group 语义。

## 目标 / 非目标

### Goals

- 下线后端 `/api/admin/grant-groups*` 并返回 404。
- 前端移除 Grant groups 导航、路由、页面、调用与 Storybook 依赖。
- 新增用户维度 grants 接口：`GET/PUT /api/admin/users/:user_id/grants`（hard cut）。
- 数据迁移：仅保留有效 grants（enabled=true）并清理历史无效/冲突/孤儿数据。
- `group_name` 从领域模型与持久化业务语义中彻底移除。

### Non-goals

- 不修改 quota 分配算法（P1/P2/P3/overflow）。
- 不修改非 admin 订阅协议格式（raw/base64/clash）。

## 范围（Scope）

### In scope

- 后端 domain/state/http/raft 全链路去 group 化。
- 前端管理端去除 group 页面与 API，改为 user-grants 模型。
- 迁移 `schema_version v5 -> v6`。
- Rust + Web + Storybook 测试回归。

### Out of scope

- 业务策略调整（quota/reset 算法变更）。
- 非 admin 接口格式与消费协议变更。

## 需求（Requirements）

### MUST

- UI 不存在 Grant groups 入口与页面。
- 代码中无 `/grant-groups` admin 路由。
- `PUT /api/admin/users/:user_id/grants` 支持 `items=[]`，并使订阅输出为 0。
- 迁移后旧数据可读且不影响运行。

### SHOULD

- 保持已有 grant_id/credentials 的稳定复用，避免无意义凭据漂移。
- 提供明确迁移失败提示（尤其 WAL 存在未应用旧命令时）。

### COULD

- 增加仅调试用途的迁移统计日志（清理/去重数量）。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- `GET /api/admin/users/:user_id/grants`
  - 返回该用户当前有效 grants（enabled=true），不暴露 group 概念。
- `PUT /api/admin/users/:user_id/grants`
  - hard cut 替换：请求集合即最终有效集合；空列表表示清空全部有效接入。
- 用户详情页“Quota limits”矩阵
  - 读取 user-grants 还原勾选；提交直接写 user-grants；不再经由 managed group。

### Edge cases / errors

- user 不存在：返回 404。
- endpoint 不存在：返回 404。
- 请求重复 endpoint：返回 409/invalid_request（实现按既有冲突策略）。
- 旧 WAL 若存在未应用的 `grant-group` 命令：启动失败并给出迁移指引。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）          | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                              |
| --------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ------------------------------------------ |
| AdminGrantGroups APIs | HTTP API     | internal      | Delete         | ./contracts/http-apis.md | backend         | web/admin           | `/api/admin/grant-groups*` 下线            |
| AdminUserGrants APIs  | HTTP API     | internal      | New            | ./contracts/http-apis.md | backend         | web/admin           | `GET/PUT /api/admin/users/:user_id/grants` |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given 管理 UI
  When 打开导航与路由
  Then 不存在 Grant groups 入口与页面。

- Given 已升级服务
  When 调用 `/api/admin/grant-groups*`
  Then 返回 404 not_found。

- Given 用户选择 0 个 endpoint
  When `PUT /api/admin/users/:user_id/grants` with `items=[]`
  Then 订阅输出为 0（raw/clash 都无条目）。

- Given 历史数据包含 group_name/disabled/冲突/孤儿 grants
  When 启动并完成迁移
  Then 系统可读可运行且不再依赖 group 语义。

## 实现前置条件（Definition of Ready / Preconditions）

- 流程类型锁定为 `fast-track`。
- 迁移口径与退役策略已确认：移除 `group_name`、仅保留 enabled、旧接口返回 404。
- 新旧 API 契约已冻结于 `contracts/http-apis.md`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Backend：`cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test`
- Web：`bun run lint` + `bun run typecheck` + `bun run test` + `bun run test-storybook`

### UI / Storybook (if applicable)

- 清理 Grant groups 页面 stories。
- UserDetails stories 改为 user-grants 数据模型。

### Quality checks

- 代码搜索守卫：无 `/api/admin/grant-groups|/grant-groups|adminGrantGroups|GrantGroup` 运行时代码依赖。

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/yzqn6-remove-grant-groups-hard-cut/contracts/http-apis.md`
- （可选）历史 `docs/plan/8rrmk:user-node-quotas-no-groups/PLAN.md` 增加“被 yzqn6 收敛”的说明

## 计划资产（Plan assets）

- None

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 后端 domain/state/raft 去 group 化 + v6 迁移
- [x] M2: 后端 admin API 切换到 user-grants，grant-groups 下线
- [x] M3: 前端路由/页面/API/Storybook 去 group 化并接入 user-grants
- [x] M4: 全量验证通过并完成快车道收敛（PR + checks + review-loop）

## 方案概述（Approach, high-level）

- 用用户维度 hard cut 接口替代 group 级接口，避免隐藏状态与跨 group 副作用。
- 迁移阶段清理无效数据并用确定性规则处理冲突，保证升级幂等与可回放。
- WAL 兼容以“已应用命令可 Blank 化、未应用命令 fail-fast”确保安全升级。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：存在未应用旧 grant-group WAL 命令时需人工先完成旧版本 apply/snapshot。
- 开放问题：无。
- 假设：外部调用方已切换，不再依赖 grant-groups admin API。

## 变更记录（Change log）

- 2026-02-27: created and frozen from implementation plan (fast-track)
- 2026-02-27: completed implementation and validation; opened PR #84

## 参考（References）

- `docs/plan/8rrmk:user-node-quotas-no-groups/PLAN.md`
- `src/state.rs`
- `src/http/mod.rs`
- `web/src/views/UserDetailsPage.tsx`
