# Bind host ingress protection to the Xray service cgroup

XP protects host-managed public Xray TCP listeners with a root-owned nftables
table selected by the listener's cgroup v2 identity. The protection applies to
every non-loopback TCP listener in the managed Xray service cgroup, not to a
mutable list of endpoint ports. It admits bounded new-connection traffic and
silently drops excess initial SYN packets before Xray performs TLS or REALITY
work.

The `xp` service remains unprivileged. A root-owned, short-lived pre-start
action refreshes the table for the current Xray cgroup on every service start,
verifies the owned ruleset, and exits. In enforcement mode, Xray does not start
unless that action succeeds. The feature is host-managed systemd/OpenRC only;
container deployment remains outside this boundary.

## Considered options

- Per-port firewall rules: rejected because managed endpoint ports are dynamic
  cluster state and custom public Xray listeners would be left unprotected.
- Give `xp` firewall permissions: rejected because a control-plane process must
  not receive host-administration authority merely to follow endpoint changes.
- Resident privileged guard daemon: rejected because matching is available in
  the kernel and each Xray start already provides a lifecycle point at which the
  current cgroup identity can be refreshed.
- One static cgroup rule: rejected because a service restart may recreate the
  cgroup with a different identity, leaving the new Xray process unmatched.
- Xray-only timeouts or post-handshake disconnects: rejected as the primary
  defense because TLS/REALITY work has already consumed the constrained CPU.
- TCP reset for excess connection attempts: rejected because a silent SYN drop
  avoids the response work and does not add an observable rejection behavior.
