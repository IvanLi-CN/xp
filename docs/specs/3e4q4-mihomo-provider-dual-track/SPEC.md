# Mihomo provider-only 动态链式订阅（#3e4q4）

## 状态

- Status: 已完成
- Created: 2026-04-17
- Last: 2026-06-14

## 背景 / 问题陈述

- 当前 `format=mihomo` 已支持系统动态节点 + 用户 mixin，但系统节点仍直接写入最终 `proxies`，一旦项目自己的入口地址、端口或节点集合变化，就需要整体刷新主配置。
- 这对真实客户端并不友好：用户导入的是完整配置而不是独立 provider，入口池变化无法通过 Mihomo 自身的 `proxy-provider` 拉取机制独立更新。
- 现网已切到 provider 默认方案；继续保留 legacy Mihomo 会增加双轨测试和 UI 心智成本，且不能满足链式节点随 provider 动态更新的目标。

## 目标 / 非目标

### Goals

- 移除 legacy Mihomo 输出与全局 delivery mode，`GET /api/sub/{token}?format=mihomo` 固定返回 provider 主配置。
- 保留显式 provider 回归路径：
  - `GET /api/sub/{token}/mihomo/provider`
  - `GET /api/sub/{token}/mihomo/provider/system`
- provider 方案采用单一系统 provider `xp-system-generated`，将系统直连节点与链式节点都移入 provider payload。
- 链式节点命名为 `{base}-ss-chain` / `{base}-reality-chain`，`dialer-proxy` 指向按 `Node.access_host` 聚合生成的 per-base relay 组 `🛣️ {relay-base}`。
- provider 主配置中的地区组、`💎 高质量`、`🚀 节点选择` 与 `🤯 All` 改为基于节点主动探测得到的订阅地区自动生成，并固定暴露 `Japan/HongKong/Taiwan/Korea/Singapore/US/Other`。
- 管理端只展示 provider-only 状态；用户详情页复制/预览 canonical Mihomo URL。
- 冻结部署无关的 relay 健康检查合同：只要节点以“托管默认 VLESS endpoint”身份存在，host-managed 与 container-managed 都必须用同一合同决定 `reality.dest` 改写与 Mihomo relay `url-test` URL 选择。

### Non-goals

- 不改 `raw` / `base64` / `clash` 输出。
- 不做按用户维度的 Mihomo delivery mode。
- 不承诺 provider 主配置继续兼容手写系统节点名；系统节点应通过 provider `use + filter` 消费。
- 不按 endpoint probe 健康状态过滤节点；有 access host 与用户 endpoint membership 即视为可输出。

## 范围（Scope）

### In scope

- 后端 provider-only Mihomo HTTP 路由与 provider payload 渲染。
- provider 主配置渲染、provider payload 渲染、请求 origin 解析。
- Web `Settings / Service config` provider-only 状态展示与 `User Details` canonical Mihomo URL。
- Storybook / 前后端回归 / 真实 Mihomo provider 装载验证。
- 设计文档与契约文档同步。

### Out of scope

- 不扩展更多 provider 名称或多套系统 provider。
- 不替换当前 `UserMihomoProfile` 结构。
- 不改 `raw` / `base64` / `clash` 输出。
- 不把 host-managed 限缩成“仅兼容运行、不给升级语义”；部署/升级合同必须覆盖既有 host-managed 节点与单镜像容器节点两条 owner-facing 路径。

## 需求（Requirements）

### MUST

- `GET /api/sub/{token}?format=mihomo` 必须输出 provider 主配置。
- `GET /api/sub/{token}/mihomo/legacy` 不再是可用订阅路径。
- `GET /api/sub/{token}/mihomo/provider` 必须输出 provider 主配置。
- `GET /api/sub/{token}/mihomo/provider/system` 必须返回合法 `text/yaml`，根为 `proxies:`。
- provider 主配置里的系统 provider 名称固定为 `xp-system-generated`；若用户 `extra_proxy_providers_yaml` 里占用了同名 provider，服务端返回清晰错误。
- provider 主配置里的 provider `url` 必须基于请求对外 origin 生成，而不是直接复用内网 `api_base_url`。
- provider 方案中：
  - 顶层 `proxy-providers` = `xp-system-generated` + `extra_proxy_providers_yaml`
  - 顶层 `proxies` = `extra_proxies_yaml`，不枚举系统生成节点
  - `xp-system-generated` payload = 系统 `{base}-ss` / `{base}-reality` / `{base}-ss-chain` / `{base}-reality-chain`
  - per-base relay 组 `🛣️ {relay-base}`、hidden source 组 `🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`、owner-facing `🔒 {Region}`、hidden `🤯 {Region}`、`💎 高质量`、`🚀 节点选择`、`🤯 All`、`🛬 {base}`、`🔒 落地` 保持可用
- provider 方案必须按 `Node.access_host` 聚合生成 per-base relay 组；同一 `access_host` 下的多个落地节点共享同一个 `🛣️ {relay-base}`，不同 `access_host` 不得合并到同一个 relay 组。`relay-base` 必须保留 access host 分隔符差异，避免 `a.b.example.com` / `a-b.example.com` 这类 host 退化成同一 slug 后按当前集合计数重命名；不得直接占用 `Japan/HongKong/Taiwan/Korea/Singapore/US/Other` 等历史地区 alias 名称，命中保留名时必须加内部前缀做消歧。
- provider 方案下 `🛬 {base}` 必须通过 `use: [xp-system-generated]` 与精确 filter 消费 `{base}-ss-chain` / `{base}-reality-chain`，且 Mihomo 运行时候选顺序必须稳定为 ss-chain 在前、reality-chain 在后。
- provider 方案下 `🔒 高质量` 与 `🔒 {Region}` 必须能通过 `xp-system-generated` 动态消费 `{base}-reality` 直连接入点；`{base}-ss` 仍只作为 provider payload 原料，不作为本次接入点目标。
- `💎 高质量` 作为 owner-facing 高质量入口必须保留兜底层：无论用户 mixin 是否显式声明，它都必须至少包含 `🔒 高质量`，并且在最终输出中还必须提供一个非地区直连接入面的兜底聚合入口；当前命名族中该兜底入口为 `🤯 All`。若未来重命名或替换聚合组，仍必须保留“高质量入口之上存在全局兜底层”的语义，不能让 `💎 高质量` 退化成仅剩单一路径且无兜底的壳组。
- 最终 Mihomo 配置图的强约束由 [contracts/final-mihomo-config.md](./contracts/final-mihomo-config.md) 定义：
  - `🔒 {Region}` 是 visible `select` 叶子组，只承载直连 reality 候选
  - `🌟 {Region}` 是 hidden `fallback`，且只能包装 `🔒 {Region}`
  - `🤯 {Region}` 是 hidden `url-test`，且只能包装 `🌟 {Region}`
  - `🔒 高质量` 是 visible `select`
  - `💎 高质量` 是 hidden `fallback`
  - `🤯 All` 是 hidden `url-test`
  - `💎 节点选择` 是 hidden `fallback`
- per-base relay 组不得消费 `xp-system-generated`，避免链式节点的 `dialer-proxy` 递归选中自身；有外部 provider 时使用日本/香港/新加坡 filter 做 `url-test`，并保留 `DIRECT` 兜底以防 provider 候选被 filter 筛空；健康检查 URL 选择顺序必须是：
  - 同一 `access_host` 下存在至少一个托管 VLESS endpoint 时，使用最小 VLESS 端口对应的 `https://<access_host[:port]>/generate_204`
  - 否则当同一 `access_host` 下只有一个公开 `api_base_url` 时，使用该 API health URL
  - 否则回退到 `https://www.gstatic.com/generate_204`
- 输出不得再生成共享 `🛣️ JP/HK/SG` 主路径，也不得生成 `🛣️ {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}` 兼容地区别名；旧共享外层与旧地区 relay alias 引用只允许被清理或移除，不得重新展开为共享中转语义。
- `PUT /api/admin/users/{user_id}/subscription-mihomo-profile` 必须先做最终 provider 主配置 + `/mihomo/provider/system` payload 的联合预渲染校验；任何未定义的 `proxies`、`use`、`dialer-proxy` 或 `rules` 引用都必须返回 `400 invalid_request`，不得静默 remap、裁剪或回退。
- 服务端不得自动抽取、重写或规范化用户输入中的 `mixin_yaml.proxies` / `mixin_yaml.proxy-providers`，也不得把 legacy relay alias、旧 landing 引用或保留名冲突转换成“兼容修复”。
- provider 主配置里的 hidden per-base relay 组必须统一移动到系统托管组尾部，不能插在 `💎 高质量` 与地区组之间。
- provider 主配置里的系统 owner-facing 地区组 `🔒 {Region}` 与 hidden source 组 `🌟 {Region}` 必须以节点主动探测归类为主；但对尚未产生首次成功探测结果的历史节点，渲染阶段会先沿用 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以避免升级瞬间清空原有地区组。首次成功探测落盘后，仅在 probe 未 stale 时继续把 `subscription_region` 视为权威；probe stale 后渲染回退到 legacy slug fallback / `Other`。
- `GET /api/health` 与 `GET /api/admin/config` 必须增量暴露 `vless_https_canary` 运行态，包括 enabled/bind、证书到期时间与最近一次续期错误；管理面视图保持只读，不提供手工修改接入面 probe URL 的入口。
- legacy Mihomo 路径已移除；raw/base64/clash 路径不得回归。

### SHOULD

- `PATCH /api/admin/config` 应只接受可写字段，保留其它配置只读。
- provider payload 与主配置应共享同一套系统节点命名与分组逻辑，避免悬挂引用。
- 前端订阅 URL 选择应只暴露 canonical `mihomo(provider)`。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员在 `Settings / Service config` 查看 Mihomo provider-only 状态。
- 普通 Mihomo 客户端继续使用 canonical URL；其实际返回 provider 主配置。
- 回归/测试场景通过 canonical/provider URL 与 provider system payload 验证。
- provider 主配置加载后，Mihomo 自动拉取 `/mihomo/provider/system` 获取系统直连与链式节点；链式代理经按 `access_host` 聚合的 `🛣️ {relay-base}` 做外层中转。

### Edge cases / errors

- 用户未配置 Mihomo profile 时，canonical `?format=mihomo` 与显式 provider 路径回退 clash；`/mihomo/provider/system` 始终返回系统 provider payload，不依赖用户 mixin。
- 当 `extra_proxy_providers_yaml` 已包含 `xp-system-generated` 时，保存 profile 成功但渲染 provider 路径返回 `400 invalid_request`，提示保留名冲突。
- 当请求头无法推导外部 origin 时，provider 主配置回退到 `Config.api_base_url` 的规范化 origin。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                  | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 使用方（Consumers） |
| --------------------------------------------- | ------------ | ------------- | -------------- | ------------------------ | ------------------- |
| `GET /api/sub/{token}?format=mihomo`          | HTTP API     | external      | Changed        | ./contracts/http-apis.md | Mihomo clients      |
| `GET /api/sub/{token}/mihomo/provider`        | HTTP API     | external      | Existing       | ./contracts/http-apis.md | Mihomo clients      |
| `GET /api/sub/{token}/mihomo/provider/system` | HTTP API     | external      | New            | ./contracts/http-apis.md | Mihomo clients      |
| `GET /api/admin/config`                       | HTTP API     | internal      | Changed        | ./contracts/http-apis.md | Web admin           |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given 请求 `GET /api/sub/{token}?format=mihomo`，Then 返回 provider 主配置，且 `proxy-providers.xp-system-generated.url` 指向同一外部 origin 下的 `/api/sub/{token}/mihomo/provider/system`。
- Given 请求 `GET /api/sub/{token}/mihomo/legacy`，Then 不再返回 legacy Mihomo 主配置。
- Given 请求 `/mihomo/provider/system`，When 返回 provider payload，Then 返回 `proxies:` YAML，且包含系统直连与链式节点（`-ss` / `-reality` / `-ss-chain` / `-reality-chain`）。
- Given provider 方案同时存在 `base-reality` 与 `base-ss`，When 检查 `🛬 {base}`，Then 该组只通过 provider filter 暴露 `{base}-ss-chain` / `{base}-reality-chain`，并在 Mihomo 运行时按 ss-chain、reality-chain 顺序展示。
- Given provider 方案同时存在 `base-reality` 与 `base-ss`，When 检查 `🔒 高质量`，Then 该组能动态包含 `{base}-reality` 接入点，且不会把 `{base}-ss` 作为系统直连接入候选。
- Given provider 方案同时存在 `base-reality` 与 `base-ss`，When 检查 `🔒 {Region}`，Then 对应地区组能动态包含 `{base}-reality` 接入点。
- Given 两个落地节点共享同一 `Node.access_host`，When 请求 provider 主配置与 system payload，Then 只生成一个 per-base relay 组，且两个节点的 `*-chain.dialer-proxy` 都指向该组。
- Given 两个落地节点使用不同 `Node.access_host`，When 请求 provider 主配置与 system payload，Then 生成不同 per-base relay 组，且链式节点不会合并到共享 `🛣️ JP/HK/SG`。
- Given 同一 `access_host` 下存在托管 VLESS endpoint，When 请求 provider 主配置，Then 对应 relay 组 `url` 必须使用最小托管 VLESS 端口对应的 `https://<access_host[:port]>/generate_204`。
- Given host-managed node upgrades from a legacy single-endpoint VLESS deployment, When the new `xp` version starts or `xp-ops xp sync-node-meta` runs, Then that lone VLESS endpoint is auto-adopted into the managed-default contract only when its metadata still predates the `managed_default` flag, but `reality.dest` is only rewritten after the loopback canary is ready; if canary preparation fails, the existing endpoint stays untouched and the blocker is exposed through `vless_https_canary_status.last_error`.
- Given 同一 `access_host` 下不存在托管 VLESS endpoint，When 请求 provider 主配置，Then relay 组必须回退到“唯一公开 `api_base_url` 的 `/api/health`”，若仍不能唯一确定，则回退到 `https://www.gstatic.com/generate_204`。
- Given 落地节点基名恰好是 `Japan` / `HongKong` / `Singapore` 等历史地区名，When 请求 provider 主配置与 system payload，Then per-base relay 组必须消歧为内部 relay 名，不得重新输出 `🛣️ {Region}`。
- Given provider 主配置，When 检查顶层 `proxies`，Then 不包含系统生成的 `{base}-ss` / `{base}-reality` / `{base}-ss-chain` / `{base}-reality-chain`。
- Given provider 主配置，When 检查 `proxy-groups` 与用户组引用，Then 不再出现 `🛣️ {Region}` 兼容地区别名，也不再出现共享 `🛣️ JP/HK/SG` 主路径。
- Given 任何 Mihomo profile，When 最终 provider 主配置或 system payload 中存在未定义引用，Then `PUT` 必须返回 `400 invalid_request`，并指出未定义引用所在字段/组名。
- Given 用户在 `mixin_yaml` 内写入 `proxies` 或 `proxy-providers`，When 保存 profile，Then 服务端保留原始输入，不做自动抽取；坏数据只在最终渲染校验阶段失败。
- Given provider 主配置，When 检查 `proxy-groups` 顺序，Then hidden `🛣️ {relay-base}` 必须排在 `🚀 节点选择` 之后。
- Given provider 主配置，When 检查系统托管 `proxy-groups` 顺序，Then 必须稳定满足：
  `🔒 高质量`
  `-> 💎 高质量`
  `-> 🔒 {Japan..Other}`
  `-> 🌟 {Japan..Other}`
  `-> 🤯 {Japan..Other}`
  `-> 🛬 {base}`
  `-> 🔒 落地`
  `-> 🤯 All`
  `-> 🚀 节点选择`
  `-> 💎 节点选择`
  `-> 🛣️ {relay-base}`，
  且 canonical region order 固定为 `Japan/HongKong/Taiwan/Korea/Singapore/US/Other`。
- Given 新增节点完成主动探测并被归类到 `Taiwan`，When 请求 provider 主配置，Then `🌟 Taiwan`、`💎 高质量` 与 `🚀 节点选择` 会自动包含对应 `🛬 {base}`，无需更新用户模板。
- Given provider 主配置，When 检查 `💎 高质量` 相关聚合语义，Then 最终输出必须保留“高质量入口 + 全局兜底入口”两层结构；若 `💎 高质量` 本身不直接引用 `🤯 All`，则必须存在另一个 owner-facing 包装组稳定同时暴露 `💎 高质量` 与 `🤯 All`，不能让最终可见入口缺失全局兜底。
- Given 任一最终 Mihomo 配置，When 检查地区组三元关系，Then 必须满足 `🤯 {Region} -> 🌟 {Region} -> 🔒 {Region} -> leaf proxies`，且 `🔒` 只能被 `🌟` 作为单跳包装引用。
- Given 托管 VLESS endpoint 已启用，When 请求 `https://<access_host[:port]>/generate_204`，Then 现有 VLESS/REALITY 接入点会把未认证 HTTPS 流量转到 xp 进程内 loopback TLS canary 并返回 `204`，且不新增公网 probe listener。
- Given 托管 VLESS endpoint 被创建或 reconcile，When 检查 Xray inbound Reality 配置，Then `dest` 必须等于 `XP_VLESS_CANARY_BIND`，`server_names` 必须等于 `[node.access_host]`，且 SNI 不包含端口。
- Given Admin UI/API 修改托管 VLESS endpoint，When payload 包含 `reality.dest`、`server_names` 或 `server_names_source`，Then 系统必须拒绝或忽略该修改，不能让隐藏 UI 字段绕过托管协议事实。
- Given 非 `/generate_204` 请求进入 canary，When `Host` / HTTP/2 `:authority` 归一化为 `access_host[:endpoint_port]`，Then 必须匹配同 node 上唯一托管 VLESS endpoint；0 个匹配返回可诊断未匹配错误，多个匹配返回冲突错误，匹配但未设置 `canary_upstream` 返回未配置 upstream 错误。
- Given endpoint 设置了 origin-only `canary_upstream`，When canary 代理非探测请求，Then 请求 method/path/query/body、非 hop-by-hop header、响应 status/header/body 应尽量透明流式转发；发往 upstream 的 `Host` 按 `canary_upstream` origin 归一化；HTTP/1.1、HTTPS ALPN HTTP/2、显式 h2c、SSE、大上传/下载属于支持范围，WebSocket upgrade 使用 HTTP/1.1 upstream 连接，显式 h2c 仅用于非 upgrade HTTP 流量，`CONNECT` 不属于 v1 反代能力。
- Given deployment mode is host-managed or container-managed, When the node declares or auto-adopts a managed default VLESS endpoint, Then the same managed-default marker / reconcile contract determines both `reality.dest` rewriting and Mihomo relay URL selection; delivery semantics must not differ by deployment mode.
- Given Web 管理端打开 `Settings / Service config`，Then 显示 Mihomo provider-only 状态，且 `User Details` 可复制/预览 canonical Mihomo URL。
- Given 真实 Mihomo 加载显式 provider URL，When 执行 `mihomo -t` 或运行时 delay 检查，Then provider 内链式节点可引用主配置中的 per-base relay 组 `🛣️ {relay-base}`。

## 实现前置条件（Definition of Ready / Preconditions）

- Provider-only URL 语义与 provider 保留名已冻结。
- provider 路径只保证系统组与链式逻辑兼容的口径已冻结。
- 旧全局默认开关不再影响 canonical Mihomo 输出。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust unit/integration tests：覆盖 provider-only 订阅路径、provider payload、origin 解析、保留名冲突与 legacy 路由移除。
- Web tests：覆盖 provider-only service config 状态与 user details canonical Mihomo URL。
- Storybook：为 `ServiceConfigPage` / `UserDetailsPage` 增加 provider-only 状态与交互覆盖。
- Mihomo smoke：至少一次真实 Mihomo provider 装载与 provider 内链式 `dialer-proxy` 验证。

### Quality checks

- `cargo test`
- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run test-storybook`

## 文档更新（Docs to Update）

- `docs/desgin/subscription.md`
- `docs/desgin/api.md`
- `docs/ops/README.md`
- `docs/ops/docker.md`
- `docs/specs/README.md`
- `docs/specs/3e4q4-mihomo-provider-dual-track/contracts/http-apis.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/3e4q4-mihomo-provider-dual-track/assets/`

## Visual Evidence

- source_type=storybook_canvas · target_program=mock-only · capture_scope=element
  - state: `Pages/ServiceConfigPage/ProviderOnly`
  - evidence_note: 管理端 `Settings / Service config` 展示 Mihomo 已收敛为 provider-only，移除 legacy/default route 切换。
    ![Service config provider-only Mihomo delivery](./assets/service-config-provider-only.png)
- Real Mihomo validation
  - environment: local `mihomo v1.19.24`
  - result: provider-hosted `*-ss-chain` can reference a main-config `dialer-proxy`; missing main-config dialer fails immediately, proving provider payload chains depend on main-config relay groups.

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: follow-up spec / 设计文档 / 契约冻结双轨语义
- [x] M2: provider-only admin config 与订阅路由
- [x] M3: provider 主配置 / payload 渲染 + origin 解析 helper
- [x] M4: provider payload 动态承载直连与链式节点
- [x] M5: Web 设置页与订阅 URL provider-only UI + Storybook
- [x] M6: 回归测试、视觉证据、共享测试机 Mihomo 验证
- [x] M7: PR / review / merge / cleanup

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：provider 路径若把 `{base}-reality` 或 `{base}-ss` 直连暴露到用户可见组，容易绕过链式中转，需要用测试锁死。
- 风险：请求头组合在反向代理下可能非常杂，需要优先以 live 请求头为准，并保留 `api_base_url` 回退。
- 假设：项目自己的系统 provider 名称 `xp-system-generated` 当前未被现有用户配置占用；若占用，返回显式错误即可。
- 部署支持矩阵：
  - host-managed `systemd` 节点：正式支持，包含历史单 VLESS endpoint 的升级自动接管语义
  - host-managed `OpenRC` 节点：正式支持，合同与 `systemd` 节点一致
  - single-image Docker / Compose 节点：正式支持，由 `xp-ops container run` 提供同等 managed-default 合同
- 当前受限环境说明必须继续显式保留在 ops 文档：
  - `xp-ops deploy/init/install` 的宿主机发行版自动化识别目前覆盖 Arch/Debian/Ubuntu/RHEL-family/Alpine
  - 历史 CentOS 7 / RHEL-family host-managed 节点属于支持的运行/升级/部署目标，文档应明确其与 Arch/Debian/Ubuntu host-managed 节点一样走标准 `xp-ops deploy` / `xp-ops upgrade` 路径

## 变更记录（Change log）

- 2026-04-17: 创建规格并冻结双轨 URL、provider 保留名与双轨 admin 设置语义。
- 2026-04-17: 完成全局 `mihomo_delivery_mode`、显式 dual-track 路由、Storybook/真实 Mihomo provider 验证与文档同步。
- 2026-04-24: provider 主配置的系统地区组切换为 probe-derived 固定地区面，并补充 `🌟 Other`、`💎 高质量` / `🚀 节点选择` 自动补点语义。
- 2026-05-02: 冻结 provider-only Mihomo 口径；系统 provider 动态输出直连与链式节点，主配置通过 provider filter 消费链式候选。
- 2026-05-02: 修正 provider-only 高质量与地区接入点口径；`🔒 高质量` / `🔒 {Region}` 动态包含系统 `{base}-reality`，`🛬 {base}` 通过 system provider payload 顺序保证 ss-chain 先于 reality-chain。
- 2026-06-14: relay 外层中转从共享 `🛣️ JP/HK/SG` 改为按 `Node.access_host` 聚合的 per-base relay 组，并删除 `🛣️ {Region}` 兼容地区别名。
- 2026-06-15: Mihomo profile 保存收紧为“预渲染联合校验 + 明确 invalid_request”；移除静默 remap / prune / autosplit，hidden relay 组统一移到系统托管组尾部。
- 2026-06-15: 明确补充高质量入口兜底合同；`💎 高质量` 之上必须存在稳定的全局兜底聚合入口，不能因 mixin 缺失或系统组收敛而消失。
- 2026-06-16: relay 外层健康检查切到托管 VLESS 端口自身的 HTTPS canary（`https://<access_host[:port]>/generate_204`），并冻结地区组合同为：`🌟 {Region}` 是 hidden source group，`🔒 {Region}` 是 owner-facing 可见入口，`🤯 {Region}` 是 hidden `url-test` 包装组。
- 2026-06-23: 托管 VLESS 的 SNI/dest 收敛为固定协议事实：`server_names=[node.access_host]`、`dest=XP_VLESS_CANARY_BIND`；canary 增加 endpoint-level `canary_upstream` 透明 TLS 终止反代，并通过 HTTP authority 匹配 endpoint。
