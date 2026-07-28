# Implementation

## Status

- Authentication, runtime defaults, upgrade activation, OpenRC compatibility,
  and the initial release-footprint changes are released through `v3.20.1`.
  SG runs `v3.20.1` with the rotated administrator PHC, but its aggregate PSS
  preflight reached `71,356 KiB`; rollout remains stopped at the first follower
  while the XP executable-code footprint is reduced further.

## Work areas

- Low-memory admin authentication and bounded verification concurrency.
- Go runtime defaults for host-managed and container deployments.
- Xray low-buffer static policy and upgrade backfill.
- Upgrade activation reloads systemd units and restarts both Xray and
  cloudflared before reporting success.
- PSS sampler, shared-testbox load profile, release rollout and production soak.
- Build-time compression and HTTP negotiation for embedded Web assets.

Implemented: low-memory administrator authentication and bounded verification,
JWT-first authorization, Xray/cloudflared runtime defaults for
systemd/OpenRC/container launches, upgrade backfill, and the PSS sampler at
`scripts/ops/sample-managed-stack-pss.sh`.

Remaining gates: release and measure the size-oriented XP build, pass the SG
hard gate, roll the same release and PHC across the other three nodes, run the
load/soak gates, and complete the 24-hour production observation. The shared
testbox run is blocked until space is available under its managed workspace.
