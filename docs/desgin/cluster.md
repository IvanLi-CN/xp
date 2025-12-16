# xp · 集群一致性与安全（Raft + mTLS，HTTPS-only）

## 1. 目标

- **无外部依赖**：不使用外置数据库、Etcd、Consul、Redis 等。
- **任意节点入口可管理全局配置**：管理员访问任意节点，都能完成创建用户/端点/授权等操作。
- **强一致配置**：避免平等模式的冲突合并；写入由 leader 串行化。
- **HTTPS-only 节点互联**：节点间通信只走 HTTPS；尽量单端口承载（减少端口占用）。

## 2. 基本拓扑（方案 A：回环监听 + 外部可达入口）

- `xp` 默认监听：`127.0.0.1:<xp_port>`（HTTPS）
- 其他节点访问本节点时，使用 `Node.api_base_url`：
  - 它可能是：内网地址 / 组网地址 / 反向代理地址 / 隧道地址
  - 最终转发到本机 `127.0.0.1:<xp_port>`

> 你明确要求绑定回环；因此“互访可达性”由你的组网/隧道/反代提供，`xp` 只要求最终能被 HTTPS 访问到。

## 3. 一致性模型：Raft

### 3.1 角色

- **Leader**：唯一写入入口，负责：
  - 接收管理员写请求（或处理 follower 转发）
  - 追加 Raft 日志并复制
  - 对外返回写入结果（commit 后）
- **Follower**：只读副本，负责：
  - 接收管理员请求并在必要时转发到 leader
  - 复制/应用状态机
  - 对“本节点拥有的资源”执行 reconcile（调用本机 xray）

### 3.2 写请求转发

- 当管理员访问到非 leader：
  - follower 返回 leader 地址并自动转发（HTTP 307/内部代理皆可，具体实现后定）
- 要求：对管理员而言，访问任意节点的体验一致。

### 3.3 成员数量与投票

- 规模：1–20 节点（全部为 voter）
- 取舍：网络分区时少数派不可写（已确认可接受）

### 3.4 存储

- 每个节点在本地磁盘持久化：
  - Raft log（WAL）
  - snapshot
  - cluster metadata（cluster_id、ca、node cert）
- 目标：即使节点重启也能恢复集群状态，不依赖外部存储。

## 4. 节点身份与加密：mTLS（推荐默认）

### 4.1 为什么需要 mTLS

- 防止“伪装节点”加入或发起管理写入
- 在隧道/反代等复杂网络下仍能验证对端身份

### 4.2 证书体系

- 集群初始化时生成：
  - `cluster_ca`（自签 CA）
  - `node_cert`（首节点证书）
- 所有节点：
  - 服务端与客户端都使用节点证书（双向认证）
  - 节点证书内包含 `node_id`（建议放在 SAN/DNSName 或 URI SAN）

### 4.3 Join Token 流程（一次性加入）

1. leader 生成 `join_token`（一次性、带过期）：
   - 包含：`cluster_id`、`leader_api_base_url`、`cluster_ca_pem`、`one_time_secret`
2. 新节点执行 join：
   - 用 token 联系 leader 的 `/cluster/join`
   - 提交 CSR + token 校验材料
3. leader 校验通过后：
   - 签发节点证书并返回
4. 新节点持证书加入 Raft

### 4.4 退化模式（不推荐）

若你坚持“TLS 全由外部反代终止且不做透传”，则可退化为：

- 节点间 HTTPS（由反代提供）+ 共享 bearer token

缺点：节点身份仅靠共享密钥，边界安全更依赖你的网络与反代配置。

## 5. 访问控制（单管理员 + 订阅只读）

- 管理员 API：
  - `Authorization: Bearer <admin_token>`
  - 仅管理员可写（创建/修改/删除 Nodes/Endpoints/Users/Grants）
- 订阅 API：
  - `GET /sub/{token}`（只读）
  - token 随机不可预测；建议支持随时重置

## 6. 端口占用策略

推荐：单端口承载三类流量（同一 HTTPS listener）：

1. 管理员 API（/admin/*）
2. 订阅 API（/sub/*）
3. 集群内部（/cluster/* + Raft RPC）

这样每节点除 Xray 数据面端口外，`xp` 只占用一个 HTTPS 端口。
