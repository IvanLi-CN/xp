# Keep public HTTPS authoritative for history synchronization

History synchronization uses the target node's registered public HTTPS
`api_base_url` for summary, repair, backfill, anti-entropy, source delivery and
acknowledgements. It never probes or opens Mesh, Reverse Mesh or dynamic relay
fallbacks. A temporary delivery failure or bounded outbox is Recoverable
Backlog, not a Permanent Gap: a Source or any ready History Repository may
repair the original cursor range before a Permanent Gap is declared.

## Considered options

- Using Mesh or Reverse as a history fallback: rejected because it creates an
  additional routed dependency and can hide public-path failures from the
  durable retry state.
- Treating an unavailable public peer as converged: rejected because it would
  allow a repository to claim readiness without the complete known union.
- Declaring a gap when a source outbox fills: rejected because delivery pressure
  does not prove the underlying source history is unavailable.
