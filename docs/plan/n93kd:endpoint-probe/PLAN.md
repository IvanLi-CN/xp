# Endpoint Probe（接入点可用性 / 延迟探测）

## Background

We need a cluster-wide probe mechanism to test **all endpoints** using **HTTPS requests to public fixed-content pages** (e.g. gstatic / Cloudflare), record **real latency**, and present the last **24 hourly slots** in the Admin UI.

Key constraints from requirements:

- Probe runs on **all nodes concurrently** (including self-test), using the **same probe configuration**.
- **No loopback endpoint testing**: do not probe an endpoint via `127.0.0.1` / `localhost` (even for self-test). Always use the endpoint's public `access_host`.
- Probe user must automatically have permission to use **all endpoints**.

## Goals

- Admin UI:
  - Endpoint list shows:
    - latest probe latency (ms) for a canonical target;
    - a 24-slot hourly availability bar (like `||||||||`), clickable to drill down.
  - Endpoint details page provides a **Test** button that triggers a **cluster-wide** probe of **all endpoints**.
  - A dedicated stats page (from the list click) shows last-24h summaries and per-node breakdown.
- Backend:
  - Automatic probe runs every hour, producing data for the last 24 hours.
  - Manual probe run starts probes on **all nodes at (roughly) the same time**.
  - Probe results are persisted via Raft so the UI can query from the leader.
- Probe traffic uses a dedicated **probe user** with grants to all endpoints.

## Non-Goals

- Long-term retention beyond 24 hours (future extension).
- Full-blown charting library (keep UI lightweight).
- Probing via "direct" network path without going through the endpoint proxy.

## Scope (In / Out)

### In

- Add persisted probe history per endpoint (hourly buckets, last 24).
- Add internal/system user `probe` and ensure it has grants for all endpoints.
- Implement probe runner:
  - uses Xray client-side config to create a local SOCKS proxy;
  - sends HTTPS requests through the SOCKS proxy to a fixed-content target.
- Add Admin APIs:
  - trigger probe run (cluster-wide);
  - query endpoint probe summaries & detailed history.
- Web UI changes for list + detail + stats.

### Out

- Exposing probe user credentials to UI.
- SLA / alerting / notifications (future).

## Requirements

### MUST

- All nodes run probes **concurrently**; self-test is allowed.
- All nodes use **identical probe configuration** (targets + timeouts); manual runs validate a config hash.
- No loopback endpoint testing:
  - Reject probing endpoints whose `access_host` is `localhost`/`127.0.0.1`/`::1`.
  - Self-test still uses `access_host`, not loopback.
- Probe uses HTTPS requests to public pages with fixed response:
  - Required: `https://www.gstatic.com/generate_204` (expect `204`).
  - Optional additional check: `https://www.cloudflare.com/robots.txt` (expect `200` + prefix check).
- Endpoint list shows:
  - 24 hourly availability slots;
  - latest canonical latency (ms).
- Clicking the availability bar navigates to a stats page with per-hour + per-node details.
- Probe user automatically gets grants for all endpoints.

### SHOULD

- Mark partial outages (some nodes ok, some fail) distinct from total down.
- Limit per-node probe concurrency to avoid spawning too many Xray processes at once.
- Ensure one probe run at a time per node (mutex/lock).

## Acceptance Criteria

- Given a cluster with N nodes and M endpoints,
  - When a probe run is triggered (manual or scheduled),
  - Then each node attempts to probe **every endpoint** using the same config,
  - And the leader stores the merged results (per endpoint/hour/node) with retention of 24 hours.
- Endpoint list shows a 24-slot bar where each slot reflects:
  - `unknown` when missing data,
  - `up` when all nodes succeeded,
  - `degraded` when some succeeded,
  - `down` when none succeeded.
- Endpoint details page contains a **Test now** button that triggers cluster-wide probing.
- Stats page loads for an endpoint and shows last-24h summaries + per-node results for a selected hour.
- Probing does not attempt loopback endpoint hostnames; such endpoints show probe errors instead.

## Testing

- Rust:
  - Unit tests for:
    - hourly bucket pruning (keep last 24);
    - apply/merge behavior of the new Raft command (no overwrites across nodes);
    - loopback host rejection logic.
- Web:
  - Unit tests for zod schema parsing of probe summary/history.
  - (Optional) Storybook story for the availability bar + stats page states.

## Risks / Open Questions

- Requires `xray` binary to be present on nodes for client-side probing.
  - Mitigation: clear error path when missing; keep probe runner abstracted for future alternatives.
- Mixed-version clusters may disagree on probe config; manual run should fail-fast on hash mismatch.

## Milestones

1. Backend: persisted state + Raft command + APIs for probe summaries/history.
2. Backend: probe user bootstrap + per-node probe runner + hourly scheduler + internal trigger endpoint.
3. Web UI: list columns (latency + 24-slot bar) + stats page + detail page "Test" button.
