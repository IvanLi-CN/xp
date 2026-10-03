# Reality fallback 控制面 Mesh 与系统状态页实现状态（#56dtr）

> 有效行为以 `./SPEC.md` 为准。

## Current Status

- Implementation: the ADR 0015 directed admission, per-peer Direct/Public isolation,
  membership-bound validation receipts, and Native Reverse quarantine are implemented.
- Lifecycle: active.
- Catalog: supersedes `nbs5f`.

## Confirmed Recovery Gap

A deployed `101 -> us` Direct XHTTP/Reality request established TCP but reached TLS EOF before
the peer's signed acknowledgement. The existing real-Xray test topology did not prove this
deployment-level directed edge, so it is insufficient evidence that Direct Mesh is ready to
enable. While the durable cluster Mesh gate remains disabled, control-plane traffic uses the
registered Public Path. The remediation now enforces the ADR 0015 Direct Ingress Contract,
server-enforced all-voter re-enable preflight, bounded Direct/Public circuits, and
membership-bound validation receipts. Direct Mesh remains disabled until a separately
authorized production enablement window.

For the current release, Native Reverse is a retained diagnostic schema only: its historical
assignment, probe, and runtime notes below are not active. The production path does not select,
probe, reconcile, or dynamically install Native Reverse; existing topology is reported as
`disabled_pending_rework`.

## Delivered

- internal-auth v2、purpose-separated ack（完整 canonical request digest）与 strict bodyless
  canary ingress。
- Authentication subkeys use eight fixed process-local slots with FIFO replacement. Entries hold
  only a length-delimited digest of all three derivation inputs and the successful 32-byte key.
  PEM parsing and unchanged HKDF run outside the cache lock; poisoned locks use uncached derivation.
  Public signing/verification interfaces still validate every request and acknowledgement.
- Raft `PersistedState.mesh_enabled` provides the cluster-level Mesh switch. The authenticated
  `/api/admin/mesh/config` endpoint replicates the setting, and every process-wide Mesh client
  observes the same gate. State-machine apply and snapshot installation publish the persisted
  value immediately, before any deferred reconcile work; disabled clusters use only registered
  public HTTPS peer origins. The write barrier probes every current voter and learner using each
  target's permitted route: Direct Mesh for eligible managed endpoints and signed registered API
  requests for owner-approved private Docker voters without an endpoint. An older
  learner cannot reject the replicated command; stale learners must be upgraded or retired first.
  Mesh and Reverse requests share a read-side admission barrier and remain concurrent; gate
  transitions take the exclusive write side and wait for admitted requests to finish.
  Once the gate is authoritative, ordinary state-machine apply uses an atomic fast path and does
  not queue the write-preferring barrier; only first authenticated initialization and explicit Mesh
  switch changes take the write side. Reconcile also skips the write side when the authoritative
  value is already unchanged, regardless of an unrelated state-generation advance. Mesh admission
  and response-body guards use the caller's absolute deadline; runtime-events SSE keeps the
  three-second admission/first-byte budget but uses a fixed internal fifteen-minute body lease.
  An independent body timer releases an unconsumed response guard; lease expiry actively cancels
  the upstream stream even if its consumer stops polling. Ordinary bounded bodies retain their
  deadline timer without the streaming cancellation driver. Terminal body states
  release it without retrying a signed response through Public. Signed Mesh success
  reserves its circuit operation id at header verification and carries the half-open probe guard
  through body completion, so a late body cannot overwrite a newer protocol rejection and a second
  half-open probe cannot start while the first body is active. Breaker/validation state updates stay
  under the bounded admission guard, while persistence telemetry is deadline-aware after the guard
  is released. Critical completion state is coalesced per peer behind a bounded queue and
  active-worker window; overflow preserves the latest critical state and is serviced fairly
  alongside queued work without exceeding the 32-active bound. Empty overflow state is checked
  without taking the pending-map lock. Body completion has bounded cleanup grace when its request
  deadline has expired, while successful body
  telemetry uses bounded deferred persistence after its RAII probe guard is released. Failure,
  cancellation, and deadline telemetry remains immediate and bounded.
  An unchanged deferred breaker observation does not dirty or rewrite telemetry, while a failed
  snapshot still retries. A finite body that already reports end-of-stream releases its gate and
  finishes immediately without allocating another body wrapper or deadline task; a zero
  Content-Length alone cannot take this path. Streaming leases retain their separate lifecycle.
  Non-bootstrap nodes hold the local gate closed until the first authenticated Raft state or
  snapshot is applied, so a joining node cannot emit Mesh traffic from the default local state.
  Snapshots carry an explicit `mesh_state_applied` payload marker plus snapshot identity fields.
  Snapshot installation rejects a gate barrier that cannot be acquired within three seconds
  before replacing state. It persists a fail-closed pending marker before replacing state,
  writes data before metadata, and clears the pending marker only after both files are durable.
  Readers reject
  mismatched identity pairs and authenticated markers without identity evidence.
  Legacy startup migration only reopens the gate when that marker is true and its snapshot metadata
  exactly matches the persisted applied log; WAL-only, metadata-only, missing, malformed, or legacy
  snapshots remain fail-closed. This avoids treating a locally-built Blank/Membership snapshot as
  authenticated state while allowing purged-WAL nodes to recover from a verified snapshot.
  Modern metadata markers remain valid after later log progress: when a snapshot exists, startup
  validates the data/meta pair and authenticated marker without requiring the older snapshot
  watermark to equal the current applied log. Bootstrap startup also retries the local node
  upsert when Raft is initialized but the state machine still lacks that node, closing the
  initialization crash window; the retry is admitted only while the local node is still a current
  voter, so a learner or retired bootstrap identity cannot be resurrected. Capability
  probes for missing, invalid, ambiguous, or unsupported Mesh targets use the registered public
  origin directly, while an unsigned predecessor `404` still enters the existing legacy
  `/api/capabilities` compatibility path; these probes never use a Reverse relay assignment.
  Capability probes keep predecessor 404 compatibility over that public path, while dedicated
  Reverse health and link probes are suppressed until the gate is enabled again.
- per-peer HTTPS Mesh transport、breaker、fallback 与本地 telemetry；Raft、leader forwarding、
  node history、探针、管理 fan-out 与 SSE 共用进程级传输 bundle。托管 Mesh 使用 HTTP/2-only
  client，每 origin 最多保留一条 idle connection，idle timeout 为 120 秒；公网 direct
  fallback 使用独立的长期 client。Vision/TCP 与 XHTTP managed-default endpoint 都可生成
  Direct Mesh URL，后者使用 Reality fallback 承载既有签名 HTTP/2；Native Reverse 不再参与
  通用请求，空白 `access_host` 与歧义 endpoint 仍直接选择 Public fallback。
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

- Public authentication regressions use independent OpenSSL HKDF/HMAC and literal canonical wire
  fixtures for both routes and request/ack domains. They cover key-only and certificate-only
  changes, PEM spelling, more authorities than cache capacity, concurrent callers, malformed PEM,
  forged MACs, wrong identities, expired timestamps and mismatched acknowledgements. An opt-in
  repeated-signing seam supports external allocation tracing; its diagnostic reduction does not
  replace the unchanged release-candidate resource workload.
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
- Mesh re-enable preflight is server-bounded to 30 seconds with a bounded serialized direction
  schedule; cancellation and deadline expiry fail closed before any Raft write.
- Targeted lock-coupling regressions cover authoritative apply progress while a Mesh read guard is
  held, unchanged reconcile without a queued writer after a generation advance, queued-writer
  admission timeout with remaining-budget Public fallback and half-open release, protocol rejection
  cleanup after the request deadline, and EOF/error/drop/unpolled/deadline response-body guard
  release. They also cover delayed Mesh success versus newer protocol rejection and retain the
  half-open probe slot until a successful response body finishes. They also verify lease expiry
  drops an unpolled upstream body, snapshot gate acquisition is bounded, and queued plus overflow
  completions share the active capacity while the queue remains continuously replenished.
  Telemetry regressions also distinguish unchanged breaker observations from failed-snapshot
  retries. Body regressions distinguish a confirmed empty EOF from delayed EOF with zero
  Content-Length.
  The direct admission path
  preserves `PreDispatchTimeout` while the gate remains enabled; bounded state and telemetry
  updates retain epoch classification.
- The 50-peer resource comparison records XP anonymous and total PSS separately. Anonymous PSS has
  an 18 MiB absolute ceiling; XP total PSS and the isolated XP-plus-Xray stack each have a 1 MiB
  regression ceiling against the locked baseline. File-backed executable pages remain included in
  total PSS. Candidate and baseline use separate target directories in the owning Agent Directory;
  only Cargo's compatible download cache may be shared. Stable source markers include the source
  generated Web-shell archive and build-version identities before a target is reused. The runner
  copies the resolved executables into the disposable run before measurement, so build scripts and
  release artifacts cannot cross-contaminate the comparison. Remote build and resource-test
  temporary files use the disposable run's `tmp/` directory inside the owning Agent Directory.
  The separate full managed-stack 64
  MiB target remains outside this topic's contract.
- Candidate validation is bound to exact source commits, source and generated Web archive
  digests, and inspectable runner manifests in the delivery evidence. Historical local results
  for `392f4b2a` are recorded in `./evidence/local-checks-392f4b2a.md`; they do not validate later
  runtime changes. Signed response headers only
  authenticate the response; Mesh, Direct health preflight, and Public signed success commit
  after the body completes. Error, cancellation, and deadline outcomes release the relevant gate
  or probe without recording success, while the captured Mesh epoch prevents an old response from
  updating a newer gate generation; the captured circuit operation id also prevents an old body
  from reopening a newer protocol-isolated peer. Non-idempotent history cleanup keeps fail-closed
  `outcome_unknown` handling after an ambiguous response. The `392f4b2a` formal isolated workload
  passed after two diagnostic runs hit different shared-testbox peak boundaries; the exact
  successful run and both bounded failures are recorded in
  `./evidence/mesh-resource-392f4b2a.md`. The later `5607923d` CPU gate failure is preserved in
  `./evidence/mesh-resource-5607923d.md`. Current acceptance requires the full formal runner and
  full Rust checks on the exact committed delivery candidate; historical passes and short smoke
  runs cannot replace that evidence.
- Host-managed fresh-join and service-recovery evidence passed on the immediately preceding
  runtime candidate. The scope and non-same-SHA
  status remain recorded in `./evidence/host-managed-fresh-join-d0fe8246.md`. This supplementary
  evidence does not replace the required same-SHA shared-testbox gate.
- Rustls 0.23 uses the ring provider for both the server and Mesh client. Keeping one provider
  removes the unused AWS-LC implementation from the release binary while preserving TLS 1.2/1.3
  and P-256 support. ACME still carries its older HTTP/DNS dependency stack; replacing that stack is
  owned by the managed-stack memory topic rather than this transport change.

## References

- `./SPEC.md`
- `./HISTORY.md`
