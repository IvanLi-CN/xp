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
- Public Admin Web deletion never excludes an unreachable voter from the lifecycle capability
  barrier. The operator-only mapped-voter eviction command is documented with the membership
  invariant and records the same RemoveNode operation only after its separate proof contract.
- That operator-only path rejects before intent creation when any retained mapped voter cannot
  prove the lifecycle capability; it cannot turn a peer outage into a broader deletion bypass.

## Coverage

- Backend HTTP tests cover preview, default conflict, confirmed endpoint cleanup, existing delete
  guards, and the `202` pending operation contract.
- State tests cover node cleanup and endpoint cleanup.
- Signed internal-route tests cover dry run, confirmation rejection, and the retained-voter
  capability failure path with no node, endpoint, or membership-operation write.
- Web tests cover endpoint preview, cancel, confirmed cleanup, persisted `202` operation polling,
  and duplicate-delete disablement.
- Storybook includes a NodeDetailsPage delete confirmation scenario with endpoints.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-02-06
- Last: 2026-05-18

## 实现里程碑（Milestones）

- [x] M1: Server 端支持 delete preview 与确认清理参数。
- [x] M2: State machine 支持确认模式下原子删除 node 和 endpoints。
- [x] M3: Web Node details 删除流展示 endpoint 影响列表。
- [x] M4: 测试、Storybook 与视觉证据同步。
