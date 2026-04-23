# 单 Docker 镜像集群节点部署（#c8qtw)

## 状态

- Status: 已完成
- Created: 2026-04-23
- Last: 2026-04-23

## 背景 / 问题陈述

- 仓库此前只有 `scripts/dev/subscription-3node-compose/` 下的本地回归容器资产，不构成正式生产部署路径。
- 现有宿主机运维路径以 `xp + xray + cloudflared` 的 systemd/OpenRC 托管为主，而 Docker 用户需要一个单镜像、单 entrypoint、可复用现有 Cloudflare Tunnel 逻辑的节点运行模型。
- 发布流程此前只产出 Linux musl 二进制，没有官方 GHCR 镜像，也没有面向 PR 的镜像 smoke build。

## 目标 / 非目标

### Goals

- 交付官方单镜像节点部署路径：镜像内包含 `xp`、`xp-ops`、真实嵌入版 `web/dist`、`xray`、`cloudflared`、`tini`。
- 新增 `xp-ops container run` 作为唯一容器入口，负责 bootstrap/join、可选 Cloudflare Tunnel provisioning、子进程托管、健康退出与信号转发。
- 支持 bootstrap 节点与 join 节点两类首启流程，并保证重启后不会重复 `xp init` / 重复创建 Tunnel 与 DNS 资源。
- 支持保留既有数据卷时的自动 realign：环境变量变化后自动对齐 node metadata、DDNS runtime state 与默认托管 endpoint。
- 发布 `ghcr.io/<owner>/xp` 多架构镜像，并在 CI 中新增 Docker smoke build。
- 提供可直接复用的 Compose 示例与运维文档，覆盖 Cloudflare Tunnel 开关、持久化卷、bootstrap/join 使用方式。

### Non-goals

- 不把 `xp-ops deploy` 改造成 Docker 编排器。
- 不移除或重写现有 systemd/OpenRC 运维路径。
- 不为 `xp` 新增一套 container-specific cloudflared runtime contract；容器模式下 `cloudflared` 由 `xp-ops container run` 托管。

## 范围（Scope）

### In scope

- `xp-ops container run` 命令、容器环境变量契约、Cloudflare container runtime 复用与相关单元测试。
- 正式 `Dockerfile`、`deploy/docker/compose.bootstrap.yml`、`deploy/docker/compose.join.yml`。
- `.github/workflows/ci.yml` Docker smoke build 与 `.github/workflows/release.yml` GHCR 多架构发布。
- `README.md`、`docs/ops/README.md`、`docs/ops/docker.md`、`docs/ops/cloudflare-tunnel.md`。

### Out of scope

- Kubernetes manifests、Helm chart、Swarm stack。
- 容器外部的日志/监控系统集成。
- Cloudflare Access / WAF / Access policy 的自动化配置。

## 需求（Requirements）

### MUST

- `xp-ops` 提供 `container run` 子命令，并支持 `--dry-run`。
- 容器环境变量契约固定为：
  - 必填：`XP_NODE_NAME`
  - Tunnel 关闭时必填：`XP_API_BASE_URL`
  - Tunnel 开启时必填：`XP_ENABLE_CLOUDFLARE=true`、`XP_CLOUDFLARE_ACCOUNT_ID`、`XP_CLOUDFLARE_HOSTNAME`
  - join 模式必填：`XP_JOIN_TOKEN`
  - bootstrap 模式必填：`XP_ADMIN_TOKEN` 或 `XP_ADMIN_TOKEN_HASH`
  - Tunnel secret 使用：`CLOUDFLARE_API_TOKEN`
- 容器固定持久化卷：`/var/lib/xp/data`、`/etc/cloudflared`、`/etc/xp-ops/cloudflare_tunnel`。
- Tunnel 开启且未显式提供 `XP_API_BASE_URL` 时，默认派生为 `https://<XP_CLOUDFLARE_HOSTNAME>`。
- join 节点在首次执行 `xp join` 前，必须先完成/reuse Tunnel 配置并等待 public `api_base_url` 就绪。
- 当既有 `metadata.json` 与环境变量不一致时，容器入口必须自动 realign 本地 metadata，并在 `xp` 启动后同步回 Raft state machine / membership metadata。
- 容器入口必须支持 `XP_CLOUDFLARE_DDNS_ENABLED` 驱动的 DDNS runtime token file 准备，以及 `XP_DEFAULT_VLESS_*` / `XP_DEFAULT_SS_PORT` 驱动的默认 endpoint reconcile。
- runtime 镜像入口固定为 `tini -- xp-ops container run`。
- release 工作流必须发布 `linux/amd64` 与 `linux/arm64` 的 GHCR 镜像；稳定版发布 `vX.Y.Z`、`X.Y.Z`、`latest`，预发布不推 `latest`。

### SHOULD

- PR CI 至少验证镜像可构建，并验证默认 entrypoint 的 bootstrap dry-run 合同。
- Compose 示例默认提供 loopback 端口映射与容器内 healthcheck，便于本地检查 `/` 与 `/api/health`。
- 文档应明确说明：容器模式下 `xp` 仍会监控 `xray`，但 `cloudflared` 由容器入口托管，因此 runtime 页面会把 `cloudflared` 视为 disabled。

## 功能与行为规格（Functional/Behavior Spec）

### Bootstrap flow

- 空数据卷且未提供 `XP_JOIN_TOKEN` 时，`xp-ops container run` 进入 bootstrap 流程。
- bootstrap 流程要求显式提供 `XP_ADMIN_TOKEN` 或 `XP_ADMIN_TOKEN_HASH`；首次启动执行一次 `xp init`，后续重启复用已有 cluster metadata。
- 若启用 Cloudflare Tunnel，容器会在启动 `xp` 前复用/补齐 Cloudflare 本地配置，并拉起 `cloudflared` 子进程。

### Join flow

- 提供 `XP_JOIN_TOKEN` 且数据卷为空时，容器进入 join 首启流程。
- 若启用 Tunnel，容器必须先 provision/reuse Tunnel、本地 credentials 与 DNS，再等待 `https://<hostname>/health` 可达，然后执行 `xp join --token ...`。
- join 首次 runtime reconcile 必须允许直接复用 join token 自带的 `leader_api_base_url` 作为 control-plane 写入目标，避免因为本地 follower 尚未学到 leader routing 而阻塞默认 endpoint / node-meta 对齐。
- join 成功后会复用 leader 返回的证书与 admin token hash；重启时不再重复 join。

### Runtime reconcile flow

- 对已有数据卷重启时，容器入口保留既有 `cluster_id` / `node_id`，并将环境变量中的 `XP_NODE_NAME`、`XP_ACCESS_HOST`、`XP_API_BASE_URL` 写回本地 metadata。
- `xp` 启动后，入口会使用内部签名请求把同样的节点元数据同步回 Raft state machine 与 membership node meta。
- 当 `XP_CLOUDFLARE_DDNS_ENABLED=true` 时，入口会在启动 `xp` 前写好 DDNS runtime token file，并确保 `xp` 拿到最终 `XP_CLOUDFLARE_DDNS_ZONE_ID`。
- 默认托管 endpoint 使用保守 adopt 策略：当前节点上某个 kind 恰好只有一条 endpoint 时可以 adopt；若存在多条同 kind endpoint，则直接报错，避免误伤人工配置。

### Child supervision

- 容器入口负责拉起 `xray`、`xp`、可选 `cloudflared`。
- 任一关键子进程异常退出时，入口会终止其余子进程并让整个容器失败退出。
- `SIGTERM` / `Ctrl-C` 会向子进程转发终止信号并等待短暂优雅退出。

## 接口与运维契约（Interfaces & Ops Contracts）

### Container env contract

| Key                                                                                         | Required when   | Description                                      |
| ------------------------------------------------------------------------------------------- | --------------- | ------------------------------------------------ |
| `XP_NODE_NAME`                                                                              | always          | 节点名                                           |
| `XP_ADMIN_TOKEN` / `XP_ADMIN_TOKEN_HASH`                                                    | bootstrap       | bootstrap 节点 admin token/hash                  |
| `XP_JOIN_TOKEN`                                                                             | join            | join token                                       |
| `XP_API_BASE_URL`                                                                           | tunnel disabled | 节点可达 HTTPS origin                            |
| `XP_ENABLE_CLOUDFLARE`                                                                      | optional        | `true/false`                                     |
| `XP_CLOUDFLARE_ACCOUNT_ID`                                                                  | tunnel enabled  | Cloudflare account id                            |
| `XP_CLOUDFLARE_ZONE_ID`                                                                     | optional        | 显式 zone id；未提供时可由 hostname 解析         |
| `XP_CLOUDFLARE_HOSTNAME`                                                                    | tunnel enabled  | Tunnel 对外 hostname                             |
| `XP_CLOUDFLARE_TUNNEL_NAME`                                                                 | optional        | Tunnel 名称；默认 `xp-<node-name>`               |
| `XP_ACCESS_HOST`                                                                            | optional        | 对外 endpoint hostname；启用 DDNS 时推荐显式设置 |
| `XP_CLOUDFLARE_DDNS_ENABLED`                                                                | optional        | 是否启用 `XP_ACCESS_HOST` 的 runtime DDNS        |
| `XP_CLOUDFLARE_DDNS_ZONE_ID`                                                                | DDNS enabled    | DDNS zone id；与 Tunnel 同 zone 时可自动复用     |
| `XP_DEFAULT_VLESS_PORT` / `XP_DEFAULT_VLESS_REALITY_DEST` / `XP_DEFAULT_VLESS_SERVER_NAMES` | optional        | 默认托管 VLESS endpoint 契约                     |
| `XP_DEFAULT_SS_PORT`                                                                        | optional        | 默认托管 SS2022 endpoint 契约                    |
| `CLOUDFLARE_API_TOKEN`                                                                      | tunnel enabled  | Cloudflare API token                             |

### Volume contract

- `/var/lib/xp/data`：cluster metadata / raft / state / certificates
- `/etc/cloudflared`：`config.yml` 与 `<tunnel-id>.json`
- `/etc/xp-ops/cloudflare_tunnel`：Tunnel settings 持久化

## 验收标准（Acceptance Criteria）

- Given 正式 `Dockerfile` 构建完成，When 运行容器，Then 镜像内嵌真实前端资源，不再依赖占位 `web/dist/index.html`。
- Given 空数据卷与 bootstrap 环境变量，When 容器首次启动，Then 只执行一次 `xp init`；重启后不重复初始化。
- Given 空数据卷、join token 与 Tunnel 环境变量，When 容器首次启动，Then 会先完成/reuse Tunnel 配置并等待 public URL，再执行 `xp join`。
- Given 已存在 `/etc/cloudflared` 与 `/etc/xp-ops/cloudflare_tunnel` 卷，When 容器二次启动，Then 复用已有 Tunnel credentials/settings，不生成重复 Tunnel/DNS 资源。
- Given 既有数据卷且节点 hostname 变更，When 使用新的 `XP_NODE_NAME` / `XP_ACCESS_HOST` / `XP_API_BASE_URL` 重启，Then 本地 metadata 与集群 node meta 自动对齐到新值。
- Given 设置了 `XP_DEFAULT_VLESS_*` / `XP_DEFAULT_SS_PORT`，When 容器首次启动或后续重启，Then 当前节点存在与 env 对齐的默认托管 endpoint；删除这些 env 后，对应托管 endpoint 会被移除。
- Given PR CI 运行，When Docker job 执行，Then 镜像 smoke build 通过且默认 entrypoint dry-run 合同通过。
- Given release 工作流成功，When 版本被发布，Then GHCR 产出 amd64/arm64 镜像与约定 tag。
- Given 操作者只参考 README 与 ops 文档，When 按 Compose 示例部署 bootstrap / join 节点，Then 能明确知道所需 env、secret、volume 与 Cloudflare 前提。

## 实现前置条件（Definition of Ready / Preconditions）

- 仓库已有 `xp-ops`、`xp`、Cloudflare provision 与 Xray 静态配置生成逻辑可复用。
- 快车道 stop condition 已锁定为 `merge-ready`。
- 本任务不需要 UI 视觉证据（非 UI-affecting）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cargo run --quiet --bin xp-ops -- container run --dry-run`（bootstrap contract）
- `docker build ...` + 容器 dry-run smoke

### Ops / Release

- GH Actions PR job 可完成 Docker smoke build。
- GH Actions release job 可发布 GHCR 多架构镜像。

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `README.md`
- `docs/ops/README.md`
- `docs/ops/docker.md`
- `docs/ops/cloudflare-tunnel.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 spec，冻结单镜像 / Tunnel / GHCR / 卷契约
- [x] M2: 实现 `xp-ops container run` 与 Cloudflare container runtime 复用
- [x] M3: 交付正式 Dockerfile 与 Compose 示例
- [x] M4: 扩展 CI / release 到 Docker smoke + GHCR 多架构发布
- [x] M5: 同步 README / ops 文档并收口到 merge-ready PR

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：启用 Cloudflare 时，每次启动都需要可用的 `CLOUDFLARE_API_TOKEN` 来执行 provisioning/reconcile。
- 风险：容器模式下 `cloudflared` 不受 `xp` runtime supervisor 直接监控，运维观测需额外查看容器日志/编排器状态。
- 假设：每个节点通常部署在独立宿主机；若在同一宿主机并行运行多个节点，需要调整 `XP_HOST_PORT` 避免冲突。
- 开放问题：None.

## 变更记录（Change log）

- 2026-04-23: 创建规格，冻结 `xp-ops container run`、单镜像发布、Compose 示例、Cloudflare Tunnel 契约与 GHCR 发布要求。
- 2026-04-23: 完成容器入口、正式 Dockerfile、CI/release 更新与运维文档同步。
