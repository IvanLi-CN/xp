# 集群节点部署引导（Web 生成命令 + xp-ops deploy join）（#38wmj）

## 状态

- Status: 待实现
- Created: 2026-01-27
- Last: 2026-01-27

## 冻结结论（Decisions frozen）

- Web 引导命令必须覆盖“下载安装到可运行”为止（下载 `xp`/`xp-ops` 并启服务）。
- join token 允许以明文出现在可复制命令中。
- Web 下载/版本策略：跟随 leader 当前运行版本（通过 `GET /api/cluster/info` 暴露 `xp_version`）。
- `XP_ADMIN_TOKEN` 模型：服务端与磁盘不存明文，仅存 `XP_ADMIN_TOKEN_HASH`（Argon2id PHC，默认参数 `m=65536,t=3,p=1`）。
- bootstrap：`xp-ops deploy` 自动生成 admin token，明文仅打印一次供管理员保存；丢失后通过“轮换 token”恢复（不在本计划范围）。

## 背景 / 问题陈述

- 当前“新增节点加入集群”的操作链路分散：Web 端只能生成 `xp join --token ...`，但“安装/初始化/写入配置/自启动/加入集群”需要人工拼装命令，容易出错。
- `XP_ADMIN_TOKEN` 目前由 `xp-ops deploy` 在本机缺失时随机生成（见 `src/ops/deploy.rs`），导致多节点部署后 token 可能不一致，进而出现跨节点访问时“需要重复登录/部分管理能力不可用”的体验问题。
- 语义澄清：`XP_ADMIN_TOKEN` 是 `/api/admin/*` 的 Bearer token（相当于“管理员共享密钥”），不是“最终用户身份系统”；Web UI 的“登录”仅是在浏览器端保存该 token。
- 安全目标：服务端不再保存 `XP_ADMIN_TOKEN` 明文，仅保存其 hash；加入新节点时同步 hash（而非同步明文），从而同时满足“集群一致”与“服务端不落原文”。

## 目标 / 非目标

### Goals

- 在 Web UI 提供“部署/加入集群节点”的引导入口：可生成**直接可用**的命令（可复制粘贴执行），用于把新机器部署成集群成员。
- `xp-ops deploy` 扩展支持“加入集群（join）模式”：支持非交互执行，并能把新节点写入与集群一致的 `XP_ADMIN_TOKEN`（不再默认生成一个可能不一致的 token）。
- 管理员 token 采用“服务端存 hash、请求端提交明文、服务端 hash 比较”的模型；服务端与磁盘不落原文，且 bootstrap 时 token 仅打印一次供管理员保存。
  - Hash 算法（v1）：Argon2id（PHC string 存储；见 `contracts/file-formats.md`）。

### Non-goals

- 不引入外部 Secret 管理系统（Vault/KMS/SSO 等）或复杂的权限体系。
- 不在本计划中解决“多租户/多角色 RBAC”。
- 不在本计划中提供“忘记 admin token 的找回能力”；若管理员丢失 token，默认通过“轮换 token”恢复（轮换工具能力可作为后续计划）。
- 不把 `docs/plan/` 中的任何资产作为实现产物的运行/交付依赖。

## 范围（Scope）

### In scope

- Web UI：新增“部署节点引导”交互，输出可复制命令（包含 join token、以及可选的 xp-ops 方案）。
- `xp-ops deploy`：新增/调整 CLI 以支持“Join 流程”与“安全地同步 admin token”（见契约）。
- 明确并文档化“集群范围内 `XP_ADMIN_TOKEN` 一致性”的约束与推荐实践。
- 版本策略：Web 引导生成的下载命令应跟随 leader 当前运行版本（不默认 latest）。

### Out of scope

- 为所有 OS/发行版提供 100% 覆盖的一键安装脚本（除非在开放问题中确认目标集合）。
- 自动化托管“远程 ssh 执行”（由 Web 直接登录目标机器执行命令）。

## 需求（Requirements）

### MUST

- Web UI 必须提供入口（建议：`Nodes` 页增强）用于生成“新增节点加入集群”的命令指引：
  - 能生成 join token（现有能力：`POST /api/admin/cluster/join-tokens`）。
  - 能生成完整的复制粘贴命令串（必须覆盖“下载安装到可运行”为止：下载 `xp-ops` + `xp` → `xp-ops deploy` join 模式 → 启用服务）。
  - 对包含敏感信息的片段（token）必须有明显提示，并优先提供“copy”而非默认长期展示。
  - join token 允许以明文出现在可复制命令中（由管理员自行控制转发范围与 TTL）。
- `xp-ops deploy` 必须提供**可脚本化**的 join 能力（非交互可用），并保证新节点的 `XP_ADMIN_TOKEN` 与集群一致：
  - 增加 join 模式（例如 `--join-token` / `--join-token-stdin`）。
  - join 模式下不得要求用户手工输入/粘贴 admin token；必须由程序自动、安全地获取并写入（见接口契约）。
  - 当目标机已有 `/etc/xp/xp.env` 且 `XP_ADMIN_TOKEN` 与输入不一致时，默认必须失败并提示 `--overwrite-existing`（避免静默形成不一致）。
- 所有实现不得把 token 写入普通日志；错误信息需可操作（指出缺失项与建议命令行参数）。
- bootstrap 节点首次部署时，`xp-ops deploy` 必须自动生成一个随机 admin token，并且：
  - 只在终端 **打印一次** 明文 token（用于管理员保存与登录 Web）
  - 仅将其 hash 落盘（`/etc/xp/xp.env`），服务端不得保存明文

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                      | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）    | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                         |
| ------------------------------------------------- | ------------ | ------------- | -------------- | --------------------------- | --------------- | ------------------- | ------------------------------------- |
| `xp-ops deploy` join mode (auto admin token sync) | CLI          | internal      | Modify         | ./contracts/cli.md          | ops             | 管理员、Web 指引    | join 流程与 token 同步                |
| `POST /api/cluster/join` response schema          | HTTP API     | internal      | Modify         | ./contracts/http-apis.md    | server          | xp / xp-ops         | join 过程下发集群 admin token         |
| `/etc/xp/xp.env` (`XP_ADMIN_TOKEN_HASH`)          | File format  | internal      | Modify         | ./contracts/file-formats.md | ops/server      | xp / xp-ops         | 服务端不落原文，仅存 hash             |
| `GET /api/cluster/info` response schema           | HTTP API     | internal      | Modify         | ./contracts/http-apis.md    | server          | Web UI              | 增加 `xp_version` 供 Web 生成下载命令 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/file-formats.md](./contracts/file-formats.md)

## 验收标准（Acceptance Criteria）

- Given 管理员已能正常访问 Web UI，
  When 打开 Nodes 页面并进入“新增节点引导”，
  Then UI 能生成 join token，并展示至少一套“可直接执行”的命令（无需手工补 admin token/URL 参数）。
- Given 新部署的 bootstrap 节点已完成 `xp-ops deploy`，
  When 管理员首次打开 Web UI 登录，
  Then 只需使用部署时打印一次的 token 即可登录，且服务端/磁盘不保存明文 token。
- Given 一台全新 Linux 机器（在支持的 OS/arch 范围内）可执行生成的命令，
  When 按引导执行完成，
  Then 新节点出现在 Nodes inventory 中，且 `xp` 服务处于可运行状态（`xp-ops status` 或等价检查通过）。
- Given 集群已有多个节点，
  When 使用同一个 admin token 访问任一节点 UI，
  Then 不应出现“切节点需要重新登录/部分管理接口因 token 不一致失败”的情况。
- Given 执行过程中发生缺参/不兼容/权限不足，
  When `xp-ops` 退出，
  Then exit code 非 0 且错误信息包含明确的修复建议（例如提示使用 `--join-token-stdin` / `--admin-token-stdin` / `--overwrite-existing`）。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标/非目标、范围（in/out）、约束已明确
- 验收标准覆盖 core path + 关键边界/异常
- `contracts/cli.md` 与 `contracts/http-apis.md` 已定稿（实现与测试可直接按契约落地）
- 已确认：Web 引导命令版本策略为“跟随 leader 当前版本”
- 已确认：bootstrap 节点 admin token 采用“自动生成 + 只打印一次”策略

## 支持平台（Support matrix, v1）

（复用项目既有约束：以 `xp-ops` 当前实现与既有计划 #0013 的平台约定为准。）

- CPU arch: `x86_64`, `aarch64`
- Linux distro families: Debian/Ubuntu, Alpine, Arch/Arch ARM

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust unit tests: 覆盖 join 参数解析、stdin 输入、token 不一致检测、非交互错误分支。
- Web unit tests (Vitest): 覆盖 Nodes 引导的 command 生成（含边界：缺 token/缺字段/TTL 非法）。
- E2E (Playwright, optional): 覆盖 UI 生成 join token + copy 行为（不要求真实部署远端节点）。

### Quality checks

- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
- Web: `cd web && bun run lint`, `cd web && bun run typecheck`, `cd web && bun run test`

## 文档更新（Docs to Update）

- `docs/desgin/workflows.md`: 增加“Web 生成命令 → 目标机执行 xp-ops join → 节点注册完成”的推荐流程
- `docs/desgin/architecture.md`: 明确 `XP_ADMIN_TOKEN` 的语义与“集群范围一致性”约束
- `docs/desgin/api.md`: 若新增/调整任何 API（目前计划优先不改 API），同步更新接口说明

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [ ] M1: `xp-ops deploy` join 模式落地（含 stdin + overwrite 语义）并补齐单测
- [ ] M2: Web UI 节点引导生成命令（含边界与错误态）并补齐单测
- [ ] M3: 文档更新 + 回归验证清单（QA checklist）

## 方案概述（Approach, high-level）

- Web 端复用现有 join token 生成接口，并在前端基于已登录的 admin token + 节点输入项拼装“可复制的命令块”（覆盖“下载安装到可运行”为止”）。
- `xp-ops deploy` 增加“join 模式编排”：复用现有 install/init/enable 逻辑，将 `xp bootstrap (xp init)` 替换为“执行 join 并完成落盘”，并在写入 `/etc/xp/xp.env` 时自动同步 leader 的 `XP_ADMIN_TOKEN`（命令行不暴露明文）。
- 默认以“避免泄漏与避免隐式不一致”为原则：stdin 优先、错误提示清晰、默认不覆写现有 token。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：join token 属于敏感数据；Web/CLI 展示与复制必须减少泄漏面（剪贴板、截图、终端 history）。
- 风险：将 admin token（或其 hash）下发给 joiner 会扩大“join token 泄漏”的影响面（泄漏者可加入节点并获取更多集群 secret/配置）。
  - 但现状 join 响应已包含 `cluster_ca_key_pem`（更高敏感度），因此本计划在风险层面属于“同一信任边界内的必要同步”。
  - 缓解：join token 必须短 TTL + 一次性；`xp-ops`/服务端禁止明文日志；必要时提供集群级 token 轮换（不属于本计划范围）。
- 风险：bootstrap 时 token 仅打印一次；若管理员未妥善保存，后续需要通过“轮换 token”恢复访问（需要明确运维 SOP）。
- 风险：目标平台差异导致“一键命令”难以通用，需要先冻结支持矩阵。
- 假设：集群内所有节点应共享同一个 `XP_ADMIN_TOKEN`（用于统一 admin API 访问体验与集群级管理能力）。

## 变更记录（Change log）

- 2026-01-27: 创建计划，等待口径冻结

## 参考（References）

- `web/src/views/NodesPage.tsx`：现有 join token 生成与 `xp join` 命令展示
- `src/main.rs`：`xp join --token` 行为
- `src/ops/deploy.rs`：`XP_ADMIN_TOKEN` 缺失时的随机生成逻辑
- `src/http/mod.rs`：`POST /api/admin/cluster/join-tokens`、`POST /api/cluster/join`
