# Final Mihomo Config Contract

This document defines the owner-facing Mihomo topology that `xp` must render after all mixin replay, system group injection, and final normalization.

## Region Group Graph

For every managed region in `{Japan, HongKong, Taiwan, Korea, Singapore, US, Other}`, the generated region groups must obey the same one-way graph:

```text
🤯 {Region}  --hidden url-test-->  🌟 {Region}  --hidden fallback-->  🔒 {Region}  --visible select-->  leaf proxies
```

Required constraints:

- `🔒 {Region}` is the owner-facing manual selection group.
- `🔒 {Region}` must be `type: select`.
- `🔒 {Region}` must not be hidden.
- `🔒 {Region}` must contain only direct region candidates.
- `🔒 {Region}` must not contain `🛬 {base}` landing groups.
- `🔒 {Region}` must not contain `*-ss-chain` or `*-reality-chain`.
- `🌟 {Region}` is a hidden wrapper group.
- `🌟 {Region}` must be `type: fallback`.
- `🌟 {Region}` must contain exactly one proxy reference: `🔒 {Region}`.
- `🤯 {Region}` is a hidden probe group.
- `🤯 {Region}` must be `type: url-test`.
- `🤯 {Region}` must contain exactly one proxy reference: `🌟 {Region}`.
- `🔒 {Region}` may only be referenced by its corresponding `🌟 {Region}` system wrapper.

## High Quality And Node Selector

The generated system aggregate groups must obey these contracts:

- `🔒 高质量` is visible and owner-facing.
- `🔒 高质量` must be `type: select`.
- `🔒 高质量` must include hidden region wrapper groups `🌟 {Region}`.
- `🔒 高质量` must include landing groups `🛬 {base}` when present.
- On the non-provider route, `🔒 高质量` must include raw managed reality proxies
  `{base}-reality` when present.
- On the provider route, its system-provider filter must include the same Reality candidates.
- `🔒 高质量` must not be hidden.
- `💎 高质量` is hidden.
- `💎 高质量` must be `type: fallback`.
- `💎 高质量` must contain exactly `["🔒 高质量", "🤯 All"]` in that order.
- `🤯 All` is hidden.
- `🤯 All` must be `type: url-test`.
- `🤯 All` must reference only hidden per-region probe groups `🤯 {Region}`.
- `🚀 节点选择` is the visible owner-facing node selector.
- `🚀 节点选择` must be `type: select`.
- `🚀 节点选择` must contain hidden region wrapper groups `🌟 {Region}`.
- `🚀 节点选择` must contain all `🛬 {base}` landing groups before direct Reality
  candidates.
- On the non-provider route, direct Reality candidates are raw managed proxies `{base}-reality`.
- On the provider route, `🚀 节点选择` must consume the same raw managed proxies through
  `use: [xp-system-generated]` and an exact Reality-only filter.
- `🚀 节点选择` must contain every direct Reality candidate when present. It must not add
  `{base}-ss` as a direct candidate.
- A base with no chain proxy must not produce `🛬 {base}`. Its Reality direct candidate remains a
  node-selector candidate.
- `🚀 节点选择` must retain `💎 高质量` after system landing and direct Reality
  candidates.
- `💎 节点选择` is hidden.
- `💎 节点选择` must be `type: fallback`.
- `💎 节点选择` must contain exactly `["🚀 节点选择", "🤯 All"]` in that order.

## System Proxy Group Sequence

The generated system-managed `proxy-groups` block must be deterministic after final normalization.

Required constraints:

- The system-owned sequence must be ordered as:
  `🔒 高质量`
  `-> 💎 高质量`
  `-> all visible region groups in canonical region order`
  `-> all hidden fallback region groups in canonical region order`
  `-> all hidden probe region groups in canonical region order`
- `-> all landing groups \`🛬 {base}\` in deterministic base order`
- `-> 🔒 落地`
- `-> 🤯 All`
- `-> 🚀 节点选择`
- `-> 💎 节点选择`
- `-> hidden per-base relay groups \`🛣️ {relay-base}\` at the tail`
- The canonical region order is exactly:
  `Japan`, `HongKong`, `Taiwan`, `Korea`, `Singapore`, `US`, `Other`.
- User-defined groups must remain outside that system-owned sequence.
- Hidden relay groups must not be interleaved into the high-quality / region / landing / node-selector sequence.

## Per-Base Relay Fail-Closed Contract

Per-base relay groups are transit-only groups for generated `*-chain` proxies.

- A relay group may consume only external third-party providers through `use` and its region
  filter. It must also include an explicit `REJECT` sentinel for the empty or initializing
  filtered state. It must never add `DIRECT` as a candidate.
- With no external third-party provider, the relay group must contain only `REJECT`. The chain
  proxy remains fail-closed and cannot become equivalent to its direct Reality proxy.
- If the external provider filter yields no candidates, the relay group must resolve only to
  `REJECT`; it must not expose Mihomo's `COMPATIBLE` placeholder or use `DIRECT`.
- The explicit `{base}-reality` direct proxy remains available independently of relay-group
  candidates.

## User Mixin Normalization

System normalization must preserve user intent while mapping legacy region references onto the current visible contract.

Required constraints:

- Legacy `🌟 {Region}` references from helper sections or user groups must normalize to canonical `🌟 {Region}`.
- Legacy `🤯 {Region}` references from helper sections or user groups must normalize to canonical `🌟 {Region}`.
- No generated system group named `🌟 节点选择` may appear.
- `💎 节点选择` must remain the only hidden fallback wrapper for node selection.
- User groups such as `🐟 漏网之鱼` and `🤖 AI` may reference `💎 节点选择`, but must not be rewritten to `🚀 节点选择` unless the user explicitly authored that visible selector.

## Provider Route Equivalence

The provider main config and the non-provider rendered config must expose the same owner-facing group graph.

Required constraints:

- The visible and hidden region-group relationship must be identical on both routes.
- The high-quality and node-selector aggregate contracts must be semantically identical on both
  routes.
  Provider mode consumes raw Reality candidates through the system provider because those nodes only
  exist in provider payloads.
- Provider mode must not reintroduce `🛬 {base}` or chain proxies into `🔒 {Region}`.
- Provider mode must keep hidden relay groups outside the visible region/high-quality cluster.
- Provider and non-provider routes must expose the same landing candidate order: reality direct,
  ss-chain, reality-chain.
