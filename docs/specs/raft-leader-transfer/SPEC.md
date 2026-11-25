# Raft leader election handoff

## Context and Scope

- Context: XP needs an explicit way to move Raft leadership to a chosen voter without editing Raft
  files or forcing an uncontrolled election by stopping the current leader.
- In scope: a signed local election request, the matching `xp-ops xp transfer-leader` command, and
  operator documentation.
- Out of scope: target-directed OpenRaft transfer (not provided by OpenRaft 0.9), Web actions,
  bulk election, membership changes and automatic retries.

## Terms and Interfaces

- **Leader Election Handoff**: a request that makes one current voter campaign immediately and waits
  until that voter reports itself as leader.
- Interface: signed internal `POST /api/admin/_internal/raft/elect` on the target node.
- Interface: `xp-ops xp transfer-leader --api-base-url <origin> [--data-dir <path>]`.

## Requirements

### REQ-LEADER-HANDOFF-001

- The request MUST be accepted only on a node that is a current Raft voter.
- The request MUST be rejected while the local membership is in joint consensus, while an active
  membership operation exists, or while the local applied log is behind its last log index.
- Inputs: local signed internal request with no body.
- Outputs: the local node becomes leader, or the request fails without changing membership or data.

### REQ-LEADER-HANDOFF-002

- The node MUST trigger a local OpenRaft election and wait up to 30 seconds for its own
  `state=leader` and `current_leader=local` metrics.
- No membership operation, snapshot edit, raw Raft file write, retry loop, or fallback election on a
  different node is permitted.
- On timeout or an invalid election result the command returns a nonzero result and does not claim
  success.

### REQ-LEADER-HANDOFF-003

- `xp-ops xp transfer-leader` MUST run on the intended target, use the target's local cluster CA
  identity to sign the request, and print the resulting cluster identity.
- The command MUST accept an explicit local API origin and `XP_DATA_DIR`; it MUST NOT require an
  administrator Bearer token.
- A target running a version without the signed route MUST fail closed and instruct the operator to
  upgrade that target.

### REQ-LEADER-HANDOFF-004

- The operations documentation MUST state the target-local invocation, preconditions, timeout and
  failure semantics.
- An old target without the route, a non-voter, joint consensus, an active membership operation or
  unapplied log MUST be reported as an unsupported/refused handoff, never as success.

## Verification

### VER-LEADER-HANDOFF-001

- Method: signed internal route integration/unit test with an eligible voter and an ineligible
  target matrix.
- covers: `REQ-LEADER-HANDOFF-001`
- Pass condition: eligible request dispatches one local election; ineligible requests write no
  membership or Raft state.

### VER-LEADER-HANDOFF-002

- Method: local election facade test with a non-leader target and timeout/failure fixture.
- covers: `REQ-LEADER-HANDOFF-002`
- Pass condition: success requires observed local leader metrics; timeout returns an error.

### VER-LEADER-HANDOFF-003

- Method: `xp-ops` preflight/CLI test and signed request parser test.
- covers: `REQ-LEADER-HANDOFF-003`
- Pass condition: command builds a signed request against the local origin and does not read an
  admin token.

### VER-LEADER-HANDOFF-004

- Method: docs review plus unsupported-version/route 404 fixture.
- covers: `REQ-LEADER-HANDOFF-004`
- Pass condition: operator copy distinguishes target-local usage, refusal and timeout.

## Related ADRs

None

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
