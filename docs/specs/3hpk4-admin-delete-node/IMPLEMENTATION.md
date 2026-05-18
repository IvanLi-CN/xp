# Admin Delete Node Implementation

## Current State

- `DELETE /api/admin/nodes/:node_id` removes non-local, non-leader nodes.
- Default delete still rejects nodes with endpoints using `409 conflict`.
- `GET /api/admin/nodes/:node_id/delete-preview` lists endpoint impact for UI confirmation.
- `DELETE /api/admin/nodes/:node_id?delete_endpoints=true&expected_endpoint_ids=<id,id>` deletes node-owned endpoints in the same Raft state-machine command after confirming the endpoint ID set still matches the previewed set.
- Deleted endpoint tags are returned from state apply to the HTTP layer so remove-inbound reconcile requests are scheduled for the actual deleted endpoints.

## Coverage

- Backend HTTP tests cover preview, default conflict, confirmed endpoint cleanup, and existing delete guards.
- State tests cover node cleanup and endpoint cleanup.
- Web tests cover endpoint preview, cancel, and confirmed cleanup request.
- Storybook includes a NodeDetailsPage delete confirmation scenario with endpoints.

## Remaining Gaps

- Legacy `docs/plan/3hpk4:admin-delete-node/PLAN.md` is retained pending delete approval.
