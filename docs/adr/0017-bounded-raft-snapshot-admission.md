# Bound Raft Snapshot Admission Before OpenRaft

Status: accepted

Incoming authenticated Raft snapshots must not turn temporary Mesh reader contention into a fatal
state-machine storage error. Runtime-events SSE holds the shared Mesh read guard for its bounded
stream lease, while snapshot storage also needs the exclusive gate to publish its fail-closed marker
and replace durable state. OpenRaft propagates a storage error out of its state-machine worker; the
node shuts down instead of retrying the snapshot.

The authenticated `/raft/snapshot` route acquires the Mesh gate and epoch write barriers before it
calls OpenRaft. The admission deadline remains three seconds. If the existing readers do not drain,
the route returns HTTP 503 before the OpenRaft worker sees the request. The auth middleware signs
that status; the sender classifies it as unreachable and retains OpenRaft's existing retry/backoff.

After a successful drain, admission sets an RAII reservation and releases the gate locks so the
state machine can acquire them for installation. New Mesh requests check the reservation before
dispatch and remain known-not-dispatched while it is active. Mesh re-enable preflight participates
in the same read admission even when the durable gate is disabled. Deferred body-completion and
telemetry callbacks cannot acquire new epoch readers during the reservation, so temporary callback
lock contention cannot make the state-machine installation barrier fail as an OpenRaft storage
error. The reservation is owned by a task that continues the OpenRaft call even if the HTTP request
is cancelled, and it is released only after the real install call terminates. The snapshot pending
marker, authenticated-state evidence, and genuine storage errors retain their existing fail-closed
behavior.

## Considered Options

- Holding the existing gate write guards across `Raft::install_snapshot` was rejected because the
  state machine must acquire those same guards to install the snapshot.
- Returning an OpenRaft storage error on admission timeout was rejected because the worker treats it
  as fatal and shuts down the node.
- Increasing the state-machine timeout was rejected because it delays recovery without bounding the
  SSE lifecycle and changes behavior for all snapshot storage failures.
- Returning a successful Raft response when the gate is unavailable was rejected because it would
  acknowledge a snapshot that OpenRaft never received.

## Consequences

- Snapshot gate contention is retriable transport backpressure, not a storage failure or successful
  install; no public route, Raft payload, wire field, persistence schema, or deployment default is
  added.
- During an admitted install, new Mesh requests do not dispatch until the real state-machine result
  is known. Once the reservation ends, the installed cluster gate determines normal routing.
- A disabled-gate re-enable preflight is also known-not-dispatched during an admitted install, and
  deferred completion readers cannot block the state-machine barrier after reservation succeeds.
- Regression coverage must exercise the signed HTTP route with a real OpenRaft receiver, verify the
  signed 503 and continued Raft liveness under contention, then retry the same snapshot and observe
  applied data and index convergence.
