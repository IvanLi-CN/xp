# Grant groups 硬切下线与 Access 模型迁移（#nujzm）

## 状态

- Status: 待实现
- Created: 2026-02-27
- Last: 2026-02-27

## 背景 / 问题陈述

当前系统仍暴露并依赖 Grant groups（前端导航、后端 `/api/admin/grant-groups*`、领域命令分支）。用户接入关系已经具备 `user/node/endpoint` 语义（membership + grants），但实现仍夹带 group 概念，导致维护成本、认知成本与迁移成本升高。

## 目标 / 非目标

### Goals

- 前端彻底移除 Grant groups 导航、路由、页面与相关调用。
- 后端删除 `/api/admin/grant-groups*` 接口并统一返回 `404 not_found`。
- 引入并冻结用户 Access Admin API：`GET/PUT /api/admin/users/:user_id/access`。
- `group_name` 从核心数据模型移除；迁移后不参与任何业务决策。
- 一次性迁移历史数据：仅保留有效 access（`enabled=true`），重建 membership，并清理被删除 grants 的 usage。
- 对历史 Raft 日志中的 grant-group 命令提供本版本最小 WAL 回放兼容 shim（仅内部回放，不对外暴露）。

### Non-goals

- 不调整 quota 分配算法（P1/P2/P3/overflow）。
- 不变更非 admin 订阅协议格式。
- 不在本版本删除 WAL 兼容 shim（该动作放到下一版本 clean-up）。

## 范围

### In scope

- `src/http/mod.rs`：删除 grant-groups 路由与 handler；新增 users access API。
- `src/state.rs` / `src/domain/mod.rs`：移除 group 字段与 group 相关主流程分支；实现用户级 hard-cut 写入命令。
- `src/raft/storage/file.rs`（及必要映射）: legacy grant-group WAL 回放 shim。
- `web/src/**`：删除 Grant groups UI 面，`UserDetailsPage` 改为 access API。
- 关联测试与 Storybook 清理。

### Out of scope

- 非 admin 输出格式与客户端订阅协议升级。
- quota engine 计算逻辑重构。

## 接口契约

- `./contracts/http-apis.md`

## 数据与迁移策略

### 阶段 A（本版本）

- `SCHEMA_VERSION` 升级到 `9`。
- 迁移 `v8 -> v9`：
  - 删除 grants 的 `group_name`；
  - 仅保留 `enabled=true` grants；
  - 基于 grants 重建 `node_user_endpoint_memberships`；
  - 清理被删除 grants 对应 usage。
- 保留最小 WAL 兼容 shim：可回放 legacy grant-group 命令，不可通过 HTTP 再写入该语义。

### 阶段 B（下一版本）

- 条件：确认日志压实后无 legacy grant-group 命令。
- 动作：删除 WAL shim 与所有 legacy grant-group 兼容代码。

## 验收标准

- UI 中不存在 Grant groups 入口与页面。
- 代码中无 `/grant-groups` admin 路由。
- `GET/PUT /api/admin/users/:user_id/access` 可读写用户接入关系，且 hard-cut 生效。
- 用户接入配置与订阅输出保持一致（0 选中 = 0 输出；N 选中 = N 输出）。
- 全量测试通过（Rust + Web）。
- 迁移后旧数据可读且不影响运行（含旧 WAL 回放场景）。

## 质量门槛

- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run test:e2e`
- `cd web && bun run test-storybook`

## 实现里程碑

- [ ] M1: 冻结 docs/specs + contracts（Access API、迁移与兼容边界）
- [ ] M2: 后端完成 schema v9、access API、grant-group API 下线、WAL shim
- [ ] M3: 前端移除 Grant groups 面并接入 access API
- [ ] M4: Rust/Web/Storybook/E2E 全量回归通过
- [ ] M5: 快车道收口（PR + checks + review-loop）

## 风险 / 开放问题 / 假设

- 风险：历史数据可能存在同一 `(user_id, endpoint_id)` 的多条 grants；迁移需确定性去重。
- 假设：migration 中删除 `enabled=false` grants 不影响业务需求（已确认）。
- 假设：本版本允许“最小 WAL shim”临时保留（已确认）。

## 变更记录

- 2026-02-27: 创建规格并冻结交付边界（fast-track）。
