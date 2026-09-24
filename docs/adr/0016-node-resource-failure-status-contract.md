# Node resource failure status contract

Status: accepted

Node Details resource reads cross the browser, the local XP HTTP API, and a signed peer transport.
Those boundaries expose different evidence. The API must preserve that evidence without leaking
transport internals or causing the Web client to infer a node failure from one unsuccessful read.

## Decision

Resource current, recent, and history peer reads use a bounded structured diagnostic body. The
diagnostic contains only `failure_layer`, `cause`, `confidence`, `target_node_id`,
`attempted_path`, `dispatch_state`, `retryable`, optional `retry_after_seconds`, optional
`target_status`, and an opaque `support_id`. Values are stable enums or bounded scalars; full URLs,
addresses, credentials, headers, signatures, stacks, and raw third-party error text are excluded.

The status contract is:

- An open circuit fails before dispatch with 503 `peer_circuit_open`. The server emits an integer
  `Retry-After` and `details.retry_after_seconds`, both bounded to 1 through 300. The browser must
  honor this value and must not guess a cooldown.
- A dispatched transport timeout without a verified response remains 504
  `peer_transport_timeout`.
- A non-timeout transport failure with no verified response is 502
  `peer_transport_error`; it remains in the `peer_transport` layer.
- An invalid or missing signed acknowledgement, including peer authentication rejection, is 502
  `peer_protocol_rejected`.
- A verified target application error preserves the target HTTP status and uses
  `remote_node_error` with `failure_layer=remote_node`.
- A verified capability-route 404 retains the existing unsupported behavior. It is not a node
  outage and is not converted into a peer transport error.

This typed mapping is scoped to resource reads. Existing generic Mesh callers retain their current
gateway-timeout mapping until a separate API contract is approved.

The Web Resources Tab keeps the last successful current snapshot only for the current page
session. It displays `observed_at`, the query's last successful fetch time, and stale age as
separate facts. A current or history query pauses its own automatic polling after an error and
resumes only after a successful refetch. Unstructured and legacy failures use an `unknown`
diagnostic without displaying the raw error message. No failure action restarts a service, mutates
configuration, changes an endpoint, or probes production.

## Considered options

- Returning 504 for every Mesh failure was rejected because it hides the difference between a
  request that was never dispatched, a timeout, and a rejected signed response.
- Letting the browser derive cooldown from circuit state or generic retry headers was rejected
  because the client cannot know the server's breaker state and could create a retry burst.
- Treating verified remote application errors as local XP failures was rejected because it causes
  operators to repair the wrong node.
- Persisting the last resource snapshot in browser storage was rejected because resource data is
  intentionally session-scoped and stale persistence would outlive the evidence boundary.

## Consequences

- API tests must cover circuit-open 503 with `Retry-After`, dispatched timeout 504, protocol
  rejection 502, verified remote status preservation, and capability 404 compatibility.
- The Web client can give a safe next action while retaining usable stale data, but it must stop
  automatic polling after failure and wait for a successful query before resuming.
- The contract adds a resource-specific error surface; a future all-site error protocol change
  requires a separate decision.
