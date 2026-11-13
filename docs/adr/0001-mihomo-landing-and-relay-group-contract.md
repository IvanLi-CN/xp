# Mihomo Dynamic Group Interpolation Contract

The user-owned Mihomo `mixin_yaml` may use the reserved interpolation entries `🌟 <region>` and `🛬 <node>` inside any `proxy-groups[*].proxies` list. XP expands these entries during subscription rendering using only the groups and landing nodes available to that user.

`🌟 <region>` expands to the matching hidden regional groups such as `🌟 Japan` and `🌟 Singapore`. `🛬 <node>` expands to every generated landing group such as `🛬 Tokyo-A` and `🛬 Singapore-B`, in stable node order. A group that contains either placeholder retains the surrounding literal entries and their order.

Every subscribed node with a generated Reality access proxy receives a landing group, even when it has no chain proxy. The landing group starts with the node's Reality access proxy, followed by available chain candidates.

The system keeps `🔒 落地` as the `url-test` group over every generated `🛬 {base}` group and adds `🤯 落地` as a hidden `url-test` pool over every subscribed `*-reality` Access Point. Both `💎 高质量` and `💎 节点选择` use the automatic pool before `🤯 All`: their fallback orders are `🔒 高质量`, `🤯 落地`, `🤯 All` and `🚀 节点选择`, `🤯 落地`, `🤯 All`, respectively. Regional `🤯 {region}` groups are not generated.

Application groups, including a group such as `🎵 Spotify`, must be able to use the same placeholders. The rendered application group must list each expanded `🛬 {node}` entry explicitly; showing only regional groups is insufficient.

An unsubscribed cluster node is absent from all expansions and cannot become a landing or relay candidate. User-defined groups and literal entries remain preserved. External providers may add candidates, but cannot replace generated access points or bypass relay fail-closed behavior.

## Consequences

- Placeholder expansion is performed after generated proxy and group names are known, and before final reference validation.
- `🌟 {region}` remains an internal hidden group and is available to templates
  through `🌟 <region>`.
- `🛬 {node}` is a per-node group and is available to templates through `🛬 <node>`; no node is represented only by a regional alias.
- `🔒 落地` remains the aggregate `url-test` over generated `🛬 {base}` groups.
- `🤯 落地` is the single hidden aggregate health-tested Access Point pool. It contains only currently generated `*-reality` proxies and is omitted when no Access Point exists.
- `🤯 All` may include landing groups and hidden regional groups, but no `🤯 {region}` groups are emitted.
