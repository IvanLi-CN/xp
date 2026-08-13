# 规格（Spec）总览

本目录用于管理工作项的**规格与追踪**：记录范围、验收标准、任务清单与状态，作为交付依据；实现与验证应以对应 `SPEC.md` 为准。

> Legacy compatibility: historical repos may still contain `docs/plan/**/PLAN.md`.
> New entries must be created under `docs/specs/**/SPEC.md`.

## 快速新增一个规格

1. 选择稳定、无编号的 topic slug。
2. 新建目录 `docs/specs/<topic>/`，并创建 `SPEC.md`、`IMPLEMENTATION.md` 与 `HISTORY.md`。
3. 在下方 Slug-only topics 表新增一行；生命周期与实现状态分别记录。

## 目录与命名规则

- 新规格目录使用 `docs/specs/<topic>/`；topic 为稳定的 kebab-case slug，不分配 ID。
- 现有 `docs/specs/<id>-<title>/` 属于只读兼容命名，可继续维护但不用于新规格。

### Legacy ID-based naming（仅兼容现有规格）

- `<id>`：推荐 5 个字符的 nanoId 风格，一经分配不要变更。
  - 推荐字符集（小写 + 避免易混淆字符）：`23456789abcdefghjkmnpqrstuvwxyz`
  - 正则：`[23456789abcdefghjkmnpqrstuvwxyz]{5}`
  - 兼容：若仓库历史已使用四位数字 `0001`-`9999`，允许继续共存。
- `<title>`：历史目录中的短标题 slug；目录名尽量稳定。
- 历史人类可读标题写在 Legacy ID-based Index 的 `Title` 列。

## Legacy Index 状态说明

以下状态值仅适用于历史 ID-based Index。新 topic 使用 Slug-only topics 的 `Lifecycle` 与 `Implementation` 两列，不与本字段混用。

- `待设计`：范围/约束/验收标准尚未冻结，仍在补齐信息与决策。
- `待实现`：规格已冻结，可开工；实现与测试验证应以该规格为准。
- `跳过`：计划已冻结或部分完成，但**当前明确不应自动开工**（例如需要特定时机/外部条件/等待依赖）；自动挑选“下一个规格”时应跳过它。需要实现时再把状态改回 `待实现`（或由主人显式点名实现该规格）。
- `部分完成（x/y）`：实现进行中；`y` 为该规格里定义的“实现里程碑”数，`x` 为已完成“实现里程碑”数（见该规格 `SPEC.md` 的 Milestones；不要把计划阶段产出算进里程碑）。
- `已完成`：该规格已完成（实现已落地或将随某个 PR 落地）；如需关联 PR 号，写在 Index 的 `Notes`（例如 `PR #123`）。
- `作废`：不再推进（取消/价值不足/外部条件变化）。
- `重新设计（#<id>）`：该规格被另一个规格取代；`#<id>` 指向新的规格编号。

## `Last` 字段约定（推进时间）

- `Last` 表示该规格**上一次“推进进度/口径”**的日期，用于快速发现长期未推进的规格。
- 仅在以下情况更新 `Last`（不要因为改措辞/排版就更新）：
  - `Status` 变化（例如 `待设计` -> `待实现`，或 `部分完成（x/y）` -> `已完成`）
  - `Notes` 中写入/更新 PR 号（例如 `PR #123`）
  - `SPEC.md` 的里程碑勾选变化
  - 范围/验收标准冻结或发生实质变更

## SPEC.md 写法（简要）

每个规格的 `SPEC.md` 至少应包含：

- 背景/问题陈述（为什么要做）
- 目标 / 非目标（做什么、不做什么）
- 范围（in/out）
- 需求列表（MUST/SHOULD/COULD）
- 功能与行为规格（Functional/Behavior Spec：核心流程/关键边界/错误反馈）
- 验收标准（Given/When/Then + 边界/异常）
- 实现前置条件（Definition of Ready / Preconditions；未满足则保持 `待设计`）
- 非功能性验收/质量门槛（测试策略、质量检查、Storybook/视觉回归等按仓库已有约定）
- 文档更新（需要同步更新的项目设计文档/架构说明/README/ADR）
- 实现里程碑（Milestones，用于驱动 `部分完成（x/y）`；只写实现交付物，不要包含计划阶段产出）
- 风险与开放问题（需要决策的点）
- 假设（需主人确认）

## Legacy ID-based Index（兼容表格）

|    ID | Title                                      | Status             | Spec                                                    | Last       | Notes                                                                                                                                                                                                                                                                                                                                             |
| ----: | ------------------------------------------ | ------------------ | ------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|  0013 | Cloudflare Tunnel 远程访问                 | 已完成             | `0013-cloudflare-tunnel-remote-access/SPEC.md`          | 2026-07-28 | 从 legacy plan 迁移；共享 Tunnel 非属配置保护与显式迁移合同                                                                                                                                                                                                                                                                                       |
| d4kex | XP Web DaisyUI -> shadcn/ui 全量迁移       | 已完成             | `d4kex-shadcn-ui-full-migration/SPEC.md`                | 2026-03-09 | Fast-track 全量迁移；Tailwind v4 + shadcn/ui + RHF/Zod + Storybook docs 门禁已落地                                                                                                                                                                                                                                                                |
| gjtxy | XP release 失败 Telegram 告警接入          | 已完成             | `gjtxy-release-failure-telegram-alerts/SPEC.md`         | 2026-04-12 | fast-track release alert rollout                                                                                                                                                                                                                                                                                                                  |
| tc2kp | country.is Hosted IP Geo hard cut          | 已完成             | `tc2kp-country-is-ip-geo/SPEC.md`                       | 2026-04-24 | 免费无门槛 hosted API hard cut；移除 MMDB/DB-IP 更新面与 `/ip-geo-db`；并复用给节点主动探测出口 Geo                                                                                                                                                                                                                                               |
| 3hpk4 | Admin 删除节点确认清理                     | 已完成             | `3hpk4-admin-delete-node/SPEC.md`                       | 2026-05-18 | 从 legacy plan 迁移；节点删除支持 endpoint impact preview 与显式确认清理，保留本机/leader 护栏                                                                                                                                                                                                                                                    |
| jtnnd | 节点 runtime fan-out 超时容忍              | 已完成             | `jtnnd-node-runtime-fanout-timeout-tolerance/SPEC.md`   | 2026-03-11 | runtime list fan-out timeout patch；historical spec left untouched                                                                                                                                                                                                                                                                                |
| k7m2n | 节点历史统计与失联 fallback                | 已完成             | `k7m2n-node-history-fallback/SPEC.md`                   | 2026-05-20 | panel-local node history mirror；90d daily traffic/component status；7d capped status-change fallback for NodeDetails                                                                                                                                                                                                                             |
| much7 | Endpoint probe 在线参与节点分母            | 已完成             | `much7-endpoint-probe-online-participants/SPEC.md`      | 2026-03-11 | participant-based denominator；legacy 24h fallback + web stats/live-run semantics synced                                                                                                                                                                                                                                                          |
| t8m4q | Xray 业务入站 stale TCP reclaim            | 已完成             | `t8m4q-xray-stale-tcp-reclaim/SPEC.md`                  | 2026-06-17 | fast-track；静态 `policy.levels.0` + 动态业务 inbound `socket_settings` + `xp-ops upgrade` rollout + shared testbox real-Xray gate                                                                                                                                                                                                                |
| q4n8p | 节点列表表格优先与跨节点同页跳转           | 已完成             | `q4n8p-node-list-cross-origin-actions/SPEC.md`          | 2026-06-23 | shared node list 桌面表格优先 + `Details/Open on node` 双动作 + cross-origin login handoff；supersedes `puf2g`                                                                                                                                                                                                                                    |
| 6e7e4 | 节点 / 用户入站 IP 使用详情                | 已完成             | `6e7e4-node-user-inbound-ip-usage/SPEC.md`              | 2026-03-22 | PR #102/#110/#115；checks green；mergeable clean；API/Xray/ops docs synced；ECharts/storybook/user-tab/highlight follow-up closed；header highlight badge contrast follow-up（light/dark）+ visual evidence synced；AA contrast regression guard added（prefix + value text）；review clear；online stats false-positive warning follow-up synced |
| 5yd72 | CI workflow YAML 语法修复                  | 已完成             | `5yd72-ci-workflow-yaml-parse-fix/SPEC.md`              | 2026-03-05 | PR #98（type:patch；ci/pr-label-gate/xray-e2e 全绿，恢复发版链路）                                                                                                                                                                                                                                                                                |
| rbt5e | 用户订阅 Mihomo 混入配置                   | 已完成             | `rbt5e-mihomo-subscription-template/SPEC.md`            | 2026-06-19 | PR #100 后续修订；`mixin_yaml` API、raw profile storage、probe-derived 固定地区面、以及用户 mixin 与 provider-only 系统托管层的职责收敛                                                                                                                                                                                                           |
| 3e4q4 | Mihomo provider-only 动态链式订阅          | 已完成             | `3e4q4-mihomo-provider-dual-track/SPEC.md`              | 2026-08-04 | provider-only route；relay `url-test` 使用托管 VLESS 端口自身；managed-default 端口在 bootstrap 后由集群持有，覆盖 host-managed systemd/OpenRC 与 Docker 节点                                                                                                                                                                                     |
| gj4xg | Dashboard/Nodes shared node inventory list | 已完成             | `gj4xg-dashboard-nodes-shared-list/SPEC.md`             | 2026-03-04 | PR #93 + checks green；后续完成 2x2 列合并、窄宽度列表化与截图补充                                                                                                                                                                                                                                                                                |
| 9fke7 | Mihomo subscription/config redact CLI      | 已完成             | `9fke7-mihomo-redact-cli/SPEC.md`                       | 2026-03-03 | PR #92                                                                                                                                                                                                                                                                                                                                            |
| puf2g | Node panel icon-only entry for node lists  | 已完成             | `puf2g-node-panel-link-entry/SPEC.md`                   | 2026-03-02 | PR #87                                                                                                                                                                                                                                                                                                                                            |
| nujzm | Grant groups 硬切下线与 Access 模型迁移    | 待实现             | `nujzm-access-model-hardcut-v9/SPEC.md`                 | 2026-02-27 | fast-track                                                                                                                                                                                                                                                                                                                                        |
| s2w9k | Quota Policy 节点比例可视化编辑 + 映射替换 | 已完成             | `s2w9k-quota-policy-node-ratio-editor/SPEC.md`          | 2026-03-03 | PR #81 + responsive/table UX follow-up（双击内联输入 + 视口断点表格）                                                                                                                                                                                                                                                                             |
| 9vmap | 节点服务可观测升级                         | 已完成             | `9vmap-node-service-observability/SPEC.md`              | 2026-04-24 | local implementation + tests passed；NodeDetails metadata 补充 egress probe 摘要/刷新与视觉证据                                                                                                                                                                                                                                                   |
| r7m2q | OpenRC 小内存运行时快速自愈                | 已完成             | `r7m2q-openrc-low-memory-runtime-recovery/SPEC.md`      | 2026-05-15 | 256MB no-swap host target；xray fast first restart + exponential backoff；cloudflared monitor-only default；health restart/backoff fields                                                                                                                                                                                                         |
| nbs5f | Xray 控制面 Relay 组网                     | superseded(#56dtr) | `nbs5f-xray-control-plane-relay/SPEC.md`                | 2026-08-03 | 已由 Reality Mesh 取代；保留 XP_MESH_PROXY_URL 公网出站兼容语义                                                                                                                                                                                                                                                                                   |
| nq4ha | Web 当前节点原地升级入口                   | 已完成             | `nq4ha-web-local-upgrade/SPEC.md`                       | 2026-07-30 | fast-track；OpenRC root-only doas policy readiness helper + fixed start delegation                                                                                                                                                                                                                                                                |
| 7mvqp | Raft membership voter invariant            | 已完成             | `7mvqp-raft-membership-voter-invariant/SPEC.md`         | 2026-07-07 | fast-track；稳定 membership 节点全部为 voter；join 成功即 voter；leader-side guard 修复遗留 learner；无 quorum 仅显式灾难恢复                                                                                                                                                                                                                     |
| 7qj3h | Web PWA 化与离线状态控制台                 | 已完成             | `7qj3h-web-pwa-offline-console/SPEC.md`                 | 2026-07-30 | fast-track；installable PWA shell + IndexedDB read cache + admin aggregated status SSE + offline read-only UX + 401 re-authentication recovery                                                                                                                                                                                                    |
| m4n7c | 节点 TCP 连接数面板                        | 已完成             | `m4n7c-node-tcp-connection-count/SPEC.md`               | 2026-06-21 | fast-track；Linux-only，本机 socket 视角；Node details 独立 tab；本地 7d 分钟级历史 + endpoint 多选聚合                                                                                                                                                                                                                                           |
| yzqn6 | Remove grant groups hard cut               | 已完成             | `yzqn6-remove-grant-groups-hard-cut/SPEC.md`            | 2026-02-27 | PR #84                                                                                                                                                                                                                                                                                                                                            |
| wvrmn | Remove grants hard cut                     | 已完成             | `wvrmn-remove-grants-hard-cut/SPEC.md`                  | 2026-03-02 | PR #86 + PR #90 AccessMatrix post-fix（含树勾选状态/单端点元数据修正）                                                                                                                                                                                                                                                                            |
| e5sqd | 前端 Tools 入口 + Mihomo 订阅脱敏          | 已完成             | `e5sqd-web-tools-mihomo-redact/SPEC.md`                 | 2026-03-19 | PR #114；fast-track，停在 merge-ready                                                                                                                                                                                                                                                                                                             |
| c8qtw | 单 Docker 镜像集群节点部署                 | 已完成             | `c8qtw-docker-single-image-cluster-node-deploy/SPEC.md` | 2026-08-04 | fast-track；single-image Docker + GHCR multi-arch + optional Cloudflare Tunnel；default endpoint env 仅用于缺失 endpoint bootstrap                                                                                                                                                                                                                |
| vks2a | Engineering style gates                    | 待实现             | `vks2a-engineering-style-gates/SPEC.md`                 | 2026-06-28 | strict style-budget checker；real rustfmt；Biome/hook/CI enforcement                                                                                                                                                                                                                                                                              |
| 38wmj | 集群节点部署与管理员凭据一致性             | 已完成             | `38wmj-cluster-node-onboarding/SPEC.md`                 | 2026-07-29 | 从 legacy plan 迁移；集群共享 Argon2id PHC、bootstrap/join 同步与无明文落盘                                                                                                                                                                                                                                                                       |
| k9n2r | xp-ops 管理员 Token 运维                   | 已完成             | `k9n2r-xp-ops-admin-token-ops/SPEC.md`                  | 2026-07-29 | 从 legacy plan 迁移；shell-safe PHC 写入、显式重启与跨节点同步                                                                                                                                                                                                                                                                                    |
| nmgq8 | 完整托管栈 64 MiB 内存预算                 | 待实现             | `nmgq8-managed-stack-64m-memory/SPEC.md`                | 2026-07-29 | fast-track；xp+xray+cloudflared+canary PSS 硬门禁与低内存管理员鉴权                                                                                                                                                                                                                                                                               |
| r26nc | 节点与用户 Traffic 统计                    | 已完成             | `r26nc-node-user-traffic-analytics/SPEC.md`             | 2026-07-28 | 49h five-minute + 90d UTC daily rollups；Node/User Traffic tabs；partial-aware comparison charts                                                                                                                                                                                                                                                  |
| 56dtr | Reality Mesh                               | active             | `56dtr-reality-fallback-control-plane-mesh/SPEC.md`     | 2026-08-03 |                                                                                                                                                                                                                                                                                                                                                   |

## Slug-only topics（新规格 canonical catalog）

- `Lifecycle`: `draft|active|superseded|retired`
- `Implementation`: 自由文本的 current truth，例如 `未开始`、`部分完成`、`已完成`

| Topic                | Lifecycle | Implementation  | Spec topic                   | Successor  |
| -------------------- | --------- | --------------- | ---------------------------- | ---------- |
| Web运行时韧性        | active    | 部分完成（1/2） | `web-runtime-resilience`     | #227 child |
| Mihomo SMux          | active    | complete        | `endpoint-mihomo-smux`       | -          |
| XHTTP/XMUX           | active    | complete        | `endpoint-vless-xhttp-reuse` | -          |
| Fixture 数据策略门禁 | active    | 已实现          | `fixture-policy-gate`        | -          |
| Web 滚动容器         | active    | 组件基线已明确  | `scroll-containers`          | -          |
| 集群长期历史数据仓库 | active    | bootstrap       | cluster-history-repositories | -          |
