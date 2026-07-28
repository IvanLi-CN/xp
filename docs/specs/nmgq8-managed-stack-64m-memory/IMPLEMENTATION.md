# Implementation

## Status

- Implemented and released in `v3.19.9`; production rollout is pending the
  runtime-activation hotfix release and final SG preflight.

## Work areas

- Low-memory admin authentication and bounded verification concurrency.
- Go runtime defaults for host-managed and container deployments.
- Xray low-buffer static policy and upgrade backfill.
- Upgrade activation reloads systemd units and restarts both Xray and
  cloudflared before reporting success.
- PSS sampler, shared-testbox load profile, release rollout and production soak.

Implemented: low-memory administrator authentication and bounded verification,
JWT-first authorization, Xray/cloudflared runtime defaults for
systemd/OpenRC/container launches, upgrade backfill, and the PSS sampler at
`scripts/ops/sample-managed-stack-pss.sh`.

Remaining gates: build the web bundle for embedded-UI tests, run shared-testbox
load/soak, publish and verify the release, then perform the four-node token
rotation and 24-hour observation.
