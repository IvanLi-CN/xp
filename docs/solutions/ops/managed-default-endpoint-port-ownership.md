---
title: Managed default endpoint ports have one runtime authority
module: managed default endpoints
problem_type: configuration source-of-truth drift
component: startup and node metadata reconciliation
tags: [managed-endpoint, raft, bootstrap, reconciliation, xp-ops]
status: active
related_specs:
  - 3e4q4-mihomo-provider-dual-track
  - c8qtw-docker-single-image-cluster-node-deploy
---

# Managed Default Endpoint Ports Have One Runtime Authority

## Context

Managed VLESS and SS2022 endpoints can be created from host or container environment variables,
but operators can later edit those endpoints through the Admin UI/API. Startup, upgrade, and
`xp-ops xp sync-node-meta` all revisit the managed-default reconcile path.

## Symptoms

- A port edited through the Admin UI/API reverts after restart or node metadata sync.
- Xray listens on a stale local env port while the provider ingress maps a different cluster port.
- Removing a bootstrap env value unexpectedly deletes a live managed endpoint.

## Root Cause

The reconcile path treated a bootstrap input as permanent desired state after the endpoint became
cluster-managed. That created two writable authorities for one port: local env and Raft state.

## Resolution

Use `XP_DEFAULT_VLESS_PORT` and `XP_DEFAULT_SS_PORT` only when the corresponding managed endpoint
is missing. Once an endpoint exists or a single legacy endpoint is auto-adopted, preserve its Raft
port while reconciling only system-derived metadata. A changed or absent bootstrap env does not
update or delete the endpoint.

Use the Admin UI/API endpoint update and delete operations for intentional reconfiguration. If an
endpoint is explicitly deleted while its bootstrap env remains, the next reconcile creates the
missing endpoint from that bootstrap value. To intentionally rebootstrap on a different port,
change the env first, explicitly delete the endpoint, then restart or run node metadata sync.

## Guardrails

- Apply the same ownership rule to host startup, `xp-ops xp sync-node-meta`, and container startup.
- Preserve a legacy endpoint's port during auto-adoption.
- Keep ambiguity checks when more than one same-kind endpoint could be adopted.
- Port ownership does not make managed REALITY fields operator-controlled; `reality.dest`, SNI,
  and canary readiness remain system-managed.
- Regression tests must cover stale env versus cluster port, missing-endpoint bootstrap, Admin API
  port edits across reconcile, and SS2022 parity when the shared reconciler handles it.

## References

- `src/managed_default_endpoints.rs`
- `src/ops/container_managed_default.rs`
- `src/http/tests.rs`
- `docs/specs/3e4q4-mihomo-provider-dual-track/SPEC.md`
- `docs/specs/c8qtw-docker-single-image-cluster-node-deploy/SPEC.md`
- `docs/ops/README.md`
