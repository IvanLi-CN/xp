# Separate Direct Connectivity Observation from Public Fallback

Status: accepted

## Context

XP's Mesh probe can currently fall back to a peer's public `api_base_url` and record the fallback as
success. That proves only that both nodes can reach the public edge; it does not prove Direct Mesh
connectivity. The 60-second all-peer probe also keeps non-leader peer connections alive even when no
Direct Mesh path exists.

## Decision

Any probe that claims to measure Direct Connectivity uses Direct Mesh only. It never falls back to
Public Fallback. A peer without an eligible Direct Mesh path is not probed by the 60-second direct
probe; its `xp` peer connections may idle out, while the Raft leader still maintains
leader-to-follower heartbeats and elections connect on demand.

Public Fallback remains a functional transport for signed control-plane requests. A separate
five-minute public fallback probe may report fallback reachability, but it never contributes to
Direct Connectivity, Direct validation, or the active path.

## Considered Options

- Keep end-to-end fallback and label it: rejected because the Direct Connectivity metric remains
  invalid.
- Remove all probes: rejected because it removes the only direct-path availability evidence.
- Use Public Fallback as the only Mesh path: rejected because it cannot prove a direct peer path.

## Consequences

- Status and API must expose Direct Connectivity and Public Fallback as separate facts, using
  `direct_connectivity` and `public_fallback_health`; `fallback_success` remains a functional
  fallback fact.
- Functional requests can still succeed through Public Fallback; that success is end-to-end, not
  direct.
- Connection counts drop when no Direct Mesh is eligible; direct probes still use one HTTP/2
  connection per eligible peer.
