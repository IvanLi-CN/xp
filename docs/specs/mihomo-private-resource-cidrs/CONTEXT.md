# Context

## Terms

- **Deployment default**: the node's startup-only value from
  `XP_MIHOMO_ALLOWED_PRIVATE_CIDRS`.
- **Web override**: the complete replacement list stored in the node's local
  policy file. An empty list is meaningful and denies all private targets.
- **Effective policy**: the snapshot used by one resource request. Changing a
  policy affects subsequent requests and never mutates an in-flight snapshot.
- **Host CIDR**: a single private IP entered without a prefix and normalized to
  its exact network: IPv4 uses `/32` and IPv6 uses `/128`.
- **Rejected draft**: an uncommitted tag-input candidate that failed local
  validation and remains editable rather than being discarded.
