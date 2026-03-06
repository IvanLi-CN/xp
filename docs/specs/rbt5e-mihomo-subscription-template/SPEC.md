# 用户订阅 Mihomo 混入配置（#rbt5e）

## 状态

- Status: 已完成
- Created: 2026-03-04
- Last: 2026-03-06

## 背景 / 问题陈述

- 当前订阅接口虽已支持 `format=mihomo`，但早期口径仍偏向“template + extra_*”自由拼装，容易把动态节点、provider 名称或落地组写死在用户输入里。
- 真实使用里，动态层应由系统根据 XP 用户订阅数据生成：
  - `proxies`（主力节点）来自 membership/endpoint/node；
  - `proxy-providers`（普通节点池）若存在，则整体作为地区入口与链式代理的候选池；
  - 用户输入只应补充静态配置与业务分组，不应继续固化动态节点名。
- 若不把动态层系统内置化，会持续出现：
  - 清空 `extra_*` 或切换用户后仍残留不存在的 proxy/provider/group 引用；
  - 管理端把完整配置当模板粘贴后，动态层与静态层职责混淆；
  - 示例配置难以稳定复用，管理员无法确认最终订阅与示例行为是否等价。

## 目标 / 非目标

### Goals

- 新增并稳定 `format=mihomo` 输出，支持“每用户 mixin + 系统动态注入 + 用户扩展”。
- 保持 `raw/base64/clash` 现有行为不变。
- 将管理 API 主字段切换为 `mixin_yaml`，兼容读取旧字段 `template_yaml` 一轮实现周期。
- 系统内置生成并覆盖 JP/HK/KR 三个地区的动态组：
  - relay 组：`🛣️ Japan|HongKong|Korea`
  - 稳定地区入口组：`🌟/🔒/🤯 {Japan|HongKong|Korea}`
  - 落地组：`🛬 {base}` 与落地池 `🔒 落地`
- 保持 `extra_proxies_yaml` 为正式官方能力；`extra_proxy_providers_yaml` 保持可选。
- 对 `/Users/ivan/Downloads/mihomo.yaml` 生成可证明的“功能等价”输出，并给出脱敏示例与差异说明。

### Non-goals

- 不内置敏感配置内容到仓库。
- 不保证 YAML 注释/anchors 原样保留。
- 不扩展 JP/HK/KR 之外的首批稳定地区组。
- 不新增 provider 自动抓取逻辑。

## 范围（Scope）

### In scope

- 后端用户级 Mihomo mixin 配置存储、管理 API 与兼容读取。
- 订阅接口 `format=mihomo` 的系统动态组生成、mixin 合并与悬挂引用裁剪。
- Web 用户详情页的 Mihomo mixin 编辑、保存与预览语义迁移。
- 单测/集成测试/前端测试与共享测试机真实 Mihomo 校验。
- Spec、契约文档与设计文档同步到 mixin 语义。

### Out of scope

- 不重做现有 raw/clash 命名规则。
- 不追求 YAML 行级一致；验收以行为等价为准。
- 不把示例中的具体动态节点名继续暴露为 mixin 的稳定接口。

## 需求（Requirements）

### MUST

- `GET /api/sub/{token}?format=mihomo` 支持完整输出。
- 用户 mixin 按 `user_id` 持久化存储。
- 管理 API 对外主字段为 `mixin_yaml`；请求兼容旧字段 `template_yaml`，响应只返回 `mixin_yaml`。
- 内部状态/WAL/snapshot 在兼容窗口内仍需可读取旧字段，并保持滚动升级期间的跨版本回放兼容。
- 用户可输入 `extra_proxies_yaml`（sequence）与 `extra_proxy_providers_yaml`（mapping，可空）。
- 渲染时系统重建并覆盖 `proxies`、`proxy-providers` 与所有系统保留动态组。
- 系统固定只生成 JP/HK/KR 三组 relay/稳定地区入口/落地逻辑。
- `proxy-providers` 视为一个整体普通节点池；地区入口组候选来自 provider 全集；当 provider 为空时仍必须生成可加载配置。
- `extra_proxies_yaml` 中的节点会并入最终 `proxies`，并对稳定入口组可见。
- 落地组生成遵循“直连优先，`base-reality/base-ss` 兜底，`ss` 不优先”：
  - 存在 `base-reality` 时，不再把 `base-ss` 及其链式加入 `🛬 {base}`；
  - 仅存在 `base-ss` 时，优先 `base-JP/HK/KR`，并以 `base-ss` 兜底。
- 节点名冲突自动稳定重命名并记录告警日志。
- mixin 缺失时 `format=mihomo` 回退 clash。

### SHOULD

- `mixin_yaml` / extra YAML 在写入前做根类型校验并返回可读错误。
- 保存时若 `mixin_yaml` 顶层包含 `proxies` / `proxy-providers`，服务端自动抽取到 `extra_*`，减少误用。
- 若管理员同时在 `mixin_yaml` 顶层和对应 `extra_*` 字段里提供同类动态段，服务端返回 `invalid_request`，避免静默覆盖。
- 输出订阅示例应提供脱敏片段与差异说明，方便人工复核。

### COULD

- 后续扩展更多链式区域与可配置 filter，但不属于本轮验收。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员在用户详情页编辑并保存 Mihomo mixin config。
- 用户订阅拉取 `format=mihomo` 时：
  - 读取用户 `mixin_yaml`；
  - 读取 `extra_proxy_providers_yaml` 作为普通节点池（可空）；
  - 生成主力节点：`-reality`、`-ss`、`-JP/HK/KR`；
  - 合并 `extra_proxies_yaml`；
  - 覆盖并注入系统保留动态组；
  - 裁剪或重映射 mixin 中残留的未知动态引用；
  - 输出可直接导入的 Mihomo YAML。

### Edge cases / errors

- `mixin_yaml` 非法或根类型错误：管理 API 返回 `400 invalid_request`。
- extra YAML 根类型不符合约束：管理 API 返回 `400 invalid_request`。
- 用户只传旧字段 `template_yaml`：请求仍可成功，响应规范化为 `mixin_yaml`。
- 未配置 mixin：订阅接口回退 clash 输出。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                                 | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）       | 备注（Notes）              |
| ------------------------------------------------------------ | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------------- | -------------------------- |
| `GET /api/sub/{subscription_token}?format=mihomo`            | HTTP API     | external      | Existing       | ./contracts/http-apis.md | backend         | mihomo client/web preview | 无 mixin 时回退 clash      |
| `GET /api/admin/users/{user_id}/subscription-mihomo-profile` | HTTP API     | internal      | Changed        | ./contracts/http-apis.md | backend         | web admin                 | 响应主字段为 `mixin_yaml`  |
| `PUT /api/admin/users/{user_id}/subscription-mihomo-profile` | HTTP API     | internal      | Changed        | ./contracts/http-apis.md | backend         | web admin                 | 兼容旧字段 `template_yaml` |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given 用户已配置 mixin，When 拉取 `format=mihomo`，Then 返回 YAML 包含系统生成的 `-reality`、`-ss`、`-JP/HK/KR` 节点。
- Given 用户配置了多个 `proxy-providers`，When 拉取 `format=mihomo`，Then relay 组 `🛣️ Japan|HongKong|Korea` 的 `use` 包含这些 provider。
- Given `proxy-providers` 为空，When 拉取 `format=mihomo`，Then 稳定地区入口组仍存在、订阅仍可加载，且不出现不存在的 proxy/provider/group 引用。
- Given 仅存在 `extra_proxies_yaml`，When 拉取 `format=mihomo`，Then extra proxies 仍出现在最终 `proxies` 中，且不会额外生成由系统托管的 `🛬 {base}` 落地组。
- Given `extra_proxies_yaml` 中包含名称看起来像系统动态后缀（如 `-JP` / `-reality`）的静态节点，When 业务组显式引用这些节点，Then 引用仍绑定到这些 extra proxies，而不会被错误重映射到系统生成节点。
- Given 存在 `base-reality` 与 `base-ss` 同时可用，When 生成 `🛬 {base}`，Then `🛬 {base}` 只包含 `base-reality`。
- Given 仅存在 `base-ss`（无 `base-reality`），When 生成 `🛬 {base}`，Then `🛬 {base}` 优先包含 `base-JP/HK/KR`，并以 `base-ss` 兜底。
- Given 请求体只提供旧字段 `template_yaml`，When 保存并再次读取 profile，Then 返回体只包含 `mixin_yaml`。
- Given `/Users/ivan/Downloads/mihomo.yaml` 作为目标示例，When 在共享测试机生成 `format=mihomo` 输出，Then 必须能展示一份脱敏但结构真实的订阅片段，并说明系统托管动态段带来的结构差异与业务行为等价证据。
- Given 目标示例自带脱敏后的静态 `proxies`（如遮蔽的 REALITY 公钥），When 需要做真实 Mihomo `-t` 校验，Then 应基于同一份业务 mixin 去掉这些不可解析的脱敏静态节点后再校验，以验证系统生成的动态层、provider 池与业务分组仍可被 Mihomo 实际加载。
- Given 现有 `raw/clash/base64` 调用，When 回归测试，Then 输出语义与 content-type 不回归。

## 实现前置条件（Definition of Ready / Preconditions）

- API 主字段命名、兼容窗口与回退策略已冻结。
- 首批稳定地区集合已冻结为 JP/HK/KR。
- “功能等价而非文本等价”的验收口径已冻结。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `src/subscription.rs` 覆盖动态组注入、provider 为空、冲突重命名、根类型错误、悬挂引用裁剪。
- Integration tests: `src/http/tests.rs` 覆盖 admin profile API 的 `mixin_yaml` 主字段、旧字段兼容、autosplit、`format=mihomo` 渲染与 provider 为空场景。
- Web tests: `web/**` 覆盖 mixin 语义迁移、请求/响应兼容与 UI 文案。
- Shared testbox: 在 `codex-testbox` 生成目标示例的真实输出；若源示例已脱敏且包含不可解析静态节点，则使用去掉这些节点的 provider-only 变体执行真实 Mihomo `-t` 校验，并保留差异说明。

### Quality checks

- `cargo test`
- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`

## 文档更新（Docs to Update）

- `docs/desgin/subscription.md`
- `docs/specs/rbt5e-mihomo-subscription-template/contracts/http-apis.md`
- `docs/specs/README.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/rbt5e-mihomo-subscription-template/assets/`

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 状态层新增 `UserMihomoProfile` 持久化与 Raft 命令
- [x] M2: HTTP 管理 API + 订阅 `format=mihomo` 路由落地
- [x] M3: Mihomo 渲染引擎（系统节点生成 + 合并 + 冲突重命名 + relay use 注入）
- [x] M4: Web UserDetails 编辑 + `mihomo` 预览
- [x] M5: 测试补齐与质量门禁通过
- [x] M6: 管理 API 主字段切换为 `mixin_yaml`，并兼容读取旧字段 `template_yaml`
- [x] M7: 订阅渲染内置 JP/HK/KR 稳定入口组与落地池，mixin 不要求包含动态组定义
- [x] M8: provider 为空 / extra proxies 保留 / 共享测试机 Mihomo 校验与脱敏输出证明

## 方案概述（Approach, high-level）

- 以“混入配置（mixin config）”为静态补充，后端在渲染阶段覆盖动态段并注入系统保留动态组。
- 将高风险自由输入限制为 YAML 根类型校验 + autosplit + 悬挂引用裁剪，保证配置最小正确性。
- 通过系统托管动态组与 deterministic remap/prune，避免用户 mixin 持续依赖具体动态节点名。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：用户输入 mixin 仍可能包含客户端不兼容字段，服务端只保证结构正确性与已知动态组约束。
- 风险：示例配置的“功能等价”需要依赖真实测试机验证，不能仅凭文本 diff 判断。
- 假设：JP/HK/KR relay 组名称在短期内稳定。

## 变更记录（Change log）

- 2026-03-04: 创建规格并冻结初版 API/渲染/回退口径。
- 2026-03-04: 完成首版实现与验证（`cargo test`、`web lint/typecheck/test`），PR #95。
- 2026-03-06: 需求升级为“mixin + 系统内置动态组逻辑”，新增稳定入口组、落地组策略与 autosplit 防误用机制。
- 2026-03-06: 对外主字段切换为 `mixin_yaml`，兼容读取旧字段 `template_yaml`；稳定地区范围锁定为 JP/HK/KR。
- 2026-03-06: 在 `codex-testbox` 生成示例输出并完成两类证据：原样例的脱敏输出/差异分析，以及去掉已脱敏静态节点后的 provider-only 变体 Mihomo `-t` 通过记录。
- 2026-03-06: review 收口补充滚动升级兼容：内部 `UserMihomoProfile` 持久化继续写旧字段形状，管理 API 仍只返回 `mixin_yaml`；同时收紧落地组注入范围，避免 `extra_proxies_yaml` 单独触发系统托管 `🛬 {base}` 组。
- 2026-03-06: review 收口补充 extra proxy 引用保护：对显式 extra proxies 的名称保持最高优先级，即便名称带有 `-JP` / `-HK` / `-KR` / `-ss` / `-reality` 后缀，也不再被系统动态 remap 误绑。

## 参考（References）

- 样例配置：`/Users/ivan/Downloads/mihomo.yaml`
