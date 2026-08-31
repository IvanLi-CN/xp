# Mihomo private resource CIDR policy

## Summary

Mihomo external resource mirroring may reach a private address only when the
address is in the node-local policy. The deployment default comes solely from
`XP_MIHOMO_ALLOWED_PRIVATE_CIDRS`; an administrator may replace it for one node
through the signed Mesh-backed node API.

## Contract

- Only RFC1918 IPv4 and IPv6 ULA (`fc00::/7`) networks can be configured.
- Loopback, link-local, multicast, unspecified, documentation, shared-address,
  and metadata ranges remain blocked even when a containing network is listed.
- DNS answers are filtered individually. Each HTTPS redirect repeats URL, DNS,
  and address validation and is limited to five redirects.
- The effective policy is Web override, then deployment default, then empty.
  An empty override is an explicit deny-all private policy; DELETE restores the
  deployment default.
- Policy state is node-local and never replicated through Raft.
- A corrupt override fails closed for private targets and is reported by the
  node policy API.

## Related ADRs

- `docs/adr/0007-node-local-mihomo-private-cidr-policy.md`
