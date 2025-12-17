# xp · 开发计划（MVP · 大纲）

> 参考：`docs/desgin/requirements.md` / `docs/desgin/architecture.md` / `docs/desgin/api.md` / `docs/desgin/quality.md`

## 0. MVP 目标（约束不变）

- 单机资源：`xp` 常驻内存 ≤32MiB（RSS，不含 xray）；每台服务器仅运行 `xp` + `xray`
- 端点：VLESS+REALITY(vision/TCP)、SS2022(2022-blake3-aes-128-gcm/TCP+UDP)
- 用户/授权：多端点、多节点；授权可禁用；订阅 token 可重置
- 配额：月度周期（ByNode / ByUser=UTC+8），双向合计；超限自动禁用
- 集群：1–20 节点，Raft 强一致；节点间 HTTPS-only（建议 mTLS）
- 订阅：Raw URI / Base64 / Clash YAML

## 1. Milestone：控制面基础（单机可用）

- 细化方案：`docs/plan/m1-control-plane-foundation.md`
- 工程与约定：配置加载（CLI/ENV/文件择一）、日志、错误格式、ID（ULID）
- 领域模型：Node / Endpoint / User / Grant 的核心字段与校验规则
- 管理员认证：`Authorization: Bearer <admin_token>` 的统一中间件
- 管理 API（先单机）：按 `docs/desgin/api.md` 落地 CRUD + leader/follower 预留（先返回本机）
- Web 联调：Vite proxy `/api` → `xp`；健康检查与最小页面贯通

## 2. Milestone：Xray Controller + Reconcile（单机闭环）

- 细化方案：`docs/plan/m2-xray-controller-reconcile.md`
- Xray gRPC 适配：`AddInbound/RemoveInbound`、`AlterInbound(AddUser/RemoveUser)`、`StatsService`
- 协议材料生成：
  - VLESS/Reality：key pair、shortIds、active_short_id
  - SS2022：server/user PSK（base64）与 password 组合规则
- Reconciler：启动/变更触发/周期性兜底，保证“期望状态 → 运行态”可重试、幂等
- 运行态恢复：检测 xray 重启后可重建 inbound 与 clients

## 3. Milestone：订阅输出（对用户可交付）

- 订阅 API：`/api/sub/{token}` 输出 raw/base64/clash（按 `docs/desgin/subscription.md`）
- token 生命周期：生成/重置/失效策略；只读权限边界清晰
- 订阅内容：按 Node 的 `public_domain`/端点端口拼装，确保与 Web 展示一致

## 4. Milestone：配额系统（单机强约束）

- 用量采集：Stats 拉取 + 本地游标增量累计（用量不进 Raft）
- 周期计算：ByUser(UTC+8)/ByNode(本地时区) 与 “缺日取月末” 规则
- 超限动作：本机 RemoveUser + 写入期望状态 `Grant.enabled=false`（单机先本地落盘）
- 周期切换：按策略自动解封/保持封禁（以 `docs/desgin/quota.md` 为准）

## 5. Milestone：Raft 集群（强一致 + HTTPS-only）

- Raft 选型与落地：WAL + snapshot；状态机仅存“期望状态”（Nodes/Endpoints/Users/Grants）
- 节点身份：cluster CA + 节点证书（mTLS）；证书/密钥落盘与轮换策略
- init/join：`xp init`、join token、`/api/cluster/join`（CSR 签发）与节点注册
- 写转发：follower 将写请求转发 leader（或返回 leader 地址供客户端重试）
- Reconcile 分工：仅对“本节点拥有的端点”调用本机 xray；其余只复制状态

## 6. Milestone：Web 面板（面向日常使用）

- 基础体验：登录（admin token）、全局错误提示、加载/空态/危险操作确认
- 视图与操作：Nodes/Endpoints/Users/Grants 的列表与详情、创建/删除、启用/禁用、rotate shortId、reset token
- 订阅：一键复制、格式切换、快速校验（客户端配置可用性以最小校验为主）
- 质量：Storybook 覆盖组件态；Playwright 做关键路径 E2E

## 7. 质量门禁与交付

- Rust：`cargo fmt` / `clippy -D warnings` / 单测 /（可选）集成测试（mock xray gRPC）
- Web：Biome check + `tsc -b` + 组件 stories 测试 + E2E（可按 CI 分层）
- 运维：systemd/openrc 示例、最小配置模板、升级/回滚与数据目录说明

## 8. MVP 验收（DoD 摘要）

0. 内存：启动后空闲 60s，`xp` RSS ≤32MiB（Linux 口径，推荐读取 `/proc/<pid>/status` 的 `VmRSS`）

   ```bash
   pid="$(pidof xp)"
   awk '/VmRSS/ { printf("xp RSS: %.2f MiB\\n", $2/1024) }' "/proc/${pid}/status"
   ```

1. 单机：创建端点 → 创建用户/授权 → 导出订阅 → 成功连通
2. 单机配额：可统计 → 超限封禁（client 移除）→ 周期切换按策略恢复
3. 集群：任意节点写入全局一致；节点重启后 reconcile 可恢复运行态
