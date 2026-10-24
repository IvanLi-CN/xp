# Store Imported Subscription Nodes as Static Profile Snapshots

The Web admin will import copied subscription content into the current user's `extra_proxies_yaml` as a static node snapshot, rather than storing the source as a live `proxy-provider`. The decision matches the existing paste-and-edit workflow, keeps the existing Profile save boundary, and avoids adding background refresh, provider ownership, or source-token persistence to the control plane.

## Considered Options

- Live provider storage: rejected because it would defer parsing to Mihomo, would not support pasted Base64 content consistently, and would introduce runtime refresh and source-credential lifecycle concerns.
- Static Profile snapshot: accepted because the administrator can preview, inspect, edit, and explicitly save the resulting nodes within the existing per-user Profile model.

## Consequences

- Imported nodes do not automatically refresh when the original subscription changes; the administrator must import again.
- URL fetching remains a bounded, public-only preview operation and does not persist the original source.
- The feature can preserve existing Profile and proxy-group semantics, but imported nodes are not automatically added to user-defined groups.
