# Reality fallback 控制面 Mesh 与系统状态页演进历史（#56dtr）

> 本文记录影响范围和决策的稳定原因。

## Decisions

- Reality fallback 是普通 HTTPS ingress，不是 VLESS peer tunnel。
- 应用层重复执行与 TLS 密文重放是不同问题。
- body hash、稳定 request ID 和 durable idempotency 处理跨路径重复执行。
- trusted TLS termination 且关闭 0-RTT 时，不为该威胁模型增加 nonce cache。
- auth v1/v2 以维护窗口 hard cut，不支持持续 mixed-version 集群。
- 状态页采用 all-peer table 和 uptime strip，不采用 topology graph。
- Peer row actions belong to an explicit fixed-width grid column; when the data columns cannot fit,
  the row switches to the stacked presentation instead of allowing controls to overflow the panel.
- Mock-only page demos establish presentation states but cannot establish shared AppShell geometry;
  production-route layout regressions require a real-route geometry assertion.
- `XP_MESH_PROXY_URL` 保留为公网 egress compatibility，不定义 Mesh。
- HTTP/2 ingress may expose an absolute-form URI. Canary forwarding must discard its origin and
  combine only the authenticated raw path/query with the fixed XP loopback origin; forcing
  HTTP/1.1 would hide the defect instead of preserving the transport contract.
- Disabling reqwest idle pooling fixed a historical Cloudflare stale-socket failure, but it is not
  suitable for periodic Reality Mesh traffic: each request creates a new TLS/TCP connection while
  Xray retains the idle inbound for five minutes. Mesh therefore uses one strict shared H2 pool with
  a 120-second idle bound; public direct and relay keep separate compatibility pools.
- Connection reuse telemetry derives an ephemeral fingerprint from socket metadata but persists
  only aggregate generations and counters. This makes churn diagnosable without exposing network
  identity.

## Supersession

- This topic supersedes `nbs5f-xray-control-plane-relay`.
- Legacy SOCKS relay remains public egress compatibility only.
