# Xray 控制面 Relay History

## Key Decisions

- 2026-05-23: 选择控制面专用 relay v1，范围限定为 Raft/API 互访与节点间 fan-out，不做 full mesh/L3 VPN。
- 2026-05-23: 完成基于 `XP_MESH_PROXY_URL` 的 relay-aware HTTP client、Xray loopback SOCKS 静态入口、fallback 观测与项目文档同步。
- 2026-05-23: 在共享测试机用三节点集群验证 runtime fan-out 通过 relay-aware client 成功，并在停掉 node2 Xray 后确认 direct fallback 生效。
