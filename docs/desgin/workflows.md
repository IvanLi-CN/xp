# xp · 操作流程与一致性约束（MVP）

本文件描述：管理员在任意节点操作时，`xp` 如何把“期望状态”一致地落到各节点 `xray` 的运行态，并在配额超限时自动封禁。

## 1. 全局一致性：什么必须一致？

以下数据属于“配置/期望状态”，必须通过 Raft 强一致复制：

- Nodes / Endpoints / Users / Grants 的元数据与启用状态
- 端点协议参数与密钥材料（或其加密封装）
- 用户订阅 token

以下数据属于“运行态/高频数据”，默认不进 Raft（避免日志膨胀）：

- 每个 Grant 的累计用量（本机持久化即可）
- 节点健康、探活时间、`xray` 在线状态等

## 2. 关键不变量（Invariants）

对每个节点 `node_i`：

1. **Inbound 存在性**
   - 若 Raft 中存在 `Endpoint(node_i, tag=T)`，则本机 `xray` 必须存在入站 `tag=T`；
   - 若该 Endpoint 被删除，则本机入站 `tag=T` 必须被移除。

2. **Client 存在性（按 Grant）**
   - 若 `Grant.enabled=true` 且 `Grant.endpoint_id` 属于本节点，则该 Grant 对应 client 必须存在于该入站；
   - 若 `Grant.enabled=false`，则该 client 必须不存在（被移除）。

3. **统计可观测性**
   - 每个 client 必须设置 `email=grant:<grant_id>`，否则无法按 Grant 统计流量。

这些不变量由每个节点的 **reconcile 循环**维持（见 §6）。

## 3. 集群启动与加入

### 3.1 初始化（首节点）

管理员在首节点执行：

1. `xp init`
   - 生成集群 CA（自签）与首节点证书；
   - 生成 `admin_token`；
   - 初始化 Raft 存储目录；
   - 写入本机配置（node_id、public_domain、对外 api_base_url 等）。
2. 启动 `xp` 与 `xray`（`xray` 使用“基础配置”，入站由 `xp` 动态注入）。

### 3.2 加入新节点

1. 管理员在任意节点请求 join token（写请求会被转发到 leader）：
   - 生成一次性 token（包含 leader 地址、集群 CA、一次性密钥、过期时间）。
2. 新节点执行 `xp join --token ...`：
   - 校验 token 有效性；
   - 与 leader 建立 HTTPS 连接，提交 CSR；
   - leader 校验一次性密钥后签发节点证书；
   - 新节点带证书加入 Raft，开始复制状态机。

## 4. Endpoint 生命周期

### 4.1 创建 Endpoint

管理员在任意节点发起创建请求（写请求转发到 leader）：

1. 在 Raft 写入 Endpoint 记录（分配 endpoint_id、tag、端口、协议 meta、所属 node_id）。
2. 各节点应用该日志：
   - 只有所属节点 `node_id` 的 `xp` 会在本机调用 `xray.HandlerService.AddInbound` 创建入站。
3. `xp` 记录本机“已应用版本”，防止重复 AddInbound（幂等）。

### 4.2 删除 Endpoint

1. Raft 删除 Endpoint。
2. 所属节点 reconcile：
   - 调用 `RemoveInbound(tag)`；
   - 删除本地用量存储中该 Endpoint/Grant 的条目（可保留备查）。

### 4.3 shortId 旋转（VLESS/Reality）

1. 管理员请求 rotate-shortid（写入 Raft）：
   - 为该 Endpoint 追加新的 shortId，并将 `active_short_id` 指针切换到新值；
2. 所属节点需更新入站的 `realitySettings.shortIds`：
   - 若 Xray API 不支持原地改 streamSettings，则采取“重建入站”的方式：
     - `RemoveInbound(tag)` → `AddInbound(new)` → 重新 AddUser（按 Grants）。
3. 订阅输出立即使用新 `active_short_id`。

## 5. User / Grant 生命周期

### 5.1 创建 User

1. 写入 User（生成 `subscription_token`，周期默认配置，固定 `cycle_tz=UTC+8`）。

### 5.2 创建 Grant（给用户分配端点）

1. 写入 Grant（生成凭据）：
   - VLESS：UUIDv4 + email=`grant:<grant_id>`
   - SS2022：生成 user_psk；client password 为 `server_psk:user_psk`
2. 所属节点应用：
   - 调用 `AlterInbound(tag, AddUserOperation)` 将 client 添加到入站。

### 5.3 禁用/启用 Grant

- 禁用：
  1. 将 `Grant.enabled=false` 写入 Raft；
  2. 所属节点 reconcile：调用 `RemoveUserOperation` 移除 client。
- 启用：
  1. 将 `Grant.enabled=true` 写入 Raft；
  2. 所属节点 reconcile：调用 `AddUserOperation` 添加 client（并确保 shortId/端点存在）。

## 6. reconcile 循环（核心）

触发时机：

- `xp` 启动时（确保 `xray` 重启后恢复运行态）
- Raft 状态机应用新日志后（或按固定间隔）
- `xray` 探活失败恢复后

步骤（本节点视角）：

1. 获取本节点拥有的 Endpoints 与 Grants（来自已复制的 Raft 状态机）。
2. 列出本机 `xray` 的入站与 users（通过 `ListInbounds` + `GetInboundUsers` 或必要时本地缓存）。
3. 对每个 Endpoint：
   - 若缺失则 AddInbound；
   - 若存在但关键 meta 不一致（如 shortIds 旋转）则重建入站并重放 users；
4. 对每个 Grant：
   - enabled=true 但 client 缺失 → AddUser；
   - enabled=false 但 client 存在 → RemoveUser；

要求：所有操作必须 **幂等**、可重试、按顺序执行（避免抖动）。

## 7. 配额统计与封禁流程

### 7.1 统计采集

- 定时任务（如 5s–30s，可配置）从本机 Xray StatsService 拉取：
  - `user>>>grant:<grant_id>>>traffic>>>uplink`
  - `user>>>grant:<grant_id>>>traffic>>>downlink`
- 用本地游标计算增量，累计到本地持久化用量中。

### 7.2 周期重置

- 每个 Grant 计算当前周期窗口（ByNode/ByUser）：
  - ByUser：固定 `UTC+8`
  - ByNode：节点本地时区
- 到达新周期起点：
  - 将本地累计用量归零；
  - 若配置为“自动解封”，则将 `Grant.enabled=true` 写入 Raft（并由 reconcile 加回 users）。

### 7.3 超限封禁

- 当 `used_bytes + 10MiB >= quota_limit_bytes`：
  - 立即执行 RemoveUser（本机）；
  - 将 `Grant.enabled=false` 写入 Raft（确保全局一致）。

## 8. 订阅生成流程

1. 通过 token 查找 User；
2. 获取该 User 的所有 enabled Grants；
3. 对每个 Grant 生成对应 URI：
   - host = Endpoint 所属 Node.public_domain
   - port = Endpoint.port
   - VLESS：携带 pbk、active shortId(sid)、fp、flow、sni 等
   - SS2022：method 固定；password=`server_psk:user_psk`
4. 按请求输出 Raw/Base64/Clash YAML。
