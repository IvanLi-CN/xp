# xp · 架构设计（MVP）

> 本文描述：控制面 `xp` 的模块划分、与 Xray 的集成方式、以及 1–20 节点集群的一致性与运行流程。

## 1. 架构总览

每台服务器固定两进程：

- `xray`：数据面（代理转发）
- `xp`：控制面（API/订阅/集群/配额/下发）

控制面目标：

- 管理员对“全局期望状态”进行增删改查
- 期望状态通过集群一致性复制到所有节点
- 每个节点只对“属于自己的端点”执行对本机 `xray` 的调用（AddInbound / AddUser / RemoveUser）
- 通过 Xray StatsService 统计流量，执行配额封禁

## 2. 核心模块

### 2.1 HTTP API Server

承载三类入口（同一 listener，减少端口占用；对外 HTTPS 由反代/隧道终止）：

- 管理员 API：`/api/admin/*`
- 订阅 API：`/api/sub/*`
- 集群内部：`/api/cluster/*`（join、心跳、转发等）与 Raft RPC

默认只监听 `127.0.0.1:<port>`（由反代/隧道/组网提供可达性）。

### 2.2 Cluster / Raft

职责：

- 维护强一致“期望状态”（Nodes/Endpoints/Users/Grants）
- leader 串行化写入；followers 复制并应用
- follower 接收管理员写请求时转发到 leader（或返回 leader 地址）

运行态（如用量累计）不写入 Raft，避免日志膨胀与额外内存/IO。

### 2.3 State Machine（期望状态）

关键实体：

- Node：节点信息（access_host、api_base_url、时区等）
- Endpoint：一个 inbound（属于某个 node）
- User：订阅 token、周期默认策略（ByUser=UTC+8）
- Grant：用户在端点上的授权（凭据、配额、enabled）

关键约定：

- 每个 Grant 的 client `email` 固定为 `grant:<grant_id>`（用于统计与定位）。

### 2.4 Xray Controller（本机 xray 适配层）

职责：

- 通过 Xray gRPC API：
  - `HandlerService.AddInbound/RemoveInbound`：端点生命周期
  - `HandlerService.AlterInbound(AddUser/RemoveUser)`：授权启用/禁用
  - `StatsService`：读取 `uplink/downlink`
- 抽象出幂等操作（重复调用不应破坏状态）
- 在 `xray` 重启后可重建运行态（reconcile）

### 2.5 Reconciler（核心循环）

目标：维持不变量（期望状态 → 运行态）。

触发：

- `xp` 启动
- Raft 应用新日志
- 周期性（兜底）
- 检测到 xray 恢复/重启

步骤（本节点视角）：

1. 拉取本节点拥有的 Endpoints + Grants（来自已复制状态机）
2. 确保每个 Endpoint 对应 inbound 存在（必要时 AddInbound）
3. 对每个 Grant：
   - enabled=true 且 client 不存在 → AddUser
   - enabled=false 且 client 存在 → RemoveUser
4. 若 Endpoint meta 发生必须重建的变更（例如 REALITY shortIds 旋转且无法原地修改）：
   - RemoveInbound → AddInbound(new) → 重放 grants

要求：所有步骤可重试、顺序执行、不会因单个失败导致整体停摆（记录错误并继续）。

### 2.6 Quota Enforcer（配额执行器）

职责：

- 定时从 StatsService 拉取每个 Grant 的 uplink/downlink 累计值
- 用本地游标计算增量并累计到本地用量（不进 Raft）
- 超限判定：`used + 10MiB >= limit` 即封禁
  - 本机立即 RemoveUser
  - 写入 Raft：`Grant.enabled=false`（全局一致）
- 周期切换：
  - ByUser：UTC+8，按每月 X 日 00:00（缺日用当月最后一天 00:00）
  - ByNode：节点本地 00:00（同规则）
  - 可选策略：新周期自动解封（写入 Raft enabled=true）

## 3. 协议端点结构（数据面）

### 3.1 VLESS + REALITY + vision（TCP）

- Endpoint 维护：
  - REALITY privateKey/publicKey(pbk)
  - shortIds（必须生成非空；十六进制，最长 16 字符）
  - active_short_id（订阅默认使用）
- Grant 维护：
  - UUID
  - email=`grant:<grant_id>`
  - flow=`xtls-rprx-vision`

### 3.2 Shadowsocks 2022（multi-user）

- Endpoint 维护：
  - method=`2022-blake3-aes-128-gcm`
  - server_psk_b64（按规范生成）
- Grant 维护：
  - user_psk_b64（按规范生成）
  - password 组合：`server_psk:user_psk`
  - email=`grant:<grant_id>`

## 4. 集群安全（HTTPS-only）

推荐默认：mTLS（集群自签 CA 签发节点证书）。

### 4.1 init / join

- `xp init`：生成 cluster CA、首节点证书、admin token、初始化 Raft 存储
- `xp issue-join-token`：leader 生成一次性 join token
- `xp join`：新节点携 token 提交 CSR，leader 签发证书并加入 Raft

## 5. 关键接口轮廓

- 管理员接口：Nodes/Endpoints/Users/Grants 的 CRUD
- 订阅接口：`/api/sub/{token}` 输出 raw/base64/clash
- 集群接口：`/api/cluster/info`、`/api/cluster/join`、以及写转发（实现可内部化）

具体字段见：`api.md` 与 `subscription.md`（以实现时的 API 文档为准）。
