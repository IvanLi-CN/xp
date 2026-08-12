# Reality fallback 控制面 Mesh 与系统状态页实现状态（#56dtr）

> 有效行为以 `./SPEC.md` 为准。

## Current Status

- Implementation: complete pending final review.
- Lifecycle: active.
- Catalog: supersedes `nbs5f`.

## Delivered

- internal-auth v2、purpose-separated ack（完整 canonical request digest）与 strict bodyless
  canary ingress。
- per-peer HTTPS Mesh transport、breaker、fallback 与本地 telemetry；Raft、leader forwarding、
  node history、探针、管理 fan-out 与 SSE 共用进程级传输 bundle。托管 Mesh 使用 HTTP/2-only
  client，每 origin 最多保留一条 idle connection，idle timeout 为 120 秒；公网 direct/relay
  fallback 使用独立的长期 client。空白 `access_host` 会直接选择公网 fallback，不会构造无效
  Mesh URL。
- durable local internal idempotency ledger。
- Mesh status API、status SSE revision 与 System Status Web surface；复用 telemetry 记录 H2 请求、
  connection start、generation 与 `healthy` / `churning` / `unknown` 状态，不持久化 socket 地址。
- Additive Mesh capability/reason fields with backward-compatible telemetry decoding;
  System Status row actions use shared 32x32 icon targets with consistent focus and tooltip
  behavior, while mobile text actions remain available.
- systemd、OpenRC、container cutover guard、可取消的 pre-consumption marker 与 operator documentation。
- Web upgrade start 使用 advisory `flock` 且在 host trigger 前释放。OpenRC delegate 通过固定
  helper 启动后台 one-shot，结束后 zap 服务状态；Web 将 409 后的 active job 与旧终态冲突
  分开处理。
- Storybook state gallery、mock-only `ui_demo`、desktop/mobile visual evidence。
- Peer transport uses a constrained desktop grid for its data and action columns, while narrower
  viewports switch to full-width stacked rows. Storybook asserts the two desktop action targets are
  `32x32` and remain inside the row boundary.
- The mock-only System Status `ui_demo` covers presentation states but does not establish shared
  AppShell geometry. The real `/system-status` route has a Playwright content-boundary regression
  check for peer-row actions at the production screenshot viewport.
- Canary Mesh forwarding rebuilds the fixed XP loopback URL from the authenticated request's raw
  path and query. This keeps HTTP/2 absolute-form URIs from being appended as a second origin and
  preserves the URI bytes covered by internal-auth v2; requests that the URL client would normalize
  are rejected before loopback forwarding.
- A real TLS counting proxy exercises sequential, concurrent, reconnect, idle-expiry, H1 fallback,
  invalid-ack, long-lived SSE, Raft burst, 8 MiB snapshot, and ordinary fan-out paths. The shared
  testbox resource workload runs 50 signed TLS peers against release XP binaries and samples XP/Xray
  PSS, XP CPU ticks, accepted TCP connections, active overlap, and negotiated HTTP versions.

## Validation Notes

- Unit, integration, Web checks, Impeccable detection, Storybook interaction, and controlled local
  visual validation run on this topic branch.
- Read-only directed-edge checks found no signed `health-v2` acknowledgement from the current
  Reality endpoints before the HTTP/2 loopback forwarding fix; public fallback observations are
  kept separate from Mesh active success.
- A real TLS regression sends the same signed `health-v2` request over HTTP/1.1 and HTTP/2,
  verifies the loopback path/query and validates the returned acknowledgement.
- Shared testbox real-Xray validation passed the Reality fallback suite, including repeated and
  concurrent signed Mesh requests over one external TCP connection and successful reconnect after
  an intentional disconnect.
- The 50-peer resource comparison records XP anonymous and total PSS separately. Anonymous PSS has
  an 18 MiB absolute ceiling; XP total PSS and the isolated XP-plus-Xray stack each have a 1 MiB
  regression ceiling against the locked baseline. File-backed executable pages remain included in
  total PSS. Candidate and baseline use separate Cargo target directories so build scripts and
  release artifacts cannot cross-contaminate the comparison. The separate full managed-stack 64 MiB
  target remains outside this topic's contract.
- The locked 15-minute comparison completed with 50 persistent H2 connections: TLS accepts fell
  from 921 to 50, XP total PSS from 30,660 KiB to 24,469 KiB, XP anonymous PSS from 17,624 KiB to
  12,464 KiB, the isolated stack from 53,564 KiB to 47,337 KiB, and XP CPU ticks from 1,378 to 218.
  Every candidate peer remained at one active connection with a peak of one and no non-H2 request.
- Rustls 0.23 uses the ring provider for both the server and Mesh client. Keeping one provider
  removes the unused AWS-LC implementation from the release binary while preserving TLS 1.2/1.3
  and P-256 support. ACME still carries its older HTTP/DNS dependency stack; replacing that stack is
  owned by the managed-stack memory topic rather than this transport change.

## References

- `./SPEC.md`
- `./HISTORY.md`
