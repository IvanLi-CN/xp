# Classify voter capability into three states and persist signed evidence

Status: accepted

## Context

Membership lifecycle writes require every retained voter to expose
`cluster.membership-lifecycle-v1`. The barrier evaluates that requirement with one live signed
probe per voter and fails the whole operation on any non-supporting result.

That conflates two different facts: "this voter answered and lacks the capability" and "this voter
did not answer at all". A permanently decommissioned server cannot answer, yet it also cannot
contribute a mixed-version state-machine command, so treating it as an upgrade blocker is wrong.
Because the only escape hatch (`evict-unreachable-voter`) excludes exactly one target from the
barrier, two simultaneously unreachable voters deadlock: removing either requires the other to
answer.

## Decision

The barrier classifies every retained voter into one of three states.

- `Supported` — a verified signed probe response advertises the capability, or persisted signed
  evidence is newer than 24 hours and advertises it.
- `Unsupported` — a verified signed response omits the capability, or an unsigned predecessor
  `404` proves the voter predates the signed route. This is terminal and is never overridable.
- `Unreachable` — no verified signed XP response arrived (transport failure, timeout, TLS error,
  Cloudflare `530`, or a response without a signed acknowledgement).

Nodes persist a signed capability report (version, capability list, timestamp) into Raft through
the existing `CompatNoop` note channel, so no new state-machine command variant and no
`desired_state.Node` field is introduced. Only `Unreachable` may use persisted evidence, and only
while that evidence is fresh.

`xp-ops xp evict-unreachable-voter` accepts additional `--confirmed-unreachable <id>` targets. The
leader re-probes each one itself and admits it only when the classification is `Unreachable`; an
`Unsupported` result rejects the whole request. Each operation still removes exactly one voter and
still requires the resulting voter count to remain at least three.

## Considered Options

- Persist capabilities in `desired_state.Node`: rejected because it forces a field into a struct
  with wide literal usage and mutates a domain record whose meaning is operator intent, not node
  runtime fact.
- Accept an operator-declared unreachable set without re-probing: rejected because it would let a
  reachable but downgraded voter be removed and would bypass the upgrade barrier.
- Add a new state-machine command variant for reports: rejected because mixed-version voters cannot
  decode new variants, which is the exact risk the barrier protects against.
- Allow bulk removal in one operation: rejected because a partial failure would leave an
  uninterpretable membership shape.

## Consequences

- Two unreachable voters no longer block each other, while a reachable but downgraded voter still
  blocks every lifecycle write.
- Reports older than 24 hours stop granting support, so stale evidence cannot mask a rollback.
- Old binaries read the report note as a no-op and ignore the new snapshot field; downgrade
  degrades to the previous behaviour instead of relaxing the barrier.
- `node_versions` can answer for unreachable nodes from the same evidence.
- The eviction authorization reason, including excluded targets, is recorded in operation evidence
  rather than as a new structured field.
