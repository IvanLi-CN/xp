# Context Map

## Contexts

- [XP Release](./CONTEXT.md) — determines which merged change becomes an XP release.
- [Cluster History Repositories](./docs/specs/cluster-history-repositories/CONTEXT.md) —
  preserves and reconciles long-term cluster history.
- [Managed Host Operations](./docs/ops/CONTEXT.md) — defines the host-managed runtime
  and ingress-protection language.
- [Service Monitoring](./docs/specs/uptime-monitoring/CONTEXT.md) — defines
  administrator-managed remote availability checks observed from cluster nodes.

## Relationships

- **Cluster History Repositories -> XP Release**: repository behavior is delivered
  as a release target.
- **Managed Host Operations -> XP Release**: host-managed runtime contracts constrain
  the installation and upgrade behavior of release artifacts.
- **Service Monitoring -> Cluster History Repositories**: Service Monitoring authors
  long-term observations that History Repositories preserve and reconcile.
- **Managed Host Operations -> Service Monitoring**: host capabilities determine
  whether an Observer Node can execute ICMP checks.
