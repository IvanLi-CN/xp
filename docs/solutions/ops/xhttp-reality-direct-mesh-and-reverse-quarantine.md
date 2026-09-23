---
title: XHTTP Reality Direct Mesh and Native Reverse quarantine
module: control-plane-mesh
problem_type: reliability-and-resource-safety
component: XP control-plane transport
tags:
  - xhttp
  - reality
  - direct-mesh
  - native-reverse
status: active
related_specs:
  - docs/specs/56dtr-reality-fallback-control-plane-mesh/SPEC.md
  - docs/specs/reality-mesh-reverse-relay/SPEC.md
---

# XHTTP Reality Direct Mesh and Native Reverse quarantine

## Context

XP managed-default VLESS endpoints default to XHTTP, while the control plane already
uses a signed HTTP/2 protocol. A previous policy rejected XHTTP as a Mesh target and
allowed generic requests to enter Native Reverse, whose Xray dynamic underlays could
remain established after route removal.

## Symptoms

- A node has one valid managed XHTTP/Reality endpoint but its Mesh target is reported as
  unsupported and traffic uses the public path.
- A deployed peer accepts TCP on its managed XHTTP/Reality port but returns TLS EOF before the
  signed control-plane acknowledgement.
- Repeated public fallback requests continue after an unsigned edge response and inflate Raft
  retry, CPU, and socket counts.
- Generic control-plane requests create or select Native Reverse relay routes.
- Removing a Reverse outbound reports success while an established physical socket remains.
- User-facing connection counts mix internal Reverse underlays with external user sessions.

## Root cause

XHTTP was incorrectly treated as a user-session transport requirement for control-plane
traffic. The control plane does not need to speak VLESS/XHTTP: it can send its existing
signed HTTP/2 request through the endpoint's Reality fallback. Production TCP reachability alone
did not prove that the deployed Reality-to-canary path completed TLS and returned an ACK. Native
Reverse also lacked an enforceable proof that dynamic removal closed every physical underlay.

## Resolution

- Accept exactly one managed-default VLESS/Reality endpoint with a valid access host as a
  Direct Mesh target. Expose `vision_tcp` or `xhttp_reality_fallback` as an explicit status
  field.
- Route generic control-plane requests Direct Mesh first, then bounded Public HTTPS. Only
  transport setup failures may fall back; signed ACK, authentication, protocol, and
  response-start failures remain terminal. History Repository traffic stays public-only.
- Require all-voter directed Direct-only `health-v2` validation before enabling the durable Mesh
  gate. Keep a five-minute validation freshness window and move changed or restarted peers to
  Public-only until they prove the path again.
- Keep a separate Public circuit for unsigned/invalid acknowledgements and exhausted transport
  retries. Use one half-open health request per `30/60/120/240/300s` cooldown so Raft cannot turn
  a failed edge into an unbounded connection loop.
- Keep Native Reverse assignment and epoch data read-only for diagnosis, but mark it
  `disabled_pending_rework`. Do not reconcile, probe, create Xray dynamic artifacts, or
  route requests through it.
- Clear legacy Reverse artifacts during the existing owner-authorized upgrade maintenance
  restart of Xray, then verify matching Reverse sockets are zero. XP runtime must not
  restart Xray merely to recover Native Reverse.

## Guardrails / Reuse notes

- Do not implement Direct Mesh as a VLESS/XHTTP overlay or reuse a user's XHTTP session.
- Do not infer Reverse health from `RemoveOutbound` success; socket closure needs a process
  restart boundary or an equivalent measured close proof.
- Keep internal Reverse underlay accounting separate from external user inbound accounting.
- Validate Vision and XHTTP with real Xray, then validate the actual XP control plane under
  packet loss, delay, half-open connections, signed rejection, and public fallback.
- A successful TCP connect, local canary readiness, or public-path success is not Direct proof;
  the evidence must show the signed HTTP/2 ACK on the actual directed edge.
- Re-enable Native Reverse only through a new specification, physical-underlay closure
  evidence, bounded resource proof, and a new release.

## References

- `docs/adr/0014-xhttp-endpoint-direct-mesh.md`
- `docs/specs/56dtr-reality-fallback-control-plane-mesh/SPEC.md`
- `docs/specs/reality-mesh-reverse-relay/SPEC.md`
- `tests/xray_mesh_transport_e2e.rs`
