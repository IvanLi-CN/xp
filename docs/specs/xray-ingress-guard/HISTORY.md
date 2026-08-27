# Host-managed Xray Ingress Guard History

## Key Decisions

- Protection is selected by the Xray service cgroup rather than a static port
  list, so all non-loopback TCP listeners share the same admission boundary.
- `xp` receives no firewall privilege or new delegation. Root-owned init-system
  lifecycle assets are the only mutation path.
- The guard is a one-shot startup action, not a resident daemon. It refreshes
  cgroup identity at service start because that identity may change on restart.
- Enforced mode blocks Xray startup when the verified nft table cannot be
  prepared; it never silently starts an unprotected public listener.
- Existing host-managed nodes activate immediately through an explicit root
  `xp-ops` command. Observe mode is optional diagnostics.
- The constrained-node `small-vps` profile uses global `8/s` with burst `20`
  and per-source `3/s` with burst `8`; excess initial SYNs are silently dropped.
- Docker/Compose, custom Xray service assets, Web mutations, and protocol-aware
  abuse classification are intentionally outside this topic.
