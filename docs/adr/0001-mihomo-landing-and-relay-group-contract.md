# Mihomo Dynamic Group Interpolation Contract

The user-owned Mihomo `mixin_yaml` may use the reserved entries `🌟 <region>` and `🛬 <node>`.
They are valid inside any `proxy-groups[*].proxies` list.
XP expands them during rendering using only groups and landing nodes available to that user.

`🌟 <region>` expands to matching hidden regional groups such as `🌟 Japan` and `🌟 Singapore`.
`🛬 <node>` expands to every generated landing group in stable node order.
A group containing either placeholder retains surrounding literal entries and their order.

Every subscribed node with a generated Reality access proxy receives a landing group.
This also applies when the node has no chain proxy.
The landing group starts with the node's Reality access proxy, followed by chain candidates.

The system generates `🔒 落地` as a `url-test` pool containing every `🛬 {node}` group.
`💎 高质量` keeps the fallback order `🔒 高质量`, `🔒 落地`, `🤯 All`.
Regional `🤯 {region}` groups are not generated.

Application groups such as `🎵 Spotify` may use the same placeholders.
The rendered group lists each expanded `🛬 {node}` entry explicitly.
Showing only regional groups is insufficient.

An unsubscribed cluster node is absent from all expansions.
It cannot become a landing or relay candidate.
User-defined groups and literal entries remain preserved.
External providers may add candidates.
They cannot replace generated access points or bypass relay fail-closed behavior.

## Consequences

- Placeholder expansion runs after generated names are known and before final reference validation.
- `🌟 {region}` remains an internal hidden group and is available to templates
  through `🌟 <region>`.
- `🛬 {node}` is a per-node group available to templates through `🛬 <node>`.
- No node is represented only by a regional alias.
- `🔒 落地` is the single aggregate health-tested landing pool.
- `🤯 All` may include landing groups and hidden regional groups.
- No `🤯 {region}` groups are emitted.
