# Admin Delete Node Implementation

## Current State

- `DELETE /api/admin/nodes/:node_id` records RemoveNode intent and removes non-local, non-leader
  nodes through a linearizable lifecycle precondition.
- Default delete still rejects nodes with endpoints using `409 conflict`.
- `GET /api/admin/nodes/:node_id/delete-preview` lists endpoint impact for UI confirmation.
- `DELETE /api/admin/nodes/:node_id?delete_endpoints=true&expected_endpoint_ids=<id,id>` deletes node-owned endpoints in the same Raft state-machine command after confirming the endpoint ID set still matches the previewed set.
- Deleted endpoint tags are returned from state apply to the HTTP layer so remove-inbound reconcile requests are scheduled for the actual deleted endpoints.
- A finished delete returns `204`. An operation that remains incomplete after five seconds returns
  `202` with a read-only membership operation status URL; a focused worker resumes only that
  recorded operation, without re-adding membership.

## Coverage

- Backend HTTP tests cover preview, default conflict, confirmed endpoint cleanup, existing delete
  guards, and the `202` pending operation contract.
- State tests cover node cleanup and endpoint cleanup.
- Web tests cover endpoint preview, cancel, confirmed cleanup, persisted `202` operation polling,
  and duplicate-delete disablement.
- Storybook includes a NodeDetailsPage delete confirmation scenario with endpoints.

## Remaining Gaps

- Legacy `docs/plan/3hpk4:admin-delete-node/PLAN.md` is retained pending delete approval.
