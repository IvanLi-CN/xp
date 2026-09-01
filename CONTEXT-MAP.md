# Context Map

## Contexts

- [XP Release](./CONTEXT.md) — determines which merged change becomes an XP release.
- [Cluster History Repositories](./docs/specs/cluster-history-repositories/CONTEXT.md) —
  preserves and reconciles long-term cluster history.
- [Managed Host Operations](./docs/ops/CONTEXT.md) — defines the host-managed runtime
  and ingress-protection language.
- [Raft Membership Lifecycle](./docs/specs/7mvqp-raft-membership-voter-invariant/CONTEXT.md) —
  defines the valid cluster-member roles and operator recovery terms.
- [Mihomo Private Resource CIDRs](./docs/specs/mihomo-private-resource-cidrs/CONTEXT.md) —
  defines node-local private target authorization for split-DNS mirror resources.

## Relationships

- **Cluster History Repositories -> XP Release**: repository behavior is delivered
  as a release target.
- **Managed Host Operations -> XP Release**: host-managed runtime contracts constrain
  the installation and upgrade behavior of release artifacts.
- **Raft Membership Lifecycle -> Managed Host Operations**: a host operator invokes only
  the narrow lifecycle commands that the cluster leader has preflighted.
