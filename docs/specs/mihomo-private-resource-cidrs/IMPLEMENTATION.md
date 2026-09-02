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
  RFC1918/ULA boundary before accepting a tag. It accepts a bare private IP as
  a host CIDR, retains rejected draft entries for correction, and displays
  canonical CIDR tags. The server remains authoritative when it persists and
  normalizes the override.
- The shared tag input supports an optional tag normalizer. Failed entries stay
  in its editable draft while valid entries from the same batch are committed.
- The node policy save action first commits its current editor draft and sends
  no request when that commit has rejected entries. A successful explicit
  policy replacement resets the editor draft; failed requests do not reset it.
- The policy editor uses removable CIDR tags and a source-managed 393px mobile
  Storybook scenario covering an IPv6 ULA policy and uniformly sized actions.
