# Context

## Terms

- **Deployment default**: the node's startup-only value from
  `XP_MIHOMO_ALLOWED_PRIVATE_CIDRS`.
- **Web override**: the complete replacement list stored in the node's local
  policy file. An empty list is meaningful and denies all private targets.
- **Effective policy**: the snapshot used by one resource request. Changing a
  policy affects subsequent requests and never mutates an in-flight snapshot.
