# Retired Control-Plane Proxy Contract

This topic is retired. The local control-plane proxy, its configuration, static Xray listener,
runtime status, clients, compatibility behavior, and tests have been removed.

Reality Mesh and the peer public HTTPS origin remain supported peer-direct paths for control-plane
callers. Repository history synchronization is governed by the active cluster-history contract and
uses the target node's public HTTPS `api_base_url` only; it does not use a dynamic relay.

## Related ADRs

None

## References

- `../reality-fallback-control-plane-mesh/SPEC.md`
- `../cluster-history-repositories/SPEC.md`
