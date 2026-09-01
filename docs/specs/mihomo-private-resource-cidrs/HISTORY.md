# History

- The legacy cluster-wide `mihomo_resource_allow_private_targets` contract is
  retained only for one compatibility window: GET reports `false` and PUT is
  rejected with `410 Gone`. Its persisted decode compatibility is removed in a
  later schema cleanup.
