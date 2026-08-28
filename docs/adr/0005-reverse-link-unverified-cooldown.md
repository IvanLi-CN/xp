# Quarantine unverified Reverse Links after two probes

> Status: accepted
>
> Supersedes [0004-reverse-link-liveness-lifecycle](0004-reverse-link-liveness-lifecycle.md).

The signed Link Lease decision remains valid, but the original retry schedule still rebuilt a
target-side Xray reverse outbound several times during a normal peer outage. Resource testing
showed that each rebuild can retain Xray PSS even with the managed Go memory and buffer defaults.

## Decision

Each local `ReverseLinkKey` retains the signed-health and 120-second Link Lease model from ADR
0004. Its unverified acquisition circuit is changed as follows:

- A new link receives one 10-second probe window immediately. The first missed signed health
  response removes its initiating outbound and schedules one recheck after 30 seconds.
- A second missed 10-second probe removes the outbound and opens a 15-minute cooldown. After the
  cooldown, each still-unverified link gets one 10-second half-open probe before another 15-minute
  cooldown.
- An expired active lease removes its outbound immediately, waits 30 seconds, and starts the same
  two-probe acquisition sequence. A signed health response from the exact derived Link always
  enters the active 120-second lease and clears the unverified acquisition history.
- Assignment replacement and its existing 120-second retired-handler drain remain independent: a
  failed replacement never extends the old handler's drain deadline.

The target never persists the circuit, retry counter, or cooldown in Raft. A process restart begins
with the bounded initial probe. Direct/Public and membership are not circuit outcomes.

## Consequences

For a newly unreachable link, target-side Xray installs at most two probe underlays in the first
15 minutes, then at most one per following 15-minute cooldown interval. Each installation exists
for no longer than the 10-second probe window. This bounds the lifetime of Xray retry work instead
of relying on Xray's duplicate-route error strings or reconnect behavior.

Recovery remains fast for a transient loss: the first missed health receives a 30-second recheck;
a returned signed health immediately returns the link to its active lease. The resource gate keeps
the prior limits: no accumulating SYN-SENT sockets, CPU no greater than the disabled baseline's
125% or ten additional CPU-seconds, and PSS no more than 2 MiB above the disabled baseline after
one real Reverse handler has been installed and removed to account for Xray's lazy process-wide
initialization.

## Rejected Alternatives

- Keep the 30/60/120/240/300-second schedule: it repeatedly reconstructs an outbound during the
  same outage and exceeded the PSS gate in shared-testbox measurements.
- Relax the PSS acceptance threshold: it would hide the unbounded lifetime of an unavailable
  underlay rather than make the normal peer-loss case safe.
- Persist the cooldown in Raft: retry timing is target-local transport state and must not create
  membership or topology writes.
