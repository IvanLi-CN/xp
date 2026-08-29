# Context Map

## Contexts

- [XP Release](./CONTEXT.md) — determines which merged change becomes an XP release.
- [Cluster History Repositories](./docs/specs/cluster-history-repositories/CONTEXT.md) —
  preserves and reconciles long-term cluster history.
- [Managed Host Operations](./docs/ops/CONTEXT.md) — defines the host-managed runtime
  and ingress-protection language.

## Relationships

- **Cluster History Repositories -> XP Release**: repository behavior is delivered
  as a release target.
- **Managed Host Operations -> XP Release**: host-managed runtime contracts constrain
  the installation and upgrade behavior of release artifacts.
