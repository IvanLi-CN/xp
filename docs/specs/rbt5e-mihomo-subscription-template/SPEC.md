# 用户订阅 Mihomo 定制化配置（#rbt5e）

## 状态

- Status: 已完成
- Created: 2026-03-04
- Last: 2026-03-06

## 背景 / 问题陈述

- 当前订阅接口仅支持 `raw/base64/clash`，无法输出用户自定义的完整 Mihomo 配置。
- 真实使用中需要在“用户自定义配置 + 系统动态注入”之间取得平衡：
  - `proxies`（主力节点）来自 XP 用户订阅数据（membership/endpoint/node）动态生成；
  - `proxy-providers`（普通节点池）由用户配置，用于提供“地区节点选择”与链式代理中转能力；
  - 用户仅应维护与动态节点无关的静态配置（DNS/TUN/rules/业务分组等）。
- 早期以“模板 + extra_*”方式允许用户在配置中写入动态引用，容易出现：
  - 混入配置里写死引用了不存在的 proxy/provider（例如清空 extra_* 后仍引用旧名称），导致订阅不可用；
  - 用户需要把 providers 列表手工同步到多个 `use` 字段，维护成本高。

## 目标 / 非目标

### Goals

- 新增 `format=mihomo` 输出，支持“每用户模板 + 动态注入 + 用户扩展”。
- 保持 `raw/base64/clash` 现有行为不变。
- 按约定生成 SS 链式节点（`dialer-proxy` -> `🛣️ Japan|HongKong|Korea`）以及 direct reality / direct ss 节点。
- 将所有 `proxy-providers` 自动注入固定中转组的 `use` 列表。
- 将“用户模板”升级为“混入配置（mixin config）”：用户只补充非动态段，动态段由系统内置逻辑生成并覆盖。
- 内置稳定入口组（含地区组），使普通节点池可通过地区入口直接供用户选择，无需在 mixin 中写死 provider 名称列表。
- 内置落地组生成策略：落地组优先直连（reality）；当 `base-reality` 存在时，不使用 SS（不加入 `base-ss` 及其链式）。

### Non-goals

- 不内置敏感模板内容到仓库。
- 不保证 YAML 注释/anchors 原样保留。
- 不新增模板编辑 DSL，仅接受 YAML 文本输入。

## 范围（Scope）

### In scope

- 后端新增用户级 Mihomo 模板配置存储与管理 API。
- 订阅接口新增 `format=mihomo`，模板缺失时回退 clash。
- Web 用户详情页新增模板与扩展输入保存能力，并可预览 `mihomo`。
- 单测/集成测试/e2e mock 补齐。

### Out of scope

- 不重做现有订阅命名规则（raw/clash）。
- 不新增 provider 自动抓取逻辑（provider 数据仅来自用户输入）。

## 需求（Requirements）

### MUST

- `GET /api/sub/{token}?format=mihomo` 支持完整输出。
- 用户模板按 `user_id` 持久化存储。
- 用户可输入 `extra_proxies_yaml`（sequence）和 `extra_proxy_providers_yaml`（mapping）。
- 渲染时忽略模板中的 `proxies` 与 `proxy-providers`，由系统重建。
- 节点名冲突自动重命名并记录告警日志。
- 模板缺失时 `format=mihomo` 回退 clash。
- 系统内置并覆盖“动态相关的 proxy-groups”（稳定入口组 + 落地组），mixin config 不要求包含这些组定义。
- `proxy-providers` 视为一个整体普通节点池；地区入口组使用“所有 provider 名称”的全集作为候选来源。

### SHOULD

- 模板/扩展 YAML 在写入前做根类型校验并返回可读错误。
- relay 组缺失时自动补齐最小可用定义。

### COULD

- 后续扩展更多链式区域（如 SG/US）与可配置 suffix。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员在用户详情页编辑并保存 Mihomo profile（字段名不变，但语义为 mixin config + pools）。
- 用户订阅拉取 `format=mihomo` 时：
  - 读取用户 mixin config（存储字段：`template_yaml`）。
  - 读取普通节点池 providers（存储字段：`extra_proxy_providers_yaml`）。
  - 生成主力节点（XP 动态注入：reality/ss direct + ss chain）。
  - 覆盖并注入稳定入口组（含地区组、`🛣️` 中转组、`🔒 落地` 等）。
  - 输出 YAML。

### Edge cases / errors

- 模板 YAML 非法或根类型错误：管理 API 返回 `400 invalid_request`。
- extra YAML 根类型不符合约束：管理 API 返回 `400 invalid_request`。
- 未配置模板：订阅接口回退 clash 输出。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                                 | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）       | 备注（Notes）      |
| ------------------------------------------------------------ | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------------- | ------------------ |
| `GET /api/sub/{subscription_token}?format=mihomo`            | HTTP API     | external      | New            | ./contracts/http-apis.md | backend         | mihomo client/web preview | 无模板时回退 clash |
| `GET /api/admin/users/{user_id}/subscription-mihomo-profile` | HTTP API     | internal      | New            | ./contracts/http-apis.md | backend         | web admin                 | 返回用户模板配置   |
| `PUT /api/admin/users/{user_id}/subscription-mihomo-profile` | HTTP API     | internal      | New            | ./contracts/http-apis.md | backend         | web admin                 | 写入并校验 YAML    |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given 用户已配置模板，When 拉取 `format=mihomo`，Then 返回 YAML 包含系统生成的 `-reality`、`-ss`、`-JP/HK/KR` 节点。
- Given 用户配置了多个 `proxy-providers`，When 拉取 `format=mihomo`，Then relay 组 `🛣️ Japan|HongKong|Korea` 的 `use` 包含这些 provider。
- Given 用户配置了多个 `proxy-providers`，When 拉取 `format=mihomo`，Then 地区入口组（如 `🌟 Japan/🔒 Japan/🤯 Japan`）存在且候选来自所有 provider。
- Given 存在 `base-reality` 与 `base-ss` 同时可用，When 生成落地组 `🛬 {base}`，Then `🛬 {base}` 只包含 `base-reality`（不包含 `base-ss` 及其链式）。
- Given 仅存在 `base-ss`（无 `base-reality`），When 生成落地组 `🛬 {base}`，Then `🛬 {base}` 优先包含链式 `base-JP/HK/KR` 并以 `base-ss` 作为兜底。
- Given 系统节点名与用户额外节点名冲突，When 渲染，Then 冲突项自动稳定重命名且日志有告警。
- Given 用户未配置模板，When 拉取 `format=mihomo`，Then 返回合法 clash YAML（回退行为）。
- Given 现有 `raw/clash/base64` 调用，When 回归测试，Then 输出语义与 content-type 不回归。

## 实现前置条件（Definition of Ready / Preconditions）

- API 形态与回退策略已冻结。
- 链式节点命名与 relay 组策略已冻结。
- 用户确认模板不内置仓库、按用户持久化。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `src/subscription.rs` 覆盖渲染、冲突重命名、模板根类型错误。
- Integration tests: `src/http/tests.rs` 覆盖 admin profile API 与 `format=mihomo`（含回退）。
- E2E tests (if applicable): 更新 web mock 支持 `mihomo` 格式与 profile API。

### UI / Storybook (if applicable)

- UserDetails 页面新增模板编辑区，保留原有订阅预览流程。

### Quality checks

- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`

## 文档更新（Docs to Update）

- `docs/desgin/subscription.md`: 补充 `format=mihomo` 行为与模板驱动规则。

## 计划资产（Plan assets）

- Directory: `docs/specs/rbt5e-mihomo-subscription-template/assets/`

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 状态层新增 `UserMihomoProfile` 持久化与 Raft 命令
- [x] M2: HTTP 管理 API + 订阅 `format=mihomo` 路由落地
- [x] M3: Mihomo 渲染引擎（系统节点生成 + 合并 + 冲突重命名 + relay use 注入）
- [x] M4: Web UserDetails 模板编辑 + `mihomo` 预览
- [x] M5: 测试补齐与质量门禁通过
- [x] M6: 管理 API 保存时自动抽取 `template_yaml` 顶层 `proxies/proxy-providers` 到 extra_*（减少配置误用）
- [x] M7: 订阅渲染内置稳定入口组（含地区组），mixin config 不要求包含动态组定义
- [x] M8: 订阅渲染内置落地组生成策略（reality 优先；存在 reality 时不使用 ss），并完成共享测试机真实 mihomo 校验

## 方案概述（Approach, high-level）

- 以“混入配置（mixin config）”为静态补充，后端在渲染阶段覆盖动态段并注入系统代理与系统稳定入口组。
- 将“高风险自由输入”限制为 YAML 根类型校验与保存时自动抽取动态段，保证最小正确性与可维护性。
- 冲突处理采用自动重命名，避免因用户输入导致订阅不可用。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：用户输入模板质量不稳定，仍可能出现运行时客户端不兼容字段。
- 需要决策的问题：告警是否需要额外暴露到 API 响应（当前仅日志）。
- 假设（需主人确认）：固定 relay 组名称长期稳定。

## 变更记录（Change log）

- 2026-03-04: 创建规格并冻结 API/渲染/回退口径。
- 2026-03-04: 完成实现与验证（`cargo test`、`web lint/typecheck/test`），PR #95（label: `type:minor`，xray-e2e + label-gate 通过）。
- 2026-03-06: 需求调整：引入“混入配置 + 系统内置动态组逻辑”（地区入口组 + 落地组策略），并将保存时自动抽取 `proxies/proxy-providers` 作为防误用机制。
- 2026-03-06: 完成稳定入口组/落地组内置注入，并在共享测试机用真实 Mihomo `-t` 校验订阅可加载。

## 参考（References）

- 样例配置：`/Users/ivan/Downloads/mihomo.yaml`
