# Xray 控制面 Relay History

## Key Decisions

- 2026-05-23: 选择控制面专用 relay v1，范围限定为 Raft/API 互访与节点间 fan-out，不做 full mesh/L3 VPN。
- 2026-05-23: 完成基于 `XP_MESH_PROXY_URL` 的 relay-aware HTTP client、Xray loopback SOCKS 静态入口、fallback 观测与项目文档同步。
