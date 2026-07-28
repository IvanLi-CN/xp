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
- `GET /api/cluster/info` 提供 leader 的 XP 版本，供部署命令锁定版本。
- 所有节点必须使用字节完全一致的 PHC；轮换会使旧管理员 Token 和由旧 PHC 签发的短期登录 Token 失效。

## 验收

- bootstrap 节点只保存 PHC，明文只显示一次。
- join 节点无需输入管理员明文即可与集群凭据一致。
- 同一管理员 Token 可访问任一节点，且缺参、权限或配置冲突返回可操作错误。
- 部署和 join 测试不得在输出中出现明文或完整 PHC。

## 参考

- `docs/specs/k9n2r-xp-ops-admin-token-ops/SPEC.md`
- `docs/specs/nmgq8-managed-stack-64m-memory/SPEC.md`
- Legacy source retained pending delete approval: `docs/plan/38wmj:cluster-node-onboarding/PLAN.md`
