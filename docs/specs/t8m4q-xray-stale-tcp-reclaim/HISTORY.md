# Xray Stale TCP Reclaim History

## Key Decisions

- 2026-06-16: 选择“双层默认”方案，由静态 `policy.levels.0` 与动态业务 inbound `socket_settings` 共同承担 stale TCP reclaim。
- 2026-06-16: 明确 `sockopt` 仅覆盖业务 inbound；`api` 与 `mesh-proxy` 保持控制面 loopback 语义，不纳入本次修复。
- 2026-06-16: 明确旧节点 rollout 通过一次 `xp-ops upgrade` 完成，且 merge-ready 以前必须通过共享测试机真实 Xray 验证。
