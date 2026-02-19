# Ops: Endpoint probe (availability / latency)

`xp` runs a cluster-wide endpoint probe to measure **reachability** and **latency** for every ingress endpoint.
Results are recorded per-hour (UTC) and surfaced in the Admin UI (last 24 hours).

This document is an ops-oriented companion to the frozen spec:

- `docs/plan/n93kd:endpoint-probe/PLAN.md`

## What is being tested

For each endpoint, each node runs a probe runner that:

1. Builds an Xray client config for the endpoint (VLESS REALITY / SS2022).
2. Brings up a local SOCKS proxy (ephemeral port on `127.0.0.1`).
3. Sends HTTPS requests **through the endpoint path** to fixed public targets.

Required target (canonical latency):

- `https://www.gstatic.com/generate_204` (expects HTTP `204`)

Optional target:

- `https://www.cloudflare.com/robots.txt` (expects HTTP `200` + body prefix check)

The endpoint latency shown in UI is derived from the **first required target** (stable and comparable).

## Non-negotiable semantics (do NOT "fix" by cheating)

The probe is meant to test the _real_ ingress path. In particular:

- Self-test still uses `access_host` (no loopback special-casing).
- If `access_host` is `localhost` / `127.0.0.1` / `::1`, the probe must be rejected (record an error; do not send requests).

Operationally, this also means:

- Do NOT override `access_host` resolution to loopback via `/etc/hosts` or split DNS.
  - Example of a disallowed workaround: `127.0.0.1 <access_host>`.
  - Doing this makes the probe pass even when the real ingress path is broken, which defeats the purpose.

If a node cannot reach its own `access_host` due to provider/network restrictions (no hairpin), fix the
network/ingress design instead (see below).

## Interpreting common errors

### `required targets failed: gstatic-204`

Meaning:

- The HTTPS request to `https://www.gstatic.com/generate_204` failed _through the endpoint_.

This can be caused by:

- Endpoint is not reachable (routing / firewall / port blocked).
- Xray inbound on the endpoint node is missing / stale / misconfigured.
- Egress from the probe runner node is blocked (or DNS returns an unreachable edge IP).

### `missing`

Meaning:

- No samples were recorded for the given hour bucket / endpoint / node.

Typical causes:

- The node runner was down during that hour.
- The cluster was mid-upgrade or temporarily partitioned.

## Troubleshooting checklist

1. Confirm whether the failure is isolated to one node (degraded) or all nodes (down):

- Admin UI: endpoint stats page shows per-node results and the exact error string.

2. Validate the required target from the failing node (direct egress sanity check):

```
curl -sS -o /dev/null -w "status=%{http_code} time=%{time_total}\n" \
  https://www.gstatic.com/generate_204
```

If the direct request times out, check:

- DNS resolution for `www.gstatic.com` on that node (some resolvers may return unreachable edge IPs).
- General egress connectivity (firewall / routing / upstream).

3. Validate that the node can reach its own ingress host _without_ loopback overrides:

- The node must be able to reach `access_host:<listen_port>` using the real network path.
- If the provider blocks hairpin to the node's own public IP, prefer using an ingress that is reachable
  from inside the node as well (for example, Cloudflare Tunnel or an external reverse proxy).

Do NOT "solve" this by mapping `access_host` to `127.0.0.1` locally.
