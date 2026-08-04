# Implementation

## Status

- Authentication, runtime defaults, upgrade activation, OpenRC compatibility,
  and XP release-footprint changes are released through `v3.21.0`.
- The 2026-08-04 SG production comparison isolated cloudflared transport as the
  CPU cause: QUIC with `8MiB` used about `7.70%` CPU and `8.15` GC/s, while
  HTTP/2 with `8MiB` used about `0.90%` CPU and `0.85` GC/s. An HTTP/2
  `10MiB` run showed no measurable CPU or GC benefit. The managed default is
  therefore HTTP/2 with `12MiB`, retaining explicit overrides.
- The current HK, SG, and JP production process trees exceed the `65,536 KiB`
  aggregate gate. The earlier SG `8MiB` 60-second preflight at `63,632 KiB`
  remains historical evidence, not a current passing result.

## Work areas

- Low-memory admin authentication and bounded verification concurrency.
- Go runtime defaults for host-managed and container deployments.
- cloudflared HTTP/2 transport default with an explicit operator override for
  networks that block outbound QUIC/7844, including reload and restart when a
  managed service definition changes.
- Joined Docker/Compose administrator-token reconciliation from an explicitly
  configured low-memory PHC before the XP child starts.
- Xray low-buffer static policy and upgrade backfill.
- Runtime-default backfill recognizes systemd drop-ins and EnvironmentFile
  overrides, including explicit variable/all-environment resets and readable
  non-regular inputs; only complete XP-generated legacy service templates are
  eligible for the `8MiB` to `12MiB` migration.
- Upgrade activation reloads systemd units and restarts both Xray and
  cloudflared before reporting success.
- Host-managed upgrades replace `xp` and managed runtime assets before the optional
  `xp-ops` self-update, so a self-update cannot terminate the locked release phase early.
- Upgrade activation waits for systemd/OpenRC service readiness after each restart and rolls back
  rather than treating an asynchronous transition as success.
- PSS sampler, shared-testbox load profile, release rollout and production soak.
- Build-time compression and HTTP negotiation for embedded Web assets.
- Pinned low-memory Go runtime assets shared by host upgrades and the official
  container image, with paired checksum and rollback behavior.

Implemented: low-memory administrator authentication and bounded verification,
JWT-first authorization, Xray/cloudflared runtime defaults for
systemd/OpenRC/container launches, upgrade backfill, and the PSS sampler at
`scripts/ops/sample-managed-stack-pss.sh`.

Remaining gates: restore the aggregate PSS budget, pass the 15-minute load
gate, roll the same release and PHC across the remaining nodes, and complete
the 24-hour production observation.
The shared testbox run is blocked until space is available under its managed
workspace.
