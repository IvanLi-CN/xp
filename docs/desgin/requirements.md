# xp · 需求分析（MVP）

> 目标用户：个人/自用场景；固定单管理员。

## 1. 背景与目标

你希望在多台服务器上运行 Xray 作为代理节点，并提供一个统一的“管理面”来：

- 统一创建与管理 Xray 配置（重点是入站与其 clients）
- 为用户分配可用的代理端点（Endpoints），并支持动态调整权限
- 对每个端点/授权执行月度流量配额（双向合计），超限自动禁用
- 支持 1–20 节点集群，访问任意节点的面板/API 都能管理整个集群

约束：

- 控制面资源极小：`xp` 常驻内存目标 **≤32MiB**（RSS，不含 xray）
- 系统内只运行 **两** 个进程：`xp` + `xray`（无外部依赖服务）
- 节点间通信仅使用 **HTTPS**（尽量减少端口与协议面）
- `xp` 默认只绑定 `127.0.0.1`（方案 A），由组网/隧道/反代实现互访与对外访问
- `xp` 默认只提供 **HTTP** 服务（回环），对外 **HTTPS** 由反代/隧道终止（如 Cloudflare Tunnel / Caddy）

## 2. 术语定义

- 节点（Node）：每台服务器上的 `xp + xray` 实例
- 端点（Endpoint）：一个 Xray `InboundObject`（一个入站）
- 用户（User）：管理员创建的用户
- 授权（Grant）：用户与端点的绑定关系（包含凭据、配额与启用状态）
- 周期（Cycle）：月度配额窗口

## 3. 功能需求（Functional）

### 3.1 节点与集群

- 支持 1–20 节点加入同一集群
- 管理员访问任意节点的 Web 面板/API：
  - 读：可查看全局配置（节点/端点/用户/授权）
  - 写：可修改全局配置，并在集群内一致生效
- 节点间通信：
  - 使用 HTTPS（建议 mTLS）
  - 同一端口承载管理 API + 集群 RPC（减少端口占用）

### 3.2 端点（Inbound）管理

- 支持创建两类端点：
  - VLESS + REALITY + vision（TCP）
  - Shadowsocks 2022（`2022-blake3-aes-128-gcm`，TCP+UDP）
- 支持删除端点
- 支持生成端点所需的协议材料：
  - VLESS/Reality：privateKey/publicKey(pbk)、shortIds（必须生成非空）
  - SS2022：server PSK（base64，按规范生成）
- 端点默认绑定策略：
  - **xp**：只绑定 `127.0.0.1`
  - **xray**（数据面端口）：由端点配置决定（通常 `0.0.0.0:<port>`）

### 3.3 用户与授权（clients）管理

- 单管理员可创建/删除/禁用用户
- 每个用户拥有订阅 token（可重置）
- 一个用户可拥有多个授权（Grant）：
  - 授权可跨节点、可同时拥有多个端点
  - 授权可被禁用（禁用后禁止连接）
- 授权需要生成客户端凭据：
  - VLESS：UUID
  - SS2022：user PSK（base64，按规范生成）

### 3.4 订阅链接

每个用户通过 token 获取订阅，输出格式包括：

- Raw URI（逐行）
- Base64（将 Raw 整体 base64）
- Clash YAML（面向 Mihomo/Clash.Meta）

订阅中的 host 使用节点配置域名（未来“优选域名”不纳入 MVP）。

### 3.5 配额与周期重置

配额口径：

- 双向合计：uplink + downlink
- 允许 10MiB 误差

周期策略（两种）：

- ByNode：按节点本地时区的 00:00
- ByUser：按用户固定时区 **UTC+8** 的 00:00

周期起点规则：

- 每月 X 日 00:00
- 若当月无 X 日：取当月最后一天 00:00

超限行为：

- 超限后立即禁止连接：
  - 将该授权对应的 client 从 Xray inbound clients 中删除或禁用（MVP 采用“删除 client”）

## 4. 非功能需求（Non-functional）

### 4.1 资源与性能

- 常驻内存目标：**≤32MiB**（`xp` RSS，不含 xray）
- 设计上避免：
  - 高频状态写入一致性日志（如用量累计不进 Raft）
  - 大对象常驻内存（订阅/配置按需生成）
- 关键操作必须幂等与可重试（网络抖动、节点重启可恢复）

### 4.2 安全

- 单管理员模型（一个 admin token）
- 订阅 token 仅允许读，不允许写
- 节点间通信 HTTPS-only；建议 mTLS 确认节点身份

### 4.3 可运维性

- 运行环境：Arch/Debian/Alpine
- 能在 systemd（以及 Alpine 的 openrc）下以服务形式运行
- 可观测：基本日志、节点健康、错误可定位

## 5. 关键设计选择（已确认）

- 动态管理方式：使用 Xray gRPC API（HandlerService 的 AddInbound / AddUser / RemoveUser）
- 集群一致性：Raft 选主（leader 写入，followers 复制）
- 通信：节点间只用 HTTPS；xp 默认绑定回环

## 6. MVP 验收清单（建议）

1. 单机：
   - 创建 VLESS/SS 端点
   - 创建用户与授权
   - 导出订阅并成功连接
2. 单机配额：
   - 统计可用（按授权 email 区分）
   - 超限自动封禁（client 被移除）
   - 周期切换后自动解封（若启用该策略）
3. 集群（3 节点示例）：
   - 任意节点写入都能落到全局一致
   - 端点与用户变更只在所属节点触发对本机 xray 的调用
   - 节点重启后 reconcile 可恢复运行态
