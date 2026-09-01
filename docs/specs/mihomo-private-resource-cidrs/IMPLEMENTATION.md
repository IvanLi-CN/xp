# Implementation

- The policy is loaded at startup from `XP_MIHOMO_ALLOWED_PRIVATE_CIDRS` and
  `${XP_DATA_DIR}/mihomo-resource-policy.json`.
- Overrides are written with mode `0600`, file and directory fsync, and atomic
  rename. The JSON schema version is `1`.
- `GET|PUT /api/admin/nodes/{node_id}/mihomo-resource-policy` and
  `DELETE /api/admin/nodes/{node_id}/mihomo-resource-policy/override` are the
  owner-facing routes. PUT accepts `{"override_cidrs":[]}`.
- Remote node requests use the existing signed Mesh helpers and execute only on
  the target node through the internal local route.
- Capability `node.mihomo-resource-private-cidrs-v1` gates the feature. Missing
  capability is a structured upgrade error, not a fallback to the legacy flag.
- The node details editor validates CIDR syntax, prefix bounds, and the private
  RFC1918/ULA boundary before accepting a tag; the server remains authoritative
  when it persists and normalizes the override.
- The policy editor uses removable CIDR tags and a source-managed 393px mobile
  Storybook scenario covering an IPv6 ULA policy and uniformly sized actions.
