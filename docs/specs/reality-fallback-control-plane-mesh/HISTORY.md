# Reality fallback 控制面 Mesh 与系统状态页演进历史（#56dtr）

> 本文记录影响范围和决策的稳定原因。

## Decisions

- The Web control now treats the Mesh write route as an independent compatibility contract,
  serializes the shared local lock map with Web Locks before persisting the local unknown result
  lock, rechecks the selected backend after lock acquisition, and cancels stale confirmation
  refresh callbacks. It retains the lock until an accepted target is confirmed, fails closed for
  unreadable lock storage, and refuses to send after cancellation while waiting for the lock.
  The accepted follow-up observation remains inside the backend mutation barrier and must match
  the confirmed backend generation and cluster id before the lock can be cleared. Missing setting
  values are rendered as `Unknown`, never as `Disabled`.
  Unknown settings render as an indeterminate control, stale or unsampled observer peers remain
  visibly marked, and missing observations never render as zero counts.

- Cluster Mesh configuration belongs in Service config, while System Status keeps node-local
  path diagnosis. A single control reflects the Raft setting; Public fallback and dormant
  Native Reverse are not independently selectable capabilities. An ambiguous write remains
  locked until an administrator independently verifies termination and convergence, because
  a local status read cannot identify the write's outcome.
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
- 本机控制面代理已完全删除；Mesh 与 peer public HTTPS origin 是同级 direct path。
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
- Authenticated snapshot admission belongs before OpenRaft dispatch: contention is a signed,
  retryable transport refusal, while an admitted reservation blocks regular and re-enable Mesh
  dispatch plus new deferred epoch readers until the real installation task ends without holding the
  state-machine gate recursively. Direct cleanup overflow retains the newest operation identity so
  a delayed older callback cannot displace a newer safety result.
- Body-completion commits recheck absolute deadlines after acquiring circuit, validation, and
  telemetry locks. A callback that is first polled after expiry cannot mutate status; completed Mesh
  success synchronously persists its breaker and route reason, while ordinary samples remain
  coalesced.
- Web upgrade 的互斥所有权属于活进程锁，而不是锁文件路径。OpenRC one-shot 必须在退出后
  清理自身状态；409 只有在状态刷新证明任务仍 active 时才进入观察，否则作为 stale conflict
  立即解锁。

## Supersession

- This topic supersedes the retired local control-plane proxy contract.
