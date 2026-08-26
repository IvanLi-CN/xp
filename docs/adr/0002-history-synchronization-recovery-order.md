# Keep peer direct paths equal before history repair relays

History synchronization selects Reality Mesh and Cloudflare Tunnel as equal
peer-direct paths, then tries the Raft-assigned Reality Mesh Reverse relay, and
only then the in-memory encrypted dynamic relay. The two direct paths have
independent availability and neither is a fallback for the other; Reverse adds
a routed dependency and therefore follows both, while the dynamic relay remains
the low-frequency last resort. A temporary delivery failure or bounded outbox is
Recoverable Backlog, not a Permanent Gap: a Source or any ready History
Repository may repair the original cursor range before a Permanent Gap is
declared.

## Considered options

- Treating Cloudflare Tunnel as a fallback for Reality Mesh: rejected because
  both are direct node-to-node paths with independent health.
- Skipping Reverse for history: rejected because it leaves NAT-only targets
  dependent on the low-frequency dynamic relay.
- Declaring a gap when a source outbox fills: rejected because delivery pressure
  does not prove the underlying source history is unavailable.
