# Raft core skeleton (Milestone 5)

## Crate choice: OpenRaft

This project uses **OpenRaft** (`openraft`) as the Raft engine.

Why:
- OpenRaft explicitly supports a **storage v2** model that separates **log store** (`RaftLogStorage`) and **state machine** (`RaftStateMachine`), which matches Milestone 5's requirement of **WAL + snapshot** and a state machine that stores only **desired state**.
- OpenRaft's guide recommends implementing those two storage traits as the primary integration point, and its changelog notes the v2 separation as a design goal for natural parallelism.

References (checked in Task 003):
- OpenRaft Getting Started: implement `RaftLogStorage` + `RaftStateMachine` (docs.rs / upstream guide).
- OpenRaft changelog: `storage-v2` notes v2 separates log store and state machine.

If we ever need to switch away from OpenRaft, document the concrete blocker here (API mismatch, missing feature, etc.) before changing dependencies.

## Module boundaries

- `types.rs`: project Raft type config (node id, node metadata, request/response types).
- `node.rs`: `RaftNode` wiring object (directories + bind/api info). No HTTP wiring yet.
- `storage/`: local persistence boundaries:
  - `wal.rs`: write-ahead log (Raft log store).
  - `state_machine.rs`: desired-state state machine store (apply / snapshot view).
  - `snapshot.rs`: snapshot building + snapshot store abstraction.
- `network.rs`: transport boundary (Wave 2 will implement over HTTPS; no RPC here yet).

The goal of this skeleton is to make Wave 2 integration a focused task: implement storage + network adapters without reworking module boundaries.
