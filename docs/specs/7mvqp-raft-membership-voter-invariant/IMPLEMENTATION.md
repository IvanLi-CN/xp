# Raft membership lifecycle implementation

## Current state

- `PersistedState.membership_operations` is additive and defaults on old snapshots; the schema
  version remains 12. Begin and transition state-machine commands validate operation identity,
  one active operation, monotonic phase transitions, and terminal timestamps.
- `RaftFacade::ensure_linearizable()` uses OpenRaft's quorum heartbeat in production. Lifecycle
  writers evaluate mappings and fingerprints only after that check.
- `raft_membership_guard` never infers a promotion. Its periodic work audits orphan voters,
  unexpected learners, and missing desired members; it resumes only recorded RemoveNode and
  Restore operations, and prunes terminal evidence after 24 hours.
- Join atomically records the Node, JoinSession, and operation before learner registration. The
  coordinator only promotes the exact recorded target after a fingerprint match; a legacy session
  cannot cause a promotion until it has passed capability-gated migration into an operation.
- Delete records RemoveNode intent, uses a single retain=false removal action, verifies absence,
  and does not compensate by restoring a learner or voter. A five-second incomplete delete returns
  `202` and leaves a status operation for retry.
- Orphan voter repair is signed, internal, leader-local CLI/API. Generic internal client-write
  refuses membership-operation commands.
- Unreachable mapped-voter eviction is a separate signed, leader-local CLI/API with no Admin Web
  action. It proves the target before excluding it from retained-voter capability verification,
  then records the existing RemoveNode operation with an immutable endpoint snapshot so the normal
  resumer and cleanup machinery retain ownership of uncertain-result recovery. The resumer writes
  only from the current local leader and terminally blocks the operation if its target becomes that
  leader before `RemoveVoters`.

## Compatibility

- `cluster.membership-lifecycle-v1` is advertised by this build. Before a new lifecycle command,
  every retained DesiredState-mapped voter must advertise it through a signed Mesh capability read.
  A predecessor's unacknowledged `404` for that signed route means it lacks the route, so its
  legacy public `/api/capabilities` is tried within the same probe budget. Only that response may
  omit an acknowledgement; other missing or invalid acknowledgements, non-`404` status,
  authentication, and transport failures do not downgrade. Each capability body is limited to
  64 KiB. An unreachable public URL is therefore not a separate rejection condition for current
  peers. A retained voter that cannot
  verify still returns `coordinated_upgrade_required` and no new command is written.
- An additive persisted field alone is compatible with old snapshots. The command variants are not
  compatible with an old binary, which is why the capability barrier precedes the first command.
- After the barrier, valid legacy JoinSessions become replayable Join operations. Invalid legacy
  sessions become terminal Blocked evidence; they are not inferred from member shape or promoted.
- Orphan repair first performs its existing leader-local, linearizable preview. Only the previewed
  unique orphan is excluded from the retained-voter capability probe, so a stale orphan public URL
  cannot bypass or block the capability barrier.
- Unreachable mapped-voter eviction likewise excludes only the previewed target from the retained
  capability probe. Unlike orphan repair it requires the target's exact DesiredState mapping and
  confirmed endpoint snapshot, then deletes those records through RemoveNode after Raft proves the
  target absent.

## Coverage

- State tests reject overlapping operations, invalid phase jumps, and terminal transitions without
  evidence timestamps.
- Membership tests exercise dry-run/apply orphan repair, exact fingerprinting, unique target
  validation, `RemoveVoters(..., false)`, absent postcondition, and unchanged DesiredState nodes.
- HTTP coverage exercises a dry-run with unavailable or empty public peer URLs and a signed Mesh
  capability response, plus the predecessor's unacknowledged-404 legacy capability fallback. A
  signed `404` or unreachable Mesh transport cannot invoke a public fallback; invalid
  acknowledgements, repair-target rejections, and bounded capability-body reads remain covered.
- HTTP delete tests cover synchronous `204`, pending `202`, endpoint confirmation, leader/local
  guards, and membership failure paths.
- Membership tests cover mapped-voter preview, endpoint snapshot mismatch rejection, durable
  RemoveNode creation, retain=false membership removal, and state cleanup.
- Node details tests cover an accepted deletion's persisted operation id, status polling, and
  duplicate-delete disablement.
