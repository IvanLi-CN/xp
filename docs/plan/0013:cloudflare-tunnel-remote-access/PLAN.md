# Cloudflare Tunnel 公网访问与互联（API 自动创建 + 应用路由）（#0013）

## 状态

- Status: 待实现
- Created: 2026-01-15
- Last: 2026-01-16

## 1) 问题陈述

目标服务器需要一种“无需公网 IP / 无需开放入站端口”的方式，让管理员可以在公网访问其 Web 管理界面，并让其他机器能够稳定访问该服务器对外提供的接口（例如：节点/客户端访问 API）。

术语：本计划中“目标服务器”指运维程序 `xp-ops` 实际运行的那台机器；`xp-ops` 在该机器上完成 `xp`、`xray` 以及可选的 `cloudflared` 的安装、配置与开机自启。

Cloudflare Tunnel 的创建、路由与 DNS 配置**全部由单独的运维 CLI（root 运行）完成**（不通过 Web 管理端配置）。运维 CLI 通过 **Cloudflare API** 自动完成：

- 创建 Tunnel
- 配置“已发布应用程序路由”（public hostname → origin service）
- 创建 DNS 记录（CNAME → `<tunnel-id>.cfargotunnel.com`）

启用后由目标服务器端运行 `cloudflared`，并提供可观测的运行状态与错误信息。运维 CLI（`xp-ops`）按约定以 **root** 运行：负责安装依赖（`cloudflared`、`xray` 等）、初始化本机配置，并确保组件开机自动运行（systemd/OpenRC）。

## 2) 目标 / 非目标

### Goals

- 提供一个运维 CLI（root 运行），在 Arch Linux / Arch Linux ARM / Debian / Alpine 上：
  - 安装并部署 `xp`、`xray`，以及可选的 `cloudflared`
  - 调用 Cloudflare API：创建 tunnel、配置应用路由（ingress）、创建/校验 DNS
  - 初始化 `XP_DATA_DIR`、权限，并完成 `xp` 的**首次引导配置**（节点信息、token、对外访问地址）
  - 注册并启用开机自启（Arch/Debian: systemd；Alpine: OpenRC）
- 目标服务器上的 `xp` 以普通用户身份运行；`xp` 由 `xp-ops` 安装部署；Cloudflare 集成不通过 Web 管理端配置。
- 公网暴露范围为“全部”（Web UI + 目标服务器对外 API）：对外 hostname 的 `path="*"` 转发到 `origin_url`。

### Non-goals

- 不在本计划阶段修改业务源码或运行配置。
- 不在本计划中定义 `xp` 二进制的分发/发布渠道（v1 通过 `xp-ops xp install --xp-bin <path>` 由管理员提供）。
- 不考虑 Cloudflare WARP/Private Network 等私网路由能力（本计划仅覆盖“公网访问 Web/UI/API”）。

## 3) 用户与场景

- **主要用户**：目标服务器管理员 / 运维（具备 root 或等价权限，可运行运维 CLI）。
- **典型场景**
  - 在家用网络/内网目标服务器部署 `xp`，但需要从公网访问 Web UI。
  - 其他机器（节点、运维机器、远程客户端）需要访问目标服务器的外部 API，而服务器不具备可用的公网入站能力。
  - 管理员通过运维 CLI 一键初始化与启用 tunnel，并能定位失败原因（Cloudflare API 权限、DNS 写入失败、网络受限等）。

## 4) 需求列表（MUST/SHOULD/COULD）

### MUST

- 不新增/不修改任何 Web 管理端页面用于 Cloudflare 配置。
- `xp-ops` 必须在目标服务器上完成 `xp` 与 `xray` 的安装部署，并确保两者默认开机自启。
- `xp-ops` 必须为 `xp` 生成/写入初始化配置，并在启动 `xp` 前完成一次性引导（bootstrap）：
  - 节点信息：`node_name`、`public_domain`（用于订阅/节点访问域名）
  - 管理 token：`XP_ADMIN_TOKEN`（生成/写入，默认启用鉴权）
  - 对外访问地址：`api_base_url`（用于 API/Web UI/节点互访；必须为 `https://...` 的完整 origin）
  - 首次启动前调用 `xp init` 写入 `XP_DATA_DIR/cluster/*`（cluster metadata + CA/证书）；后续启动 `xp run` 以该元数据为准
- Cloudflare Tunnel 为选配功能：
  - 未启用时：不安装/不配置/不自启动 `cloudflared`，不要求 Cloudflare 侧资源存在。
  - 启用时：按需安装/配置/自启动 `cloudflared`，并完成 Cloudflare API provisioning。
- 运维 CLI 提供完整的“安装 + 初始化 + 启用自启动”能力：
  - 输入/保存 `account_id`、`zone_id`、`hostname`、`origin_url`
  - 输入 Cloudflare API token（不回显、不写入日志；**允许**以严格权限落盘保存，便于重复执行）
  - 调用 Cloudflare API 创建 tunnel、写 ingress、创建/校验 DNS
  - 获取并写入 tunnel `credentials_file`（落盘为 `/etc/cloudflared/<tunnel-id>.json`），并生成 `cloudflared` config（`/etc/cloudflared/config.yml`）供常驻运行
- 运维程序提供交互式 TUI（Terminal UI）：
  - 支持通过向导完成“一键部署”（安装依赖 → 初始化目录 → 写入/生成 xp 配置 → xp bootstrap（xp init）→ Cloudflare provision（可选）→ 启用自启动 → 启动服务）
- TUI 内输入 token 必须默认隐藏（mask），不得写日志；允许默认落盘保存（并在界面上提供关闭“保存 token”的选项）
- 运维 CLI 同时支持“无交互（non-interactive）”模式：
  - 所有关键操作都必须有等价的子命令（见 `contracts/cli.md`），可在 CI/脚本中直接调用
  - 无交互模式下不得弹出任何提示；需要分支行为必须以显式 flag 表达（例如 `--disallow-additional-repos`、`--enable/--no-enable`、`--dry-run`）
- 启用流程（对齐 Cloudflare Dashboard 的“已发布应用程序路由”）：
  - 通过 API 创建 tunnel（得到 `tunnel_id` 与 `credentials_file`/`token`）
  - 通过 API 配置 ingress（hostname → origin service）
  - 通过 API 创建 DNS 记录（CNAME `hostname` → `${tunnel_id}.cfargotunnel.com`，`proxied=true`）
    - 注：Dashboard 中看起来“自动创建 DNS”，本质也是为该 hostname 写入 DNS 记录；API 集成需要显式（即通过 Cloudflare DNS API）创建/校验该记录。
  - 当 Cloudflare Tunnel 启用时：
    - `hostname` MUST 用作 `xp` 的对外访问域名：`xp api_base_url = https://<hostname>`（默认 443；不得携带自定义端口）
    - Web UI 与 API 均通过 `https://<hostname>/` 对外提供
- 运维 CLI 需要确保组件开机自动运行：
  - Arch/Debian：systemd unit（`systemctl enable --now ...`）
  - Alpine：OpenRC service（`rc-update add ...`，并能启动）
- 失败可诊断：
  - token 缺失/无效、下载失败、权限不足、网络出口受限等，必须以可理解的错误码/文本输出（并可选提供 `--json` 机器可读输出）
  - 错误信息需包含下一步建议（例如需要配置 token、需要开放出站端口、需要启用额外仓库等）
- token 安全：
  - token 不允许通过命令行参数传递（避免出现在 shell history/`ps`）；推荐仅用环境变量或 root-only token 文件（见 `contracts/cli.md`、`contracts/config.md`）
  - 运维 CLI 输出与日志中不得打印 token（包括错误回显中的敏感片段）

### SHOULD

- 安装来源（已在计划阶段冻结；见 `contracts/cli.md`）：
  - `cloudflared`：
    - Arch：发行版官方仓库（`pacman`）
    - Debian：Cloudflare 官方 APT 仓库（`pkg.cloudflare.com`，dist=`any`）
    - Alpine：Cloudflare 官方 GitHub Releases（预编译二进制下载）
  - `xray`：Project X 官方 GitHub Releases（zip 下载）
- 运维 CLI 提供 `status`（以及可选 `logs`）等诊断能力。
- TUI 与非交互命令复用同一套业务逻辑（避免两套实现口径漂移）。
- TUI 默认将 Cloudflare Tunnel 标记为“启用”（可在向导中手动关闭），以符合“选配但默认启用”的交互预期。

### COULD

- `xp-ops logs`：汇总并脱敏展示最近 N 条关键日志（仅供排障）。

## 5) 接口清单与契约（Inputs/Outputs/Errors）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Cloudflare Tunnel 本地配置与密钥存储 | Config | internal | New | ./contracts/config.md | ops | xp-ops、cloudflared | 需明确路径/权限/脱敏 |
| Cloudflare REST API（tunnel/config/dns） | HTTP API | external | New | ./contracts/http-apis.md | cloudflare | ops | 账号/域名需已接入 Cloudflare |
| 运维 CLI（`xp-ops`） | CLI | internal | New | ./contracts/cli.md | ops | 管理员 | root 运行，用于安装/初始化 |
| `xp` 运行配置（env file） | File format | internal | New | ./contracts/file-formats.md | ops | xp-ops、xp | `/etc/xp/xp.env`（含 `XP_ADMIN_TOKEN` 等） |
| 自启动服务文件（systemd/OpenRC） | File format | internal | New | ./contracts/file-formats.md | ops | ops | unit/service 文件落盘约定 |

## 6) 约束与风险

- **root 权限要求**：`xp-ops` 以 root 运行并写入 `/etc/*`、创建 system user、注册系统服务；若目标服务器不提供 root/等价权限，则本计划范围内的“安装/初始化/自启动”无法落地（可仅支持 `--dry-run` 预演与错误提示）。
- **网络出口限制**：Cloudflare Tunnel 依赖出站连接；若服务器环境限制特定端口/协议，需要在部署手册与错误提示中明确。
- **安全风险**：token 属于敏感凭据；存储、日志、故障上报必须严格脱敏；同时需要考虑 token 轮换与撤销。
- **跨平台差异**：systemd（Arch/Debian）与 OpenRC（Alpine）的服务管理语义不同，需要抽象并覆盖自动化测试。
- **证书与子域限制**：若使用多级子域名（多于一层 subdomain），可能需要额外证书能力；需要在产品限制与错误提示中覆盖。
- **安装来源已冻结（重要）**：
  - `cloudflared`：
    - Arch Linux / Arch Linux ARM：发行版官方仓库（`pacman`）安装 `cloudflared`
    - Debian：Cloudflare 官方 APT 仓库（`pkg.cloudflare.com`，dist=`any`）安装 `cloudflared`
    - Alpine：Cloudflare 官方 GitHub Releases（预编译二进制下载）安装 `cloudflared`（不使用 `apk edge/testing`）
  - `xray`：Project X 官方 GitHub Releases（zip 下载）安装到 `/usr/local/bin/xray`
  - 若网络出口无法访问上述下载源/仓库，运维 CLI 必须失败并提示可执行的解决办法（代理、镜像、离线预下载等）。
- **TUI 可用性与安全**：
  - 终端环境差异（TERM、ssh、中文宽度、鼠标事件）会影响显示；需要将 TUI 作为“加速器”，但保留等价的非交互 CLI。
  - token 输入必须避免出现在 shell history、进程列表与日志中（TUI 与 CLI 都要满足）。

## 技术选型（Tech choices）

- 语言：Rust
- CLI 参数解析：`clap`（derive 模式）
- TUI：`ratatui` + `crossterm`
- Cloudflare HTTP client：`reqwest`（配合 `tokio`）
- 测试（建议）：`wiremock`（HTTP mock）、`assert_cmd`（CLI 子进程）、`tempfile`（临时目录）

## 7) 验收标准（Acceptance Criteria）

### 运维 CLI（核心）

- Given 目标系统为 Arch Linux / Arch Linux ARM / Debian / Alpine，
  When 以 root 运行 `xp-ops install`，
  Then `xray` 安装成功并可执行（可通过 `xray version` 或 `xray -version` 验证）。

- Given 管理员提供 `xp` 可执行文件（见 `contracts/cli.md`），
  When 以 root 运行 `xp-ops xp install`，
  Then `xp` 安装部署成功并可执行（可通过 `xp --help` 或 `--version` 验证）。

- Given 管理员提供 `account_id/zone_id/hostname/origin_url` 与有效 Cloudflare API token，
  When 以 root 运行 `xp-ops cloudflare provision`（命令名以 `contracts/cli.md` 为准），
  Then 系统创建 tunnel、写 ingress、创建/校验 DNS，
  And 在本机写入 `/etc/cloudflared/<tunnel-id>.json` 与 `/etc/cloudflared/config.yml`（最小权限），
  And 不在任何输出/日志中泄露 token 明文。

- Given 管理员提供 `node_name/public_domain` 与（启用 tunnel 时）`hostname`，
  When 以 root 运行 `xp-ops deploy`（命令名以 `contracts/cli.md` 为准），
  Then `xp-ops` 必须在启动 `xp` 前完成一次性引导（`xp init`），并写入：
  And `/etc/xp/xp.env`（包含非空 `XP_ADMIN_TOKEN`，并被 `xp` 服务加载），
  And `XP_DATA_DIR/cluster/metadata.json` 中的 `api_base_url` 必须为 `https://<hostname>`（默认 443；不得携带自定义端口），
  And `XP_DATA_DIR/cluster/metadata.json` 中的 `node_name/public_domain` 与输入一致。

- Given 已完成 provision 且服务已启用，
  When 重启机器，
  Then `xp` 与 `xray` 自动启动并保持运行（可通过 `xp-ops status` 验证），
  And 若 Cloudflare Tunnel 已启用，则 `cloudflared` 也自动启动并保持运行。

### 公网可达性（核心）

- Given provision 已完成且 `cloudflared` 正常运行，
  When 在公网访问 `https://<hostname>/`，
  Then Web UI 可正常加载（或返回明确的错误页，不出现连接超时/解析失败）。

- Given provision 已完成且 `cloudflared` 正常运行，
  When 在公网访问 `https://<hostname>/api/...`（目标服务器对外 API），
  Then API 可到达（按既有鉴权返回 `401/403` 或业务响应），不出现连接超时/解析失败。

### 异常与边界

- Given 未提供 Cloudflare API token，
  When 运行 provision 命令，
  Then 运维 CLI 失败并给出可理解错误（不泄露敏感信息）。

- Given 系统内存在包管理器但仓库中无对应包，
  When 运维 CLI 尝试安装依赖，
  Then 运维 CLI 必须失败并给出明确提示（例如“检查软件源/镜像配置、检查网络、确认发行版与架构是否受支持”），不进入静默失败。

- Given 目标系统为 Arch Linux，
  When 运行 `xp-ops install --only cloudflared`，
  Then 运维 CLI 必须使用发行版官方仓库通过 `pacman` 安装 `cloudflared`。

- Given 目标系统为 Debian，
  When 运行 `xp-ops install --only cloudflared`，
  Then 运维 CLI 必须配置并使用 Cloudflare 官方 APT 仓库（`pkg.cloudflare.com`，dist=`any`）安装 `cloudflared`。

- Given 目标系统为 Alpine，
  When 运行 `xp-ops install --only cloudflared`，
  Then 运维 CLI 必须从 Cloudflare 官方 GitHub Releases 下载并安装 `cloudflared`（不得使用 `apk edge/testing`）。

- Given 目标服务器网络无法访问 Project X 官方 GitHub Releases（下载失败/超时/被拒绝），
  When 运行 `xp-ops install --only xray`，
  Then 运维 CLI 必须失败并给出可执行提示（例如检查网络/代理环境变量、或改用可达的下载镜像/手工预下载）。

## 自动化测试要求（必须）

所有关键操作必须具备自动化测试（CI/本地可重复运行），并满足：

- 默认不依赖真实 Cloudflare 账号与真实 DNS 生效（不使用真实环境）
- 覆盖关键路径与失败路径（至少：token 缺失、Cloudflare 4xx/5xx、网络超时、JSON 异常、文件权限错误、外部命令失败）
- 对“需要 root/系统服务/包管理器”的动作提供测试友好的执行层（例如 `--dry-run` 或可注入的 `CommandRunner`/`HttpClient`），使测试可在非 root、无 systemd/openrc 的 CI 环境运行

### 必测项（最小集合）

- **TUI 与无交互一致性**：TUI 向导每一步必须调用同一套内部 actions；对 actions 做单元测试覆盖成功/失败路径。
- **无交互子命令**：通过子进程方式断言 exit code/stdout/stderr；至少覆盖：
  - `xp-ops install`（dry-run + 真实命令拼装）
  - `xp-ops init`（生成目录/权限/服务文件的 dry-run）
  - `xp-ops xp install`（dry-run + 真实命令拼装）
  - `xp-ops deploy`（dry-run + 关键步骤编排）
  - `xp-ops cloudflare provision`（使用 mock server；见下文）
  - `xp-ops status`
- **文件生成幂等**：对 `settings.json`、`/etc/cloudflared/config.yml`、`/etc/cloudflared/<tunnel-id>.json`、`/etc/xp/xp.env`、systemd/OpenRC 文件生成做快照/结构化断言（重复执行输出一致）。

### Cloudflare 自动化测试（不使用真实环境）的可行性与边界

可自动化（mock 覆盖）：

- 对 Cloudflare API 的 HTTP 请求形状（method/path/headers/body）与错误处理（4xx/5xx/超时/坏 JSON）
- `tunnel_id`/`credentials_file`/`dns_record_id` 的解析与持久化策略

可自动化（不依赖真实账号/zone 的“本地验证”）：

- 使用真实 `cloudflared` 二进制对 ingress 规则做离线校验：
  - `cloudflared tunnel ingress validate`：验证生成的 ingress 规则语法与结构
  - `cloudflared tunnel ingress rule <url>`：对给定 URL 验证将匹配到的 service（用于断言 `https://<hostname>/...` 会被转发到 `origin_url`）
  - 说明：该验证不需要 Cloudflare 账号/zone，也不需要跑起 tunnel；但要求测试环境可执行 `cloudflared`（实现阶段需给出 CI 里“获取/固定版本”的方案）。

仍无法在“不使用真实 Cloudflare 账号/zone”的前提下做完全端到端验证：

- DNS 记录在公网的实际解析与传播（propagation）
- tunnel 的真实连通性与流量转发（从公网访问 `https://<hostname>/` 到本机 `origin_url`）

本计划要求：实现阶段必须同时交付：

- **自动化**：API mock + 离线 ingress 校验（覆盖核心配置正确性）
- **可选的集成测试入口**：仅在提供真实凭据/环境变量时运行（默认跳过，避免 CI 依赖真实环境）
- **手工 smoke test 清单**：少量命令 + 预期结果（用于真实环境最终验收）

## 8) 开放问题（需要主人回答）

- None

## 9) 已确认事项（来自主人决策）

- 运维程序为 `xp-ops`（Rust），同时支持 TUI 与无交互模式；两者必须复用同一套 actions。
- 目标服务器为 `xp-ops` 实际运行的机器：`xp-ops` 在该机器上安装部署 `xp`、`xray`，并管理其开机自启。
- 目标发行版必须支持：Arch Linux / Arch Linux ARM / Debian / Alpine。
- Cloudflare Tunnel 为选配：未启用时不安装/不自启动 `cloudflared`；启用时通过 Cloudflare API 创建 tunnel + ingress + DNS，并启用 `cloudflared` 自启动。
- 只考虑“单目标服务器 + 单 hostname + path='*' + 暴露范围为全部”的情况；不考虑 Cloudflare Private Network/WARP 等私网路由。
- `xray` 配置由 `xp-ops` 生成：`/etc/xray/config.json` 最小可用模板（不擅自开放任何入站）。
- Cloudflare API token **允许**以严格权限落盘保存（便于重复执行）；TUI/CLI 输入后可默认保存到 `/etc/xp-ops/cloudflare_tunnel/api_token`。
- 目标服务器 CPU 架构仅需支持：`x86_64`、`aarch64`。
- Arch Linux / Arch Linux ARM：`cloudflared` 默认通过发行版官方包管理器安装（`pacman`）。
- 安装策略总则：
  - 本计划在计划阶段已**冻结安装来源**（避免实现阶段再做版本新鲜度比较）：
    - `cloudflared`：
      - Arch：发行版官方仓库（`pacman`）
      - Debian：Cloudflare 官方 APT 仓库（`pkg.cloudflare.com`，dist=`any`）
      - Alpine：Cloudflare 官方 GitHub Releases（预编译二进制下载）
    - `xray`：Project X 官方 GitHub Releases（zip 下载）
- Cloudflare：由 `xp-ops` 通过 Cloudflare DNS API 创建/更新 DNS 记录；不要求管理员在 Dashboard 手工操作。
- Cloudflare：允许有独立命令（例如 `xp-ops cloudflare provision`），同时要求“一键部署”（TUI/非交互）能丝滑完成整套流程（以快速部署成功为准）。

## 非功能性验收 / 质量门槛（Quality Gates）

实现阶段完成后，至少运行（沿用仓库现有约定，不引入新工具）：

- 后端（xp）：
  - `cargo fmt`
  - `cargo clippy -- -D warnings`
  - `cargo test`
- 运维 CLI（xp-ops）：
  - `cargo fmt`
  - `cargo clippy -- -D warnings`
  - `cargo test`（必须包含：mock Cloudflare、dry-run、文件生成幂等、TUI actions 单测、子进程 CLI 测试）

## 文档更新（Docs to Update）

- `docs/ops/` 下新增/补充“Cloudflare Tunnel 部署说明”（含：Cloudflare 侧需要做什么、服务器侧需要开放哪些出站、如何排障）。
- `README.md`（可选，待实现后）：补充“公网访问/集成设置”的入口与安全注意事项（token 不回显、建议配合 Cloudflare Access）。

## 里程碑（Milestones）

- [x] M1: 需求口径冻结：运维 CLI 安装/部署/自启动策略（回答开放问题并冻结口径）
- [x] M2: 冻结契约：HTTP APIs / Config / CLI / File formats（contracts 完整、可测试）
- [ ] M3: 实现运维 CLI：安装依赖（xray/cloudflared）+ 初始化目录与权限 + 注册自启动服务（root）
- [ ] M4: 文档与手册：三发行版安装/排障指南（含 Cloudflare API 权限与 DNS 说明）
