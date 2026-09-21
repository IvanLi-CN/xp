# Reality Mesh 反向中继实现状态

## Current Status

- Implementation: retained for diagnosis, but Native Reverse runtime is quarantined and cannot
  route, probe, reconcile, or create Xray dynamic artifacts.
- Lifecycle: dormant pending rework.
- Delivery stop: production re-enable is not part of this release.

The historical implementation notes below document retained state and prior evidence only. They
do not authorize Reverse probes, assignment writes, Xray dynamic artifact changes, relay serving,
or automatic restarts in the current release.

## Delivered

- 主题分支 `th/reality-mesh-reverse-relay` 从锁定基线创建。
- assignment、wire、生命周期、Xray dynamic API and additive status are implemented. The durable
  state barrier is schema v13, and the coordinator refuses to write the first epoch until every
  voter advertises the assignment capability and at least one candidate reports signed Xray
  readiness plus a managed VLESS endpoint.
- History repository direct sync uses the target node's public HTTPS `api_base_url` only; it never
  probes or opens the assigned Reverse route or the legacy encrypted dynamic relay. Public failures
  leave the durable checkpoint or outbox for the next bounded retry.
  Control-plane fallback URL handling preserves the registered public endpoint for non-history
  callers while keeping this history-specific HTTPS-only boundary explicit.
- Fresh joins and join retries return no `reverse_mesh_bootstrap` marker while Native Reverse is
  quarantined. They do not probe voters or write Reverse epoch/assignment state; existing durable
  assignment data remains untouched for diagnosis and future rework.
- Assignment reconciliation, Reverse probes, Xray dynamic artifact creation/removal, and relay
  serving are disabled in this release. Normal XP reconciliation does not touch dormant Reverse
  state or request an Xray restart; only the separately authorized maintenance upgrade path may
  perform one-time cleanup and restart verification.
- Mesh and Reverse sends use shared read admission, while cluster gate transitions take an
  exclusive write barrier so normal reverse concurrency is preserved without crossing a disable
  boundary.
- Successful Mesh telemetry reuses the response's existing read admission instead of reacquiring
  the write-preferring lock; protocol and transport failures release the response and admission
  before recording telemetry. Inbound signed Reverse health holds the same admission through lease
  confirmation, so a queued gate disable cannot cross either lifecycle boundary.
- Target-side Reverse lifecycle is local and fail-closed. Each derived Link starts with one
  10-second Xray probe underlay and asks its exact Rendezvous for a signed return health request.
  The target grants a 120-second lease only when the request's assigned Rendezvous identity and
  signed relay envelope match the Link's derived authority; Link headers on direct health cannot
  extend a lease. One missed probe receives one 30-second recheck; two consecutive missed probes
  enter a 15-minute cooldown, after which one half-open probe is permitted per cooldown. Lease
  expiry begins the same bounded acquisition sequence after a 30-second wait. A retired handler
  gets one fixed 120-second drain deadline, even when its replacement never becomes healthy.
  The historical `XP_REVERSE_MESH_ENABLED=false` path is superseded by the compile-time quarantine;
  current normal reconciliation leaves dormant Reverse artifacts untouched.
- The unreachable-Rendezvous resource fixture first installs and removes one real Reverse outbound,
  then keeps Reverse disabled during its baseline phase and includes a loopback-only managed
  VLESS/REALITY inbound. This warms Xray's lazy native Reverse handler once, so the unchanged 2 MiB
  PSS gate measures repeated unavailable-Link lifecycle rather than a process-wide cold-start
  allocation; the test also proves that the prewarm outbound is gone before sampling.
- Reverse route reconciliation now reads existing Xray rule tags before adding desired routes.
  The only duplicate-response compatibility branch accepts `app/router: duplicate ruleTag` for
  the exact desired tag.
- Reverse outer requests now use a fixed eight-request in-flight budget per Rendezvous on the
  process's shared control-plane client. Ordinary requests may use seven slots while one remains
  available to signed health probes; the budget is shared by cloned Mesh clients, rejects excess
  requests before opening a new underlay stream, and binds its permit to a guarded response
  body/stream until that stream is consumed or dropped. Direct/Public fallback, assignments, and
  Link leases are unchanged.
- Fresh-join bootstrap links use the domain-separated `ReverseRole::Bootstrap` tag/UUID/origin
  while the durable join operation is active. Both Rendezvous and the learner switch to the
  formal Primary/Standby derivation only after the operation reaches a terminal phase; stale
  bootstrap users/rules then drain for 120 seconds under the same Xray reconciler.
- Mesh status now adds the active Rendezvous role plus the primary and standby members to
  `active_route`. System Status resolves those IDs to current member names and limits every peer
  status cell to at most two single-line summaries. It derives the primary/standby label for each
  direct Rendezvous from current assignments, gives each Reverse target separate `Reverse relay`
  and active Rendezvous/generation lines, counts the local node with all remote members, and
  retains the existing Details entry for the full diagnostic path.
- Reverse XHTTP outbounds now set an XP-owned XMUX `max_connections=2`. Existing underlays are
  reused, a third socket is never requested by the generated config, and the existing
  Direct/Public fallback remains responsible for failed control-plane requests.
- Mesh status now samples established local TCP sockets and joins them with current node egress
  probes and Reverse assignments. `peers[].reverse_underlay` reports logical Links separately
  from physical sockets, while `local.connection_usage.user_inbound` separates external users,
  known cluster peers, and unknown sources. Egress addresses are ignored after the one-hour
  probe freshness window; a shared address is unknown rather than attributed to either node.
  Reverse sockets are removed from user-inbound totals, and source details are capped at 128
  aggregate addresses with an explicit truncation flag. Unsupported `/proc` inspection returns
  nullable counters so the UI cannot render unavailable data as zero. The System Status page
  exposes these as separate internal/external sections and keeps source addresses behind an
  administrator disclosure.
- Socket inspection is line-streamed and fail-closed above the bounded 4096-record inspection
  limit; quota sampling filters by configured listen ports before counting. If any current node
  lacks a fresh, valid egress identity, inbound sources remain `unknown` rather than being
  promoted to `external`. Aggregate Reverse state gives precedence to unavailable/unknown data
  over an over-limit sub-link. During a 120-second generation drain, the socket view is an
  endpoint-level diagnostic count and must not be interpreted as a generation-specific counter.
- Fixed Xray spike: Xray `26.3.27`, image digest
  `sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7`. Two Xray
  instances completed dynamic VLESS Reverse registration over both Vision TCP + Reality and
  XHTTP + Reality. The test then proved password SOCKS5, SOCKS-to-Axum H2C prior-knowledge,
  eight concurrent XHTTP requests, `<=2` established target underlays, exact-origin routing,
  unmatched block and rule/outbound removal isolation. The test-only SOCKS listener is mapped to
  a host loopback port because the Rust test runs outside the Xray containers; production remains
  fixed at `127.0.0.1:10086` with no public listener.
- The spike is a transport/protocol/resource gate only. It does not yet prove asymmetric firewall
  behavior, signed end-to-end health, fresh-join bootstrap, or deployment restart recovery.
  The managed-stack memory budget remains a closed integration gate before writing a
  production epoch.

## Validation

已完成的本地门禁包括 `cargo fmt --all`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、
反向 assignment/wire/lifecycle 单测、Web typecheck/lint/unit/build，以及固定 Xray spike。完整
`cargo test`、非对称双节点 Reality transport、fresh join、三种部署升级/回滚、managed-stack
内存 soak、Storybook/E2E、spec drift、独立 review 和 required CI 仍是 Step 5C 的收口条件。
此文件不替代部署真相；部署行为同步到 `docs/ops/**` 与 `AGENTS.md`。
