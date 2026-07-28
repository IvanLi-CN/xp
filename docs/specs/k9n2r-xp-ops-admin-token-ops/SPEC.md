# xp-ops 管理员 Token 运维 (#k9n2r)

## 背景

管理员 Token 变更只有在服务重新读取环境后才生效。多节点集群还必须复制同一个 PHC；未经 shell 引用的 PHC 会被 `$` 展开破坏。

## 契约

- `xp-ops admin-token set` 接收明文、stdin 或 PHC，并以单引号 shell-safe 形式更新 `XP_ADMIN_TOKEN_HASH`。
- 默认删除遗留 `XP_ADMIN_TOKEN` 明文；`--quiet` 只输出稳定成功标识。
- 写入后不自动重启，输出本机重启、跨节点 PHC 同步和鉴权验证指引。
- `xp-ops xp restart` 按当前 init system 执行显式重启，并支持 dry-run。

## 验收

- PHC 经 POSIX shell 读取后字节不变。
- systemd/OpenRC 的设置、重启提示和错误路径有测试覆盖。
- 命令输出不包含管理员明文。
- 集群轮换使用同一 PHC 并逐节点验证后再继续。

## 参考

- `docs/specs/38wmj-cluster-node-onboarding/SPEC.md`
- `docs/specs/nmgq8-managed-stack-64m-memory/SPEC.md`
- Legacy source retained pending delete approval: `docs/plan/k9n2r:xp-ops-admin-token-ops/PLAN.md`
