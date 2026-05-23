# Xray 控制面 Relay 组网（#nbs5f）

## 状态

- Status: 已完成
- Created: 2026-05-23
- Last: 2026-05-23

## 背景 / 问题陈述

- 现有 `xp` 只把 `xray` 当作本机数据面和统计面，节点间控制面互访仍依赖公网 `api_base_url`、反代或隧道。
- 这让 Raft RPC、leader 转发、节点 runtime fan-out、endpoint probe fan-out 等控制面流量无法统一经过 `xray` 代理层。
- 需要一个可选、可回退、可观测的控制面 relay 入口，让节点间控制面请求可以经由本机 Xray 代理出口转发，而不改变用户订阅语义。

## 目标 / 非目标

### Goals

- 为控制面 HTTP 请求增加可选的 Xray relay 出口，默认保持关闭，启用后由 `xp` 自动复用。
- 让 Raft/API 互访、node runtime fan-out、probe fan-out 与其它节点间 admin 读取请求优先走 relay 出口，再回退公网 `api_base_url`。
- 在 `xp-ops init` 生成的静态 Xray 配置里增加本机代理入口，供控制面客户端复用。
- 保持现有公网 `api_base_url`、Cloudflare Tunnel 与反代路径不变，作为 bootstrap 和 fallback 入口。
- 将 relay 的启用状态、回退状态和错误原因暴露到 runtime / health / admin 观测面。

### Non-goals

- 不实现 L3 VPN、系统路由接管、TUN/TAP、ACL 平面或 full mesh 自动寻址。
- 不改变用户订阅、Mihomo provider-only 或 endpoint 业务流量的语义。
- 不把 Xray gRPC API 暴露给节点间网络。
- 不要求现网默认启用 relay；v1 保持显式配置开启。

## 需求（Requirements）

### MUST

- 新增可选配置用于启用控制面 relay 代理地址，未配置时必须保持直接公网访问路径不变。
- 控制面远程请求必须统一经过单一 client factory，能在启用 relay 时自动注入 proxy。
- `xp-ops init` 生成的静态 Xray 配置必须包含一个本机代理入口，默认监听 loopback。
- 远程 fan-out 失败时必须回退到公网 `api_base_url`，并记录 fallback 原因。
- 运行状态必须能区分 `disabled`、`ready`、`degraded`、`fallback`。

### SHOULD

- relay proxy 地址可通过环境变量覆盖，便于不同部署选择本机 SOCKS/HTTP 入口。
- 失败日志应包含目标节点、目标 URL、proxy 是否启用与最终回退路径。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 节点启动时，`xp` 构建一个控制面 HTTP client；若配置了 relay proxy，就让该 client 的出站走本机 Xray proxy。
- 所有节点间远程 HTTP 请求统一改用该 client，包括 leader 转发、节点 runtime fan-out、probe fan-out、runtime 读取等。
- 若 relay 不可用或未配置，client 自动回退为直连公网 `api_base_url`。
- 本机 Xray 配置增加一个 loopback 代理入口，供上述 client 使用，但不暴露业务订阅端口。

### Edge cases / errors

- relay proxy 地址非法：启动阶段必须报配置错误。
- relay 可用但远端节点不可达：返回原有网络错误语义，不静默吞错。
- relay 代理故障：自动切回直连，并在观测中标记 degraded/fallback。

## 接口契约（Interfaces & Contracts）

- 新增可选配置项：
  - `XP_MESH_PROXY_URL`
  - `--mesh-proxy-url <URL>`
- 新增 runtime 观测字段：
  - `mesh_proxy_url`
  - `mesh_proxy_status`
  - `mesh_proxy_fallback_reason`

## 验收标准（Acceptance Criteria）

- Given 未设置 relay 配置，When 节点启动并执行远程 fan-out，Then 行为与当前公网直连一致。
- Given 设置了合法 relay 配置，When 节点执行远程 fan-out，Then 请求通过该 proxy client 发出。
- Given relay 配置非法，When 节点启动，Then 直接失败并给出明确配置错误。
- Given relay 失效，When 远程请求重试，Then 系统回退到公网 `api_base_url` 并记录 fallback。
- Given `xp-ops init`，When 生成 Xray 静态配置，Then loopback 代理入口存在且可用于控制面 client。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认 v1 只做控制面 relay，不做 full mesh。
- 已确认控制面远程请求可以统一走一个 reqwest client factory。
- 已确认本机 Xray 可提供 loopback 代理入口。

## 非功能性验收 / 质量门槛

- Rust 单元测试覆盖 relay client builder、fallback、非法 URL、观测状态。
- HTTP fan-out 测试覆盖启用/禁用 relay 两条路径。
- `xp-ops init` 配置生成测试覆盖新增 Xray 代理入口。

## 文档更新

- `docs/desgin/cluster.md`
- `docs/desgin/xray.md`
- `README.md`
- `docs/ops/docker.md`

## 实现里程碑（Milestones）

- [x] M1: 配置与 client factory 接入 relay proxy
- [x] M2: xray static config 增加 loopback 代理入口
- [x] M3: 所有控制面远程请求切到统一 client
- [x] M4: tests、docs、观测字段同步

## 风险与开放问题

- 代理入口默认协议选型要与 reqwest 支持和现有 Xray 配置兼容。
- 如果控制面请求量升高，需观察 proxy client 对超时与连接池的影响。

## 假设

- 控制面 relay 默认关闭，仅在显式配置后启用。
- 现有公网 `api_base_url` 仍然是 bootstrap / fallback 的权威入口。
