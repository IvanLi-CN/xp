# Raft membership lifecycle invariant (#7mvqp)

## Background

An earlier guard treated every member outside `voter_ids` as a stable voter candidate and called
`add_voters`. That classification was invalid: OpenRaft has exactly three relevant roles,
`voter`, `learner`, and `absent`. A learner is a transient replication state, not a fourth
"non-voter" role that can safely be promoted by a timer. The old guard could turn stale member
metadata into an orphan voter without a DesiredState Node mapping.

## Goals

- Persist one auditable membership operation at a time for join, restore, node removal, and orphan
  voter repair.
- Check a quorum-backed, linearizable membership view before a lifecycle write.
- Preserve a strict mapping from every voter to exactly one DesiredState Node.
- Permit learners only while the recorded join or restore operation owns their transition.
- Make an abnormal member shape visible and block further unrelated membership writes; never infer
  a promotion, deletion, rollback, or disk rewrite from a periodic scan.
- Permit one exact, leader-local orphan voter repair after dry-run fingerprint confirmation.

## Roles and invariants

- `voter`: Raft voting member with a DesiredState Node mapping; valid in steady state.
- `learner`: replication-only member owned by an active Join or Restore operation; never steady.
- `absent`: no Raft membership identity; valid for deleted and repaired targets.

- Every voter maps to one DesiredState Node. A voter without that mapping is an orphan voter.
- A learner is valid only for the active operation's target during its recorded learner phase.
- A DesiredState Node without a Raft member is valid only for the target of a recorded removal
  after membership has become absent.
- Operations advance monotonically through `prepared`, learner/voter/removal phases, and a terminal
  `completed`, `blocked`, or `expired` state. Terminal records include evidence and are retained for
  24 hours. A second non-terminal operation is rejected.
- The membership revision is a SHA-256 fingerprint of the linearizable membership log identity,
  voters, and node metadata. It is an opaque compare-and-swap value, not a user-editable setting.

## Lifecycle contract

- A fresh join passes `cluster.membership-lifecycle-v1` on every current voter before the first
  lifecycle command. It records Join intent, registers the learner, waits for the durable log
  index, promotes only that recorded learner, then terminally records completion.
- Delete records its endpoint snapshot and RemoveNode intent, requires the mapped target to be a
  voter, uses `RemoveVoters(..., false)`, verifies `absent`, and only then deletes DesiredState
  Node/endpoints. It never performs a compensating re-add on an unknown result.
- The periodic worker only audits invariants, prunes terminal records after their retention period,
  and resumes a recorded RemoveNode or Restore operation. The Join coordinator advances only a
  matching recorded Join operation. None of them calls `add_voters` for an unknown session, deletes
  an unknown member, or uses a speculative rollback.
- Any retained DesiredState-mapped voter lacking `cluster.membership-lifecycle-v1` freezes fresh
  join, delete, restore, and repair lifecycle writes with `coordinated_upgrade_required`.
  Capability reads use signed Mesh transport first, with the normal public origin only as a
  compatibility fallback; public API availability is not a separate lifecycle prerequisite.
  Upgrade one voter at a time while retaining serving quorum. After that barrier, a replayable
  legacy JoinSession converts to a Join operation; malformed legacy material records a terminal
  Blocked operation. New binaries do not fall back to the old auto-promotion behavior.

## Orphan voter repair

- Run only on the current leader's local API endpoint:

```bash
sudo xp-ops xp repair-orphan-voter --api-base-url http://127.0.0.1:62416 --raft-node-id <id>
sudo xp-ops xp repair-orphan-voter --api-base-url http://127.0.0.1:62416 --raft-node-id <id> \
  --apply --expected-membership <fingerprint>
```

- The first command is dry-run and writes nothing. Apply requires the exact returned fingerprint.
- The service rechecks signed internal authentication, leader ownership, linearizability, no joint
  configuration, no active operation, the target not being leader, no DesiredState mapping or
  pending join session, and the target being the unique orphan voter.
- The dry-run establishes the unique orphan before the capability barrier excludes that exact
  target. Its advertised public API URL is not a repair prerequisite; every retained mapped voter
  must still verify the lifecycle capability through signed Mesh transport or its public fallback.
- Apply issues only `RemoveVoters({target}, false)` and verifies `absent`. It does not edit Nodes,
  endpoints, users, traffic configuration, or Raft files. A failed or mismatched precondition is a
  blocked incident, not an automated recovery signal.

## Interfaces

- `POST /api/admin/_internal/raft/repair-orphan-voter` accepts signed internal-auth requests.
  Dry-run returns `{ dry_run, raft_node_id, expected_membership }`; apply also requires
  `expected_membership` and returns the operation record.
- `GET /api/admin/membership-operations/{operation_id}` is read-only admin status.
- `xp-ops xp membership-operation status --api-base-url <local-url> --operation-id <uuid>` reads
  that status through local signed internal authentication.
- Generic internal `client-write` rejects membership-operation commands. The Admin Web has no
  repair action.

## Acceptance

- Unknown learners are never automatically promoted.
- Dry-run repair performs zero writes; a stale fingerprint, leader target, joint membership, active
  operation, mapping conflict, or multiple orphan voters is rejected.
- An unreachable public URL on the proven orphan does not block repair when retained mapped voters
  verify the capability through Mesh; unavailable retained voters remain a coordinated-upgrade
  blocker.
- Delete returns `204` when completed within five seconds; otherwise it returns `202` with an
  operation id and status URL. The Web resumes polling the same operation after refresh and only
  refreshes inventory after a terminal completion.

## Visual Evidence

PR: none

- source_type: `storybook_canvas`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `none`
  viewport_strategy: `storybook-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A; synthetic Storybook fixture only`
  submission_gate: `pending-owner-approval`
  story_id_or_title: `Pages/NodeDetailsPage/DeletePending`
  state: `prepared`
  evidence_note: `202 delete operation remains visible and disables duplicate deletion.`

![Pending node deletion](./assets/node-delete-pending-final.png)

- source_type: `storybook_canvas`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `none`
  viewport_strategy: `storybook-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A; synthetic Storybook fixture only`
  submission_gate: `pending-owner-approval`
  story_id_or_title: `Pages/NodeDetailsPage/DeleteBlocked`
  state: `blocked`
  evidence_note: `A blocked delete exposes membership evidence; destructive retry stays disabled.`

![Blocked node deletion](./assets/node-delete-blocked-final.png)
