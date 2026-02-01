# 本地 docker compose：3 节点订阅回归测试环境（#apxdg）

## 状态

- Status: 待设计
- Created: 2026-02-01
- Last: 2026-02-01

## 背景 / 问题陈述

- 线上反馈：用户配置多节点多端点后，订阅在客户端侧只显示部分节点/端点。
- 当前缺少一个可一键搭建/重置的本地 3 节点环境，用于稳定复现与回归验证。
- Join Token 强制 `leader_api_base_url` 为 `https://...`，因此本地环境需要 HTTPS 入口（通过反代/sidecar 终止 TLS）。

## 目标 / 非目标

### Goals

- 在开发机上用 `docker compose` 启动 3 个 `xp` 节点（单机多容器），形成可工作的 Raft 集群。
- 提供脚本一键：
  - `reset`：清理全部数据（包括 volumes）并回到干净状态
  - `up`：启动集群并等待 leader ready
  - `seed`：写入用于订阅回归的数据（>=2 节点，合计 4 endpoints + 1 user + grants）
  - `verify`：自动断言订阅输出符合预期（raw/clash；且在 leader/follower 上一致）
- 输出给使用者明确的访问地址、admin token、subscription token。

### Non-goals

- 不运行真实 `xray` 数据面与连通性测试（仅覆盖控制面/订阅输出）。
- 不做公网访问、Cloudflare tunnel、系统服务化（systemd/openrc）。
- 不把该环境当成生产部署方案。

## 范围（Scope）

### In scope

- 新增稳定路径下的本地测试环境资产（建议放到 `scripts/dev/subscription-3node-compose/`）：
  - `docker-compose.yml`（3xp + 3https proxy + 可选 init/seed helper）
  - TLS 证书生成脚本（使用 `xp init` 生成的 `cluster_ca_key.pem` 签发 server cert）
  - `seed` 脚本：调用 admin API 创建 nodes/endpoints/users/grant-groups
  - `verify` 脚本：拉取订阅并做可机读断言
  - `README.md`：使用说明与排障
- 端口与地址规划：本机暴露 3 个 HTTPS 入口（或至少暴露 2 个用于 follower 验证）。

### Out of scope

- 修改 `xp` 让它直接支持 HTTPS listener（仅靠反代/sidecar）。
- 引入新的集群协议或外置服务（etcd/consul 等）。
- 在 CI 中跑 3 节点 docker 回归（除非后续明确需要）。

## 需求（Requirements）

### MUST

- 可在一条命令内完成 `reset -> up -> seed -> verify`（脚本退出码可用于本地判断）。
- 3 节点成功形成 Raft 集群（`GET /api/cluster/info` 显示 1 leader + 2 follower；leader 允许变化但应稳定）。
- `seed` 生成的数据满足：
  - 至少 2 个节点各自有 endpoints，总计 4 个 endpoints
  - 同一个 user 对这 4 个 endpoints 都有 enabled grant
- `verify` 必须检查：
  - `GET /api/sub/<token>?format=raw` 行数=4，且 name（`#...`）唯一
  - `GET /api/sub/<token>?format=clash` proxies 数=4，且 `name` 唯一
  - 上述结果在 3 个节点入口上返回一致（允许 leader/follower role 不同）
- 环境重置后不会复用旧 cluster_id / state（避免“偶现通过”）。

### SHOULD

- 允许通过环境变量覆写宿主机端口（避免冲突）。
- 脚本提供 `logs`、`status`、`urls` 子命令以便排障。
- seed 数据不依赖前端 UI（纯 API）。

### COULD

- 提供可选的“导入到 mihomo/clash 的 smoke test”脚本（非强制门槛）。
- 提供 profile：仅起 leader（用于快速调试）。

## 接口契约（Interfaces & Contracts）

None（仅新增本地 dev 工具与脚本，不改对外 API）。

## 验收标准（Acceptance Criteria）

- Given 开发机安装 Docker engine + docker compose
  When 运行 `scripts/dev/subscription-3node-compose/run.sh reset-and-verify`
  Then
  - 脚本退出码为 0
  - 3 个节点均可通过各自 HTTPS 入口返回 `/api/cluster/info`
  - `verify` 断言全部通过（raw/clash 4 项且 name 唯一；三节点输出一致）

（补充关键边界与异常：端口被占用、Docker 未运行、证书生成失败、leader 选举超时、join token 过期）

## 实现前置条件（Definition of Ready / Preconditions）

- 确认：本地 env 是否需要对 `/raft/*` 启用 mTLS（建议默认不启用；如要启用则需选择 Nginx/Envoy 支持按路径校验）
- 确认：宿主机端口分配策略（固定 vs 自动探测）
- 确认：镜像构建方式（本地源码 build vs 复用发布二进制）

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Script-level: `run.sh reset-and-verify` 连跑 3 次应稳定通过（无 flake）。

### Quality checks

- Shell style: `set -eu`；尽量 POSIX sh 兼容（避免 bash-only 语法）。
- Formatting: Markdown 通过 `bunx --no-install dprint fmt`（按仓库既有约定）。

## 文档更新（Docs to Update）

- `README.md`: 增加一条指向本地 3 节点 compose 回归环境的链接（可选）。

## 计划资产（Plan assets）

- None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [ ] M1: 新增 `scripts/dev/subscription-3node-compose/` 骨架与 README
- [ ] M2: 提供 `xp` 可用的 Docker 构建（包含 embedded `web/dist`）
- [ ] M3: 实现集群 init/join 自动化 + HTTPS 入口（单机 3 节点）
- [ ] M4: 实现 `seed` + `verify`，并让 `reset-and-verify` 幂等稳定

## 方案概述（Approach, high-level）

- 使用 3 个 `xp` 容器（各自独立 `XP_DATA_DIR` volume）。
- 每个节点通过 sidecar 反代终止 TLS，对外提供 `https://...` 入口并转发到 `xp` 的回环 HTTP。
- 由脚本串起：leader `xp init` → 启动 leader → 申请 join token → joiner `xp join` → 启动 joiners。
- `seed` 通过 admin API 写入 nodes/endpoints/users/grant-groups；`verify` 通过订阅接口断言输出。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - TLS host/SAN 不匹配导致 join 或 Raft RPC 失败
  - leader 选举与 join token 时序导致 flake
- 需要决策的问题：
  - 反代选型（Caddy vs Nginx）
  - 是否要在本地 env 对 `/raft/*` 开启 mTLS 校验
  - 宿主机端口映射策略
- 假设（需主人确认）：
  - 订阅回归不依赖 `xray` 运行态（仅需要 `xp` 控制面可用）。

## 变更记录（Change log）

- 2026-02-01: create plan

## 参考（References）

- `docs/desgin/cluster.md`
- `src/cluster_identity.rs`（join token 强制 https）
- `src/raft/network_http.rs`（Raft RPC 使用 https base url）
