# Use merge-time events for release intent

Release automation reconstructs intent from the GitHub pull request event history
up to its unique merge event, rather than reading mutable current labels. This
preserves the decision that governed a merge without adding a repository-owned
snapshot, ledger, queue, or external release record. A Manual Backfill remains
an explicit workflow dispatch with an exact expected version when a historical
release must be corrected.

## Considered options

- Current PR labels: rejected because labels can change after merge and would make
  the same commit produce a different release later.
- Repository or external intent records: rejected because they add a second source
  of truth and a retention/repair contract for a one-time release decision.
- Manual dispatch for every release: rejected because normal merges should remain
  automatic and auditable from existing GitHub history.
