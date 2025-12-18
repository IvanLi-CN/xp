# Milestone 2 · Xray Controller + Reconcile（单机闭环）— 需求与概要设计

> 对齐计划：`docs/plan/README.md` 的 **Milestone 2**。\
> 参考：`docs/desgin/xray.md` / `docs/desgin/architecture.md` / `docs/desgin/workflows.md` / `docs/desgin/api.md`

## 1. 背景与目标

Milestone 1 解决的是“控制面骨架 + 期望状态（单机可持久化）”。但此时 `xp` 仍未把变更落到本机 `xray` 的运行态（inbounds/clients），即：

- 管理 API 的写入只改变 `xp` 的期望状态；
- `xray` 的动态入站与 client 仍需手工维护，且 `xray` 重启后会丢失运行态。

Milestone 2 的目标是完成 **单机闭环**：将“期望状态 → 运行态”自动收敛，并具备可重试、幂等、可恢复能力。

## 2. 范围与非目标

### 2.1 范围（M2 交付）

- **Xray gRPC 适配层（Xray Controller）**：
  - `HandlerService.AddInbound/RemoveInbound`
  - `HandlerService.AlterInbound(AddUser/RemoveUser)`
  - `StatsService` 的基础读取能力（按 Grant email 读取 uplink/downlink）
- **协议材料生成与持久化（期望状态的一部分）**：
  - VLESS/Reality：key pair、shortIds、active_short_id
  - SS2022：server PSK（base64）与 user PSK（base64）与 password 组合规则
- **Reconciler（收敛循环）**：
  - 触发：`xp` 启动 / 写操作后 / 周期性兜底
  - 能将本机 `xray` 的 inbounds/clients 收敛到期望状态
- **运行态恢复**：
  - `xray` 重启或运行态丢失后，能自动重建 inbound 与 clients

### 2.2 非目标（明确不做）

- 订阅输出（Milestone 3）。
- 配额统计与封禁（Milestone 4）：M2 只要求 StatsService 适配能力，不做周期与超限动作。
- Raft 集群、join/init、写转发（Milestone 5）。
- 完整 Web 面板（Milestone 6）。

## 3. 关键用例（单机闭环）

1. 创建 Endpoint：写入期望状态后，本机 `xray` 自动出现对应 inbound（tag 对齐）。
2. 删除 Endpoint：inbound 被移除（并同步清理该入站下的 clients）。
3. 创建 Grant（enabled=true）：client 被添加到对应 inbound。
4. 禁用/启用 Grant：
   - enabled=false → client 被移除；
   - enabled=true → client 被重新添加。
5. REALITY shortId 旋转：期望状态更新后，所属 inbound 按需重建并重放 clients。
6. `xray` 重启：`xp` 能在下一次 reconcile 中恢复全部 inbounds 与 clients。

## 4. 数据/领域模型（M2）

原则：M2 要确保“订阅输出/配额/集群”未来不会被字段缺失卡住，因此协议关键字段应尽量在 M2 前固定（允许为空/默认值要明确）。

### 4.1 Endpoint.meta（按 kind 分支）

当前代码中 `Endpoint.meta` 为 `serde_json::Value`，M2 推荐约定其结构并在写入/读取处进行校验（是否抽成强类型结构体属于实现细节）。

#### 4.1.1 VLESS + REALITY + vision（TCP）

`Endpoint.kind=vless_reality_vision_tcp` 时，`meta` 需包含：

- `public_domain`：用于订阅 host（如未来需要端点级覆盖，需在此明确）
- `reality`：
  - `dest`（例如 `"example.com:443"`）
  - `server_names: string[]`（SNI 备选；订阅默认取首个）
  - `fingerprint`（默认 `chrome`）
- `reality_keys`：
  - `private_key`（x25519）
  - `public_key`（由 private_key 推导，用于订阅 `pbk`）
- `short_ids: string[]`
- `active_short_id: string`

shortId 约束见 `docs/desgin/xray.md`：

- 十六进制字符串，长度为 2 的倍数，最长 16 个字符；
- 本项目禁止空串 shortId。

> 旋转策略（最多保留几个、淘汰策略）在实现前需确认，见 §9。

#### 4.1.2 Shadowsocks 2022（multi-user）

`Endpoint.kind=ss2022_2022_blake3_aes_128_gcm` 时，`meta` 需包含：

- `method`：固定 `2022-blake3-aes-128-gcm`
- `server_psk_b64`

### 4.2 Grant.credentials（按 Endpoint.kind 分支）

- 统一约定：每个 Grant 的 client `email` 固定为 `grant:<grant_id>`（用于 StatsService 统计与定位）。

#### 4.2.1 VLESS

- `uuid`：UUIDv4
- `email`：`grant:<grant_id>`
- `flow`：订阅/下发使用 `xtls-rprx-vision`（如需写入结构体，建议在生成时固定）

#### 4.2.2 SS2022

本项目采用多用户 SS2022：客户端 password 为 `server_psk_b64:user_psk_b64`（见 `docs/desgin/subscription.md`）。

- `method`：`2022-blake3-aes-128-gcm`
- `password`：`<server_psk_b64>:<user_psk_b64>`
- `email`：`grant:<grant_id>`

## 5. Xray 基础配置约束（M2 必须满足）

`xp` 依赖 `xray` 启用 gRPC API 与统计能力，详见 `docs/desgin/xray.md`。M2 至少要在文档与日志中明确以下前置条件：

- 启用 `api` 并包含服务：`HandlerService`、`StatsService`；
- 启用 `stats: {}`；
- policy 开启 `statsUserUplink/statsUserDownlink`；
- client 必须设置 `email`，否则无法按 Grant 统计。

## 6. 模块边界与内部接口（建议）

M1 已给出 `config/domain/state/service/http` 的边界。M2 建议新增两个模块并保持依赖方向清晰：

- `xray`：本机 xray 适配层（tonic client + domain-friendly API）
- `reconcile`：收敛循环（读取期望状态，驱动 xray 模块执行幂等操作）

依赖方向建议：

`http -> service -> (state + reconcile)`\
`reconcile -> (state + xray)`\
`xray` 不依赖 `http`；`domain` 不依赖 `xray`。

### 6.1 gRPC 连接与安全边界（结论）

M2 采用最小假设：`xp` 与 `xray` 同机部署，gRPC API 仅监听回环地址。

- 默认地址：`127.0.0.1:10085`（与 `docs/desgin/xray.md` 的推荐配置一致）
- 连接方式：gRPC plaintext（回环内网，不经反代）
- 安全边界：由“同机 + 回环监听 + OS 进程隔离”保证；不引入 TLS/mTLS（如未来确有需求，再扩展到 TLS/Unix socket）

### 6.2 Xray Controller（接口“形状”）

> 这里只定义能力轮廓，避免把实现细节写死。

- `add_inbound(tag, inbound_config)`
- `remove_inbound(tag)`
- `add_user(tag, user)`
- `remove_user(tag, email)`
- `get_user_traffic(email) -> { uplink, downlink }`（为 M4 预留）

错误处理目标：

- 对“已存在/不存在”的场景，映射为可接受的幂等结果（不作为致命失败）；
- 其余错误保留原因并打日志，交给 reconcile 继续兜底重试。

## 7. Reconcile 设计（单机闭环核心）

### 7.1 触发时机

- `xp` 启动：做一次全量 reconcile（用于运行态恢复）。
- 写操作成功后触发：Endpoint/Grant 的 create/update/delete 后触发一次 reconcile（可做事件合并）。
- 周期性兜底：固定间隔触发（避免遗漏事件、处理瞬时失败）。

### 7.2 执行模型（建议）

- 单实例、顺序执行：同一时刻最多一个 reconcile 在跑，避免抖动与竞态。
- 事件合并（coalesce）：短时间内多次触发只跑一次“最新快照”。
- 失败不阻塞：某个 Endpoint/Grant 操作失败时记录错误并继续处理其它对象；下轮再重试。

### 7.3 收敛步骤（本节点视角）

1. 从期望状态加载：
   - 本节点拥有的 Endpoints（单机模式即全部）
   - 与这些 Endpoints 关联的 Grants
2. 对每个 Endpoint：
   - 确保 inbound 存在（必要时 `AddInbound`）
   - 若 Endpoint 的关键 meta 发生变化且无法原地更新（例如 REALITY shortIds 旋转）：
     - `RemoveInbound` → `AddInbound(new)` → 重放该 Endpoint 下所有 enabled Grants
3. 对每个 Grant：
   - enabled=true 且 client 缺失 → AddUser
   - enabled=false 且 client 存在 → RemoveUser
4. 处理删除：
   - Endpoint 被删除 → RemoveInbound(tag)
   - Grant 被删除/迁移 → RemoveUser(email)

> 结论：reconcile **不依赖** “列出 inbounds/users”来做 diff。\
> 原因：不同发行版/版本的 Xray 在 HandlerService 中对 List/Get 能力的暴露并不稳定，且项目目标是“尽量少前置假设”。\
> 策略：以“**期望状态为源**”进行 apply，并将“已存在/不存在”的错误视为幂等成功（必要时基于错误码/错误信息做归类）；仅在必须重建的变更（如 shortId 旋转）时对单个 inbound 做 `RemoveInbound → AddInbound → 重放 users`。

## 8. 兼容性与迁移考虑

- **向 M5（Raft）迁移**：Reconcile 的输入应来自“已复制的期望状态”，不要把运行态或用量写入同一状态机。
- **字段稳定性**：VLESS/SS2022 的关键材料与字段在 M2 尽量定全，避免 M3/M4 再引入必填字段导致迁移。
- **Stats key 约定**：保持 `grant:<grant_id>` 作为 email，确保 `user>>>...>>>traffic>>>uplink/downlink` 的 key 稳定。

## 9. 风险点与待确认问题

1. **“已存在/不存在”的错误归类**：由于 reconcile 不做运行态 diff，需要把 xray 的常见失败映射为幂等成功，否则会造成无意义的失败噪音。
2. **shortId 旋转策略（本方案已定）**：
   - `short_ids` 最多保留 **8** 个；
   - 每次旋转生成 **16 hex（8 bytes）** 的新 shortId，追加到列表并设为 `active_short_id`；
   - 超出容量时淘汰最旧的 shortId；
   - 旋转后由所属节点对该 inbound 执行“重建 + 重放 users”，确保 `realitySettings.shortIds` 生效。
3. **端点级 public_domain 的语义（本方案已定）**：
   - `public_domain` 是 **端点级** 的“对外连接域名”（通常是指向该节点的域名/反代入口），目前仅 VLESS Endpoint 需要；
   - 订阅规格当前以 `Node.public_domain` 为 host（见 `docs/desgin/subscription.md`），因此 MVP 阶段推荐保持 `Endpoint.public_domain == Node.public_domain`，避免产生额外语义分叉；
   - 若未来需要“一节点多域名/多入口”，可把订阅 host 规则升级为“优先 Endpoint.public_domain，否则回退 Node.public_domain”（属于后续变更）。
4. **reconcile 兜底周期与退避（本方案已定）**：
   - 事件触发：写操作成功后触发 reconcile（带 debounce 合并）
   - 周期兜底：默认 **30s**（可配置）
   - xray 不可用时：对 reconcile 触发采用指数退避（例如 1s 起步，最大 30s，带 jitter），避免忙等刷屏

## 10. 验收清单（M2 DoD）

- 创建/删除 Endpoint 能自动落到本机 xray（inbound 存在性一致）。
- 创建 Grant / 启用/禁用 Grant 能自动落到本机 xray（client 存在性一致）。
- shortId 旋转能驱动入站更新（必要时重建）且不丢失已启用授权。
- kill/restart xray 后，`xp` 能在合理时间内恢复运行态（无需人工干预）。
- StatsService 能按 `grant:<grant_id>` 读取 uplink/downlink（读不到时错误清晰可定位）。
- 提供可复现的真实 `xray-core` gRPC E2E smoke（见 `scripts/e2e/`），用于验证协议与 RPC 的实际兼容性。
