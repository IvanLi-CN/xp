# 集群节点部署与管理员凭据一致性 (#38wmj)

## 背景

XP 集群的所有节点共享一个管理员凭据。服务端和节点磁盘只保存 Argon2id PHC，bootstrap 时生成一次明文，join 时从 leader 同步 PHC，避免节点间登录状态不一致。

## 目标与范围

- `xp-ops deploy` 覆盖 bootstrap 和非交互 join，并让新节点使用集群现有 PHC。
- bootstrap 生成高熵 Token，只向操作者显示一次；日志、配置和集群状态不得保存明文。
- join token 保持短 TTL、一次性，并承载加入所需的集群信任材料。
- Web 节点引导跟随 leader 版本生成可执行命令。

不引入 RBAC、SSO、Vault/KMS 或管理员 Token 找回协议。

## 契约

- `/etc/xp/xp.env` 使用 `XP_ADMIN_TOKEN_HASH='<argon2id PHC>'`；PHC 必须能被 POSIX shell 安全读取。
- `POST /api/cluster/join` 返回集群当前 PHC，且不得记录该字段。
- fresh join 使用 leader 驱动的两阶段协议。Phase 1 在全 voter lifecycle capability barrier
  通过后记录 Join membership operation、reservation join token、desired-state node 和 durable
  join session，再注册 Raft learner 并立即返回证书、CA 与 PHC；不得在
  HTTP response 前等待 learner catch-up 或 voter promotion。
- join session 状态为 `reserved -> learner_registered -> consumed|expired`。session 保存规范化
  请求指纹、签发证书、token expiry、固定 10 分钟 activation deadline 和 required log index。
  相同 token、CSR 与节点参数的重试重放同一逻辑结果，包括已 consumed session；不同指纹冲突，
  且重试不得延长 activation deadline。
- `xp join` 在首次请求前原子保存 pending key/CSR 与请求身份。响应丢失或进程重启必须复用同一
  pending identity；只有证书、CA、PHC、metadata 和初始 store 全部持久化后才能删除 pending
  文件并返回成功。
- leader coordinator 从 Raft session 恢复工作：认证 learner 达到 required log index 后才晋升
  voter 并标记 consumed。leader 重启或换主必须继续；超出 activation deadline 时移除未完成
  membership/desired-state node 并标记 expired。terminal tombstone 至少保留 24 小时且覆盖 token
  expiry，节点删除也不得使 token 可复用。
- 接受 fresh join 前，leader 必须确认所有现有 voter 声明
  `cluster.membership-lifecycle-v1` capability；未完成滚动升级或无法验证的集群返回
  `coordinated_upgrade_required`，不创建 reservation 或 lifecycle command。
- bootstrap response 携带当前 leader node ID 和当前 voter node IDs。joiner 将 0600 bootstrap
  marker 与证书材料一起原子持久化，marker 同时记录 activation deadline；每个 Raft RPC 仍须
  通过 v2 HMAC、cluster/target/time-window 校验。只要 marker 文件存在且未过期，marker 中记录的
  当前 voter sender（包括故障切换后新选出的 leader）
  可跳过尚未复制的 sender membership lookup；其他仅持有 cluster CA 的 principal 必须拒绝。
  部分复制期间 marker 保留并允许剩余记录 sender 继续发送；仅当 marker 中全部 voter sender
  都已出现在本地 state 且一次 RPC 成功后才删除 marker；过期 marker 在读取时清理，后续请求恢复
  为正常 membership 校验。
- membership guard 不得晋升任何 learner。它只报告异常并由 recorded Join operation / JoinSession
  coordinator 续跑合法 learner；没有 session 的 learner 是阻断成员变更的异常，而非 recovery
  候选。
- single-image wrapper 在 fresh learner 首次复制完成前必须保持 XP 子进程存活。managed-default
  reconcile 必须先通过本机 internal API 验证复制完成，再由本机 Raft 转发写入；仅当本机 API
  明确返回 signer 尚未进入 state machine 的认证失败或本地 leader membership 尚未复制时，才可
  在 activation deadline 内重试；其他认证、网络和配置错误立即失败。
- host-managed startup 的 managed-default reconcile 在后台持续重试到成功，因此 learner 初次复制
  超过旧的短重试窗口时也不会永久遗漏配置端点。
- `GET /api/cluster/info` 提供 leader 的 XP 版本，供部署命令锁定版本。
- 所有节点必须使用字节完全一致的 PHC；轮换会使旧管理员 Token 和由旧 PHC 签发的短期登录 Token 失效。
- host-managed join 依次执行 Tunnel/DNS provision（不启动服务）、`xp join`、
  `/etc/xp/xp.env` 写入，再按 `xray`、`xp`、`cloudflared` 顺序启用并启动或重启，逐个确认 ready。
- 启用服务的 join 仅在最终 `api_base_url/health` 返回 HTTP `200` 时成功。`502`、`530`、
  超时和连接失败返回 `post_join_health_failed`，但不得删除已写入的 metadata、撤销 Raft
  成员资格或轮换管理员凭据；重试必须识别已有 metadata 并继续配置与验证，不重复执行 join。

## 验收

- bootstrap 节点只保存 PHC，明文只显示一次。
- join 节点无需输入管理员明文即可与集群凭据一致。
- 同一管理员 Token 可访问任一节点，且缺参、权限或配置冲突返回可操作错误。
- 部署和 join 测试不得在输出中出现明文或完整 PHC。
- join 测试覆盖服务启动顺序、严格公共健康门禁，以及 post-join 失败后的可恢复重试
  语义。
- join 测试必须覆盖 Phase 1 不等待 catch-up、同指纹幂等重试、冲突请求、响应丢失、joiner/leader
  重启、promotion failure retry、expiry cleanup，以及旧 state/existing-node recovery 兼容。

## 参考

- `docs/specs/k9n2r-xp-ops-admin-token-ops/SPEC.md`
- `docs/specs/nmgq8-managed-stack-64m-memory/SPEC.md`
- Legacy source retained pending delete approval: `docs/plan/38wmj:cluster-node-onboarding/PLAN.md`
