# Bind target Reverse underlays to signed link liveness

Raft Reverse assignments describe durable topology. They do not prove that an assigned
Rendezvous is reachable from its target, nor do they bound Xray's retry behavior after that
Rendezvous is lost. Treating every assigned target outbound as permanently installed allowed a
normal peer outage to retain an unbounded retry source.

## Decision

Each target-side Reverse underlay has an in-memory `ReverseLinkKey` composed of epoch, target,
Rendezvous, role, and generation. A target installs its Xray initiating outbound only while that
link is in a bounded probe or active lease state.

- A new or retried link receives one 10-second probe window. The target asks its exact assigned
  Rendezvous to return a bodyless signed health request through that same link.
- A returned health request proves the assigned Rendezvous, signed relay envelope and derived
  reverse authority at the target, then grants a 120-second Link Lease. Link headers on ordinary
  direct health requests cannot extend that lease.
- A probe timeout or expired lease immediately removes that link's target outbounds and follows
  the local 30/60/120/240/300-second retry schedule. This state is neither Raft data nor a
  membership signal.
- During an assignment replacement, an old handler receives one fixed 120-second drain deadline
  when it is first retired. A failed replacement never renews or indefinitely delays that deadline.
- `XP_REVERSE_MESH_ENABLED=false` is an operator-local fail-closed switch. It removes all
  XP-owned Reverse Xray artifacts on that node and closes the Reverse forwarding gate without
  changing Raft assignments, Direct/Public behavior, or membership.

Xray rule reconciliation reads the current rule-tag set and skips already-present desired rules.
The compatibility fallback accepts only Xray's `app/router: duplicate ruleTag` response when it
names the exact desired tag.

## Consequences

An unreachable peer is a normal bounded-degradation input rather than a process-level fault.
Direct and Public paths remain available while Reverse is absent. The target owns its transient
resource limit and recovery attempt, while the Rendezvous retains its separate forwarding-health
gate. Restarting XP deliberately returns links to the bounded probe state.

The release requires shared-testbox evidence for an unreachable Rendezvous before a production
epoch is enabled. A package-only upgrade cannot substitute for that evidence.

## Rejected Alternatives

- Keep assigned Xray outbounds installed and gate only forwarding: this leaves Xray retries live
  while the peer is unreachable.
- Replicate Link Lease or retry counters through Raft: liveness is local and high-churn; it would
  couple transient transport loss to durable topology and membership writes.
- Treat duplicate-rule error text as the primary idempotency mechanism: Xray's listed state is
  authoritative, while the exact-message fallback is only version compatibility.
