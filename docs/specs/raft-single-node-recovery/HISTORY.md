# History

- The topic was promoted from legacy plan `#c2v6d`.
- It documents the explicit single-node recovery path instead of an automatic consensus repair.

## Decision trace

- The original specification assumed the wiped node's DesiredState record disappeared with its
  volume. The compose regression wipes every volume at `reset`, so the surviving node always ended
  up with DesiredState equal to its single-node membership and the rejoin case passed.
- A real incident keeps the survivor's full state: one node stays healthy while its peers are
  permanently gone. Recovery shrank Raft to one voter but left five DesiredState Nodes, so every
  subsequent lifecycle write failed `membership_invariant_violation` with those five ids in
  `missing_desired_members`. Fresh join was refused, and remove-node requires the same clean
  audit, so no CLI could converge the cluster.
- Fixing this by exempting `missing_desired_members` from fresh-join admission was rejected: ADR
  0010 and the voter-invariant spec deliberately restrict that exemption to `unexpected_learners`.
  The invariant is the safety contract for delete, restore and eviction, not a join obstacle.
- Pruning those DesiredState records during recovery was implemented, reviewed and then rejected on
  a live cluster: the retired nodes were still serving user traffic on their own ports, and the
  xp-proxy data plane reads its snapshot locally rather than from Raft. Deleting their Node and
  Endpoint records would have traded control-plane consistency for a data-plane outage, which the
  owner ruled out.
- The settled split keeps recovery non-destructive and gives each retired node an explicit next
  step: a node that still runs rejoins under its existing identity and endpoints, while a node that
  is permanently gone is removed through the eviction path with an explicit absence declaration.
  Recovery itself never deletes DesiredState rows.
