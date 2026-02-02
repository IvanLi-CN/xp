# 订阅输出：name 全局唯一，避免客户端覆盖（#m6mer）

## 状态

- Status: 已完成
- Created: 2026-02-01
- Last: 2026-02-02

## 背景 / 问题陈述

- 反馈：配置两个节点共四个端点，但客户端导入订阅后只看到一个节点/一条。
- 初步假设：多个 Grant 产出的代理 name 相同（常见于 `Grant.note` 被批量设置为同一值），在 Clash/Mihomo 等客户端会按 `name` 去重/覆盖，导致“看起来缺了节点”。
- 需要先用计划 `apxdg` 的三节点环境把“服务端漏输出”与“客户端覆盖”区分开，并把回归自动化。

## 目标 / 非目标

### Goals

- 服务端订阅输出保证每个 enabled Grant 对应的条目在同一订阅内 `name` 唯一（raw `#name` 与 clash `proxies[].name`）。
- 在 name 冲突时采用确定性的、可读的 disambiguation 规则（不引入随机性）。
- 在 `apxdg` 环境中可自动复现并验证修复。

### Non-goals

- 不改变 Grant/Endpoint 模型与授权逻辑（只改输出命名策略）。
- 不为客户端做额外假设（不新增 proxy-groups/rules 等）。
- 不要求运行 `xray`。

## 范围（Scope）

### In scope

- `src/subscription.rs`：命名策略调整（仅当 note 重复时追加后缀）。
- 补充单元/HTTP 测试覆盖“重复 note”场景。
- `docs/desgin/subscription.md`：记录 name 唯一性规则。
- 在 `apxdg` 环境里通过脚本 verify。

### Out of scope

- 前端 UI 强制校验/自动改 note（可后续讨论）。
- 订阅内容排序/格式的大改（除非为稳定性必须）。

## 需求（Requirements）

### MUST

- 对同一 user 的所有 enabled grants：
  - raw：输出行数 = enabled grants 数
  - clash：`proxies` 数 = enabled grants 数
- name 唯一性规则：
  - 若 `Grant.note` 为空/空白：使用默认 `{user.display_name}-{node.node_name}-{endpoint.tag}`（保持现状）
  - 若 `Grant.note` 非空且在本订阅中唯一：使用 `Grant.note`（保持现状）
  - 若 `Grant.note` 非空且在本订阅中出现多次：对这些冲突项输出 `{note}-{node.node_name}-{endpoint.tag}`
- 输出顺序保持确定性（同输入集合下稳定）。

### SHOULD

- 当存在冲突时，后缀应尽量短但能区分不同 endpoint（优先使用 `endpoint.tag` 而不是 `endpoint_id`）。
- 仅在冲突时改名，尽量减少对已有用户的“名字变更”。

### COULD

- 订阅构建失败时返回更可诊断的错误（目前是统一 500），但这属于另一议题，默认不做。

## 接口契约（Interfaces & Contracts）

None（对外 API 路径与 schema 不变；仅输出内容的 `name` 策略明确化）。

## 验收标准（Acceptance Criteria）

- Given `apxdg` 环境的 seed 数据：2 节点 4 endpoints；4 个 grants 均 enabled 且 `Grant.note="same"`
  When 分别从 3 个节点拉取：
  - `GET /api/sub/<token>?format=raw`
  - `GET /api/sub/<token>?format=clash`
  Then
  - raw 行数=4，且 `#name` 集合大小=4
  - clash `proxies` 数=4，且 `name` 集合大小=4
  - 三个节点返回逻辑一致（至少满足 `verify` 脚本断言一致）

- Given note 唯一/为空的用户
  When 拉取订阅
  Then name 与现状一致（不引入额外后缀）

## 实现前置条件（Definition of Ready / Preconditions）

- 计划 `apxdg` 的 `reset-and-verify` 脚本已可用（或至少能稳定 seed + 拉取订阅）。
- 主人确认“重复 note 导致客户端覆盖”是要修复的核心（若你希望改成“允许同名，由客户端自己处理”，则该计划应标记为 `跳过`）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust unit tests：覆盖“重复 note”时的 name 去重规则。
- HTTP tests：覆盖 raw/clash 订阅输出中 `name` 唯一性。

### Quality checks

- `cargo test`
- `cargo fmt`
- `cargo clippy -- -D warnings`

## 文档更新（Docs to Update）

- `docs/desgin/subscription.md`: 补充 name 唯一性规则（重复 note 的处理）。

## 计划资产（Plan assets）

- None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [x] M1: 补充回归测试用例（unit + http）
- [x] M2: 在订阅生成逻辑中实现 name 唯一策略
- [x] M3: 在 `apxdg` 环境中验证（3 节点一致 + raw/clash 断言）

## 方案概述（Approach, high-level）

- 预扫描 enabled grants，统计 note 的出现次数；生成 name 时仅对重复 note 的条目追加 `-{node.node_name}-{endpoint.tag}`，保证唯一且可读。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - 部分用户依赖固定 name（冲突时改名属于“可感知变化”）
- 需要决策的问题：
  - 后缀是否用 `endpoint.tag`（推荐）还是 `endpoint_id`（更稳定但更长）
  - 是否也需要处理“默认名冲突”（理论上默认名包含 node+tag，不应冲突）
- 假设（需主人确认）：
  - 问题主要表现为客户端按 `name` 覆盖/去重，而非服务端漏输出。

## 变更记录（Change log）

- 2026-02-01: create plan
- 2026-02-02: implement name disambiguation + add unit test

## 参考（References）

- `docs/desgin/subscription.md`
- 计划 `apxdg:subscription-3node-compose-env/PLAN.md`
