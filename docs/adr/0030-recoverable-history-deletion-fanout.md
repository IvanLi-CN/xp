# ADR 0030: Recover History Deletion Fanout Across Raft and Local Storage

## Status

Accepted

## Context

Deleting a user changes Raft state and separately creates local and per-node history cleanup work.
A crash or a storage error between those effects can leave a committed deletion without a retry for
some nodes. A repository ACK cannot prove that the missing destination work was recorded.

## Decision

The node handling a user deletion durably records a prepared operation with the known destinations
before submitting the Raft deletion. Preparation alone neither removes history nor emits a
repository tombstone. After Raft confirms the deletion, a single guarded history transaction
activates local deletion and every recorded destination. A bounded recovery worker retries an
interrupted Raft write and activation using the prepared record. The worker serializes with the
request path and keeps preparation when Raft or local storage is unavailable. Activation requires
a successful committed Raft response; an in-memory absence after a failed save is not proof.

Node membership removal already has a Raft-backed operation that is retried until its cleanup
finishes. Its local deletion and complete destination set are therefore committed in one guarded
history transaction before that operation is marked complete.
An observed, durably saved node removal also removes that node from active and prepared destination
sets. Activation refreshes current membership under the same local state lock; remote node cleanup
waits for the recipient's own saved membership to reflect the removal before acknowledging it.

## Consequences

A failed preparation leaves the user in Raft. A later failure leaves a durable operation that the
same node resumes after restart; real repository ACKs do not erase unregistered destination work.
Errors after preparation do not cancel the requested deletion. The prepared operation belongs to
the request node's history database and must be retained during node recovery. A build that does
not recognize prepared operations can overwrite them, so operators drain this state before a
downgrade. The design avoids introducing a new Raft command or requiring older cluster members to
decode one during a rolling upgrade.
