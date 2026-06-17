# Xray Stale TCP Reclaim History

## Key Decisions

- 2026-06-16: 选择“双层默认”方案，由静态 `policy.levels.0` 与动态业务 inbound `socket_settings` 共同承担 stale TCP reclaim。
- 2026-06-16: 明确 `sockopt` 仅覆盖业务 inbound；`api` 与 `mesh-proxy` 保持控制面 loopback 语义，不纳入本次修复。
- 2026-06-16: 明确旧节点 rollout 通过一次 `xp-ops upgrade` 完成，且 merge-ready 以前必须通过共享测试机真实 Xray 验证。
- 2026-06-17: 明确 `xp-ops upgrade` 在 `xray` restart 失败时必须同时回滚静态 config 与 `xp` 二进制，避免留下半升级节点。
- 2026-06-17: 明确静态 config rewrite 必须保留既有控制面 listener 绑定；自定义 `XP_XRAY_API_ADDR` 与既有 `mesh-proxy` listener 不得被默认值覆盖。
- 2026-06-17: 明确真实 Xray ignored suites 在 CI helper 中必须单线程运行；两套 suite 共享同一个外部 Xray 进程与转发端口，不能依赖 libtest 默认并发。
- 2026-06-17: 明确 shared testbox real-Xray runner 需要显式隔离子网，并在 vendored OpenSSL 路径下预检宿主 `make` 依赖，避免共享机默认 Docker 地址池耗尽或远端编译在中途失败。
