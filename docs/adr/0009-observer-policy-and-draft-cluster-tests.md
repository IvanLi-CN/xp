# ADR 0009: Observer Policy and Draft Cluster Tests

## Status

Accepted

## Context

Service Monitor definitions previously represented the default observer set as a
nullable `observer_node_ids` field. That shape could not express exclusions or
preserve departed node IDs, and the creation page used a synchronous preflight
that looked like a persisted observation. Administrators need an explicit policy
and a temporary, inspectable cluster test without contaminating uptime history.

## Decision

Persist `ObserverPolicy { mode, node_ids }`. Empty `exclude` resolves to every
currently registered capable Observer Node; non-empty `exclude` removes only the
listed IDs; `include` requires and resolves to the listed IDs. Legacy null and
non-empty arrays are read as empty exclude and include respectively.

Expose leader-coordinated Draft Cluster Test endpoints. The run snapshots target,
policy and resolved Observer Set, staggers node work deterministically within
750ms, and stores only its short-lived state in `uptime.sqlite3` for 15 minutes.
Draft results are not Observations and do not enter journal, Repository, rollups,
availability or coverage. Creation remains independent of every draft state.

The formal editor is a responsive route: wide containers use the B two-column
workspace, while narrower and mobile containers use a single-column page. The
policy selector and fixed-header result table are shared by new and edit routes.

## Consequences

New clients send `observer_policy` only when the capability is advertised and
degrade explicitly on older servers. Existing snapshots remain readable without
rewriting IDs. Temporary test records require TTL handling and an explicit
`interrupted` outcome when their coordinator cannot resume them.
