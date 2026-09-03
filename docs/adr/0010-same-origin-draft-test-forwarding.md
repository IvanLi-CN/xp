# ADR 0010: Same-Origin Draft Test Forwarding

Draft Cluster Tests are coordinated by the current Leader, but an administrator
may load the Web application from any cluster node. A public HTTP redirect from a
follower makes a bearer-authenticated browser POST cross origins, which neither
preserves the request contract nor keeps the token at its ingress. Followers
therefore authenticate browser requests locally and forward create and status
operations to a signed internal Draft Test route on the coordinator; the browser
always receives a same-origin response. The forwarding envelope carries only the
validated caller's rate-limit fingerprint and idempotency key, never the bearer
token. An unavailable Leader rejects an unstarted create as `leader_unavailable`;
an already-created run whose coordinator cannot be recovered is returned as
`interrupted`.

## Considered Options

- Browser redirects or leader discovery would require cross-origin authenticated
  requests and expose the admin token to another public origin.
- Replicating temporary run state through Raft would extend a 15-minute UI aid
  into the durable monitoring control plane without improving uptime history.
