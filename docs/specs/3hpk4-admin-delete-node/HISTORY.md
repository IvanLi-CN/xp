# Admin Delete Node History

## Key Decisions

- 2026-02-06: Initial delete node feature shipped with a hard `409 conflict` when endpoints referenced the node.
- 2026-05-18: Delete flow changed to preview endpoint impact and allow explicit confirmed endpoint cleanup with `delete_endpoints=true`.

## Legacy Source

- `docs/plan/3hpk4:admin-delete-node/PLAN.md` is the legacy source document for this canonical spec and remains pending delete approval.
