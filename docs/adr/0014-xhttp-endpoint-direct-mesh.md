# Prefer XHTTP-Endpoint Direct Mesh and quarantine Native Reverse

Status: accepted

XP will derive Direct Mesh from a node's unique managed-default VLESS/Reality endpoint with a
valid `access_host`, including an XHTTP endpoint. The control plane continues to use its shared,
signed HTTP/2 client through the endpoint's Reality fallback; it does not create a VLESS/XHTTP
overlay or share a user session. This was chosen because real Xray validation proved the same
HTTP/2 reuse and reconnect contract as Vision/TCP without adding an XP-local proxy, credentials or
listener.

Native Reverse is retained as dormant diagnostic code and inert persisted topology, but cannot
reconcile, probe, create Xray artifacts or serve control-plane requests. All non-history
control-plane calls use Direct Mesh before the existing bounded public HTTPS fallback; History
Repository transport remains public HTTPS-only. Re-enabling Native Reverse requires a future
specification, physical-underlay closure evidence and a separate release. Automatic state cleanup
is rejected because it adds Raft migration risk without improving current safety. The release
maintenance window performs one controlled Xray restart after XP-owned Reverse-tag cleanup and
accepts the upgrade only after matching Reverse sockets are gone; XP does not restart Xray during
ordinary runtime.

## Considered Options

- A VLESS/XHTTP overlay from XP was rejected because it recreates the dynamic-Xray lifecycle and
  underlay-budget risks that invalidated Native Reverse.
- Keeping Native Reverse as an automatic fallback was rejected after production observed physical
  underlays exceeding its configured budget and dynamic removal that did not close established
  sockets.
- Public-only routing was rejected because it makes an external endpoint the primary dependency
  rather than the bounded fallback after an eligible Direct Mesh attempt.

## Consequences

- Mesh status distinguishes `vision_tcp` from `xhttp_reality_fallback`, while user inbound and
  control-plane connection accounting remain separate.
- Direct transport failure follows the existing finite budget and breaker before public fallback;
  authentication, acknowledgement and protocol failures remain terminal.
- Delivery requires real-Xray and actual-XP weak-network evidence for Raft, snapshots, SSE,
  reconnects, fallback safety and dormant-Native-Reverse invariants.
