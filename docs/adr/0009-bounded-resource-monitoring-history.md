# Keep resource monitoring bounded and repository-backed

XP Resource Monitoring observes only a fixed Managed Runtime Stack and Resource
Domain. It retains a one-hour in-memory raw window, publishes bounded semantic
minute Rollups through the existing Source Delivery Journal, and persists them
in History Repositories with a dedicated reducer. This is deliberately not a
generic metrics platform, does not accept arbitrary process or label selection,
and does not add an XP-controlled root collector.

## Considered Options

- A generic Prometheus-style exporter or external TSDB would make cardinality,
  retention, privilege, and deployment ownership unbounded.
- Persisting every 15-second sample would multiply SQLite, transport, and
  repository cost without improving the supported operator views.
- A root collector would make process PSS and I/O more complete on hardened
  hosts, but expands XP's privilege boundary across systemd, OpenRC, and
  container deployments.

## Consequences

- Each field carries explicit supported, partial, or unsupported capability;
  unavailable data is never represented as zero.
- Resource history has a per-stream capacity allocation and records gaps when
  durable capture is suspended, while current in-memory readings continue.
- The former `nmgq8` prohibition now excludes an unrestricted metrics platform,
  not this bounded product capability.
