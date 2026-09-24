# Admin Delete Node History

## Key Decisions

- 2026-02-06: Initial delete node feature shipped with a hard `409 conflict` when endpoints referenced the node.
- 2026-05-18: Delete flow changed to preview endpoint impact and allow explicit confirmed endpoint cleanup with `delete_endpoints=true`.

## Source Migration

- The legacy plan was retired after its contract was established in this topic.
