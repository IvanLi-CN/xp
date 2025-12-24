# Milestone 5 · Raft 集群（强一致 + HTTPS-only）— 需求与概要设计

> 对齐计划：`docs/plan/README.md` 的 **Milestone 5**。\
> 参考：`docs/desgin/cluster.md` / `docs/desgin/workflows.md` / `docs/desgin/api.md` / `docs/desgin/architecture.md` / `docs/desgin/requirements.md`

## 1. 背景与目标

Milestone 1–4 已完成单机闭环（期望状态持久化 + xray reconcile + 订阅输出 + 配额强约束）。但 MVP 仍缺少关键能力：**1–20 节点集群的强一致期望状态复制**，以支持“管理员访问任意节点都能管理整个集群”。

Milestone 5 的目标是在不引入外部依赖服务的前提下，交付以下能力：

- **强一致期望状态**：Nodes/Endpoints/Users/Grants 由 leader 串行化写入，通过 Raft 复制到所有节点并应用。
- **HTTPS-only 节点互联**：节点间通信只走 HTTPS（在 `xp` 进程内优先 mTLS；若 HTTPS 由外部反代/隧道终止，则退化为共享 token）。
- **init/join**：首节点初始化 + join token + 新节点通过 CSR 加入集群并加入 Raft。
- **写请求转发**：管理员写请求在 follower 上可被转发到 leader（或返回 leader 地址供客户端重试）。
- **Reconcile 分工**：每个节点只对“本节点拥有的端点”调用本机 xray，其余只复制状态。

## 2. 范围与非目标

### 2.1 范围（M5 交付）

- **Raft 一致性层**：
  - WAL + snapshot 持久化（每节点本地磁盘）。
  - 状态机仅保存“期望状态”（Nodes/Endpoints/Users/Grants）。
  - 支持节点重启恢复（Raft log/snapshot + 本地集群元数据）。
- **集群身份与密钥材料**（本地落盘）：
  - `cluster_ca`（自签 CA）与 `node_cert`（节点证书）；
  - join token 与 CSR 签发流程；
  - 轮换策略先给出最低实现（例如：手动触发/未来扩展点），不强制实现自动轮换。
- **对外 API（最小集群集成）**：
  - `/api/cluster/info` 返回真实 role/term/leader；
  - `/api/admin/cluster/join-tokens` 生成 join token（leader 写）；
  - `/api/cluster/join` 提交 join token + CSR，leader 返回签发证书并完成节点注册。
- **写一致性与转发**：
  - 所有写入期望状态的管理员 API 进入 Raft 提交；
  - follower 接收写请求时：转发到 leader 或返回 leader 地址（实现先选一种并统一行为）。
- **Reconcile/Quota 与集群集成**：
  - reconcile/quota 读取“已复制的期望状态”；
  - 仅对本节点拥有的 endpoints/grants 调用本机 xray；
  - 用量累计仍只存本地，不进入 Raft（避免日志膨胀）。

### 2.2 非目标（明确不做）

- 节点下线/移除、成员缩容与自动 reconfiguration（MVP 可后置）。
- 多管理员、多租户权限体系（仍为单管理员 token）。
- 把“HTTPS”终止与网络可达性内建到 `xp`：仍遵循默认只监听 `127.0.0.1`，外部 HTTPS 由组网/反代/隧道提供（见 `cluster.md`）。
- Web 面板完整集群 UI（Milestone 6 再做）。

## 3. 关键用例 / 用户流程

1. 首节点初始化：
   - 管理员在首节点执行 `xp init`：生成 `cluster_ca`、首节点证书、初始化 Raft 存储目录、写入本机元数据。
   - 启动 `xp`：`/api/cluster/info` 显示本机为 leader（单节点集群）。
2. 生成 join token（管理员）：
   - 管理员请求 `POST /api/admin/cluster/join-tokens`（follower 应转发到 leader）。
   - leader 生成一次性 token（带 TTL），并持久化“待加入记录”（用于一次性校验与防重放）。
3. 新节点加入：
   - 新节点执行 `xp join --token ...`：生成 CSR，调用 leader 的 `POST /api/cluster/join`。
   - leader 校验 token，签发节点证书，注册 Node 元数据，并把新节点加入 Raft（voter）。
4. 任意节点写入一致：
   - 管理员访问 follower 写入（创建/更新/删除 Nodes/Endpoints/Users/Grants），请求被转发到 leader。
   - leader 提交 Raft 日志并在 commit 后返回；followers 应用后达到一致状态。
5. 运行态分工：
   - endpoints/grants 复制到所有节点后，每个节点只对“属于自己的 node_id 的 endpoints”执行 reconcile（调用本机 xray）。
6. 重启恢复：
   - 任意节点重启：从本地 Raft 存储恢复状态机；reconcile 根据期望状态恢复本机 xray 运行态。

## 4. 数据与存储设计

### 4.1 期望状态（Raft 状态机）

状态机只包含“期望状态”的全量快照（可等价理解为单机版 `state.json` 的内容）：

- Nodes / Endpoints / Users / Grants

用量累计等高频数据仍按 Milestone 4 保持“本地持久化，不进 Raft”。

### 4.2 集群本地元数据（不进 Raft）

每个节点本地持久化以下内容（建议独立于期望状态快照文件）：

- `cluster_id`
- `node_id`
- `cluster_ca_pem`（以及私钥，若由本节点生成/持有）
- `node_cert_pem` + `node_key_pem`
- Raft 存储目录（WAL/snapshot）的位置与版本信息

> 约束：私钥与证书文件权限应尽量收紧（例如 0600），避免被普通用户读取。

### 4.3 Raft 持久化（WAL + snapshot）

- WAL：用于记录已提交/未提交日志，支持崩溃恢复。
- Snapshot：用于日志压缩与快速追赶，snapshot 内容为“期望状态的全量序列化” + 必要元数据（last applied index/term、membership 等）。
- 快照触发策略：先按固定阈值（entries/size）或定期触发，具体策略在实现时按资源约束调优。

## 5. 节点身份与 join token（mTLS 优先）

### 5.1 推荐模式：mTLS（集群自签 CA）

- 集群初始化时生成自签 CA（`cluster_ca`）。
- 节点之间的 HTTP 客户端与服务端都使用节点证书，双向校验对端证书是否由 `cluster_ca` 签发。

### 5.2 Join token（一次性 + 过期）

join token 用于“允许新节点加入集群”，建议包含：

- `cluster_id`
- `leader_api_base_url`
- `cluster_ca_pem`
- `expires_at`（或 ttl_seconds）
- `one_time_secret`

leader 侧需持久化 token 的“已发放/未使用”状态，用于：

- 一次性校验（用后作废）
- 过期回收
- 防重放（同 token 不可重复 join）

### 5.3 Join 接口（CSR 签发）

新节点调用 `/api/cluster/join` 提交：

- join token
- Node 元数据（node_name/public_domain/api_base_url）
- CSR（PEM）

leader 校验通过后：

- 签发证书并返回（PEM）
- 将 Node 记录写入期望状态并提交 Raft
- 将新节点加入 Raft 成员列表（voter）

### 5.4 退化模式（不推荐）

当 “TLS 全由外部反代终止且不做透传” 时，`xp` 进程无法做 mTLS，可退化为：

- 节点间 HTTPS（由反代保证）+ 共享 bearer token

缺点：节点身份边界更依赖外部网络与共享密钥。

## 6. 写请求转发策略

实现需在以下两种方式中选择一种并全局一致：

1. **内部代理转发**（推荐优先）：follower 作为反向代理将管理员写请求转发到 leader，并把 leader 的响应透传给客户端。
2. **返回 leader 地址**：follower 返回 307/308 并在错误 details 中携带 `leader_api_base_url`，由客户端重试。

无论采用哪种方式，都必须保证：

- 写请求最终只由 leader 提交 Raft；
- follower 不得在本地直接写入期望状态（避免分叉）。

## 7. Reconcile 分工与数据面调用约束

为避免“每个节点都下发所有端点”的错误行为，需要明确约束：

- 每个 Endpoint 归属一个 `node_id`。
- 每个节点在 reconcile/quota 过程中，只处理 `endpoint.node_id == this_node_id` 的 resources：
  - AddInbound/RemoveInbound
  - AddUser/RemoveUser
  - StatsService 拉取与用量累计
- 对于不属于本节点的 endpoints/grants：
  - 只复制并提供只读查询；
  - 不调用本机 xray。

## 8. 测试计划（M5）

- 单测：
  - join token 编解码、一次性校验、过期策略
  - CSR 校验与证书签发（合法/非法 csr_pem）
  - 写转发行为（leader/follower 分支）
- 集成/冒烟（2–3 节点）：
  - 任意节点写入，最终一致可读
  - 节点重启后可恢复一致性与 reconcile
  - 端点/授权变更只在所属节点触发 xray 调用（可用 mock 或日志断言）

## 9. 风险点与待确认问题

1. Raft 实现选型：需在实现前确认库与持久化方案（重点评估：快照接口、membership 变更、资源占用、可维护性）。
2. TLS 终止位置：是否需要强制要求“TLS 透传到 xp（mTLS）”，还是允许默认退化模式。
3. 节点 `api_base_url` 的可达性：依赖外部组网/隧道/反代；需要明确部署指引与最小可用拓扑。
4. 证书轮换策略：MVP 先支持手动轮换还是预留接口即可。
