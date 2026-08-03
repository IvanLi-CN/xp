# Reality fallback 控制面 Mesh 与系统状态页演进历史（#56dtr）

> 本文记录影响范围和决策的稳定原因。

## Decisions

- Reality fallback 是普通 HTTPS ingress，不是 VLESS peer tunnel。
- 应用层重复执行与 TLS 密文重放是不同问题。
- body hash、稳定 request ID 和 durable idempotency 处理跨路径重复执行。
- trusted TLS termination 且关闭 0-RTT 时，不为该威胁模型增加 nonce cache。
- auth v1/v2 以维护窗口 hard cut，不支持持续 mixed-version 集群。
- 状态页采用 all-peer table 和 uptime strip，不采用 topology graph。
- `XP_MESH_PROXY_URL` 保留为公网 egress compatibility，不定义 Mesh。

## Supersession

- This topic supersedes `nbs5f-xray-control-plane-relay`.
- Legacy SOCKS relay remains public egress compatibility only.
