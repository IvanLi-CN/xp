# Context Map

## Contexts

- [XP Release](./CONTEXT.md) — determines which merged change becomes an XP release.
- [Cluster History Repositories](./docs/specs/cluster-history-repositories/CONTEXT.md) —
  preserves and reconciles long-term cluster history.
- [Managed Host Operations](./docs/ops/CONTEXT.md) — defines the host-managed runtime
  and ingress-protection language.
- [Service Monitoring](./docs/specs/uptime-monitoring/CONTEXT.md) — defines
  administrator-managed remote availability checks observed from cluster nodes.
- [Resource Monitoring](./docs/specs/resource-monitoring/CONTEXT.md) — defines
  bounded resource observations for managed node runtimes and their execution
  domains.
- [Raft Membership Lifecycle](./docs/specs/7mvqp-raft-membership-voter-invariant/CONTEXT.md) —
  defines the valid cluster-member roles and operator recovery terms.
- [Mihomo Private Resource CIDRs](./docs/specs/mihomo-private-resource-cidrs/CONTEXT.md) —
  defines node-local private target authorization for split-DNS mirror resources.

## Relationships

- **Cluster History Repositories -> XP Release**: repository behavior is delivered
  as a release target.
- **Managed Host Operations -> XP Release**: host-managed runtime contracts constrain
  the installation and upgrade behavior of release artifacts.
- **Service Monitoring -> Cluster History Repositories**: Service Monitoring authors
  long-term observations that History Repositories preserve and reconcile.
- **Managed Host Operations -> Service Monitoring**: host capabilities determine
  whether an Observer Node can execute ICMP checks.
- **Resource Monitoring -> Cluster History Repositories**: Resource Monitoring authors
  long-term Resource Rollups that History Repositories preserve and reconcile.
- **Managed Host Operations -> Resource Monitoring**: deployment shape determines
  each node's Resource Domain.
- **Raft Membership Lifecycle -> Managed Host Operations**: a host operator invokes only
  the narrow lifecycle commands that the cluster leader has preflighted.
