# Implementation

## Status

- Authentication, runtime defaults, upgrade activation, OpenRC compatibility,
  and XP release-footprint changes are released through `v3.21.0`.
- On SG, pinned Xray/cloudflared builds with Go inlining disabled plus the
  `8MiB` cloudflared profile passed a 60-second preflight at `63,632 KiB` peak
  (`xp=15,500`, `xray=21,920`, `cloudflared=26,424`). These binaries remain a
  production canary while the remaining nodes complete their formal rollout.

## Work areas

- Low-memory admin authentication and bounded verification concurrency.
- Go runtime defaults for host-managed and container deployments.
- cloudflared HTTP/2 transport default with an explicit operator override for
  networks that block outbound QUIC/7844, including reload and restart when a
  managed service definition changes.
- Xray low-buffer static policy and upgrade backfill.
- Upgrade activation reloads systemd units and restarts both Xray and
  cloudflared before reporting success.
- PSS sampler, shared-testbox load profile, release rollout and production soak.
- Build-time compression and HTTP negotiation for embedded Web assets.
- Pinned low-memory Go runtime assets shared by host upgrades and the official
  container image, with paired checksum and rollback behavior.

Implemented: low-memory administrator authentication and bounded verification,
JWT-first authorization, Xray/cloudflared runtime defaults for
systemd/OpenRC/container launches, upgrade backfill, and the PSS sampler at
`scripts/ops/sample-managed-stack-pss.sh`.

Remaining gates: pass the 15-minute load gate, roll the same release and PHC
across the remaining nodes, and complete the 24-hour production observation.
The shared testbox run is blocked until space is available under its managed
workspace.
