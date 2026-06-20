# Final Mihomo Config Contract

## Scope

This contract governs the final rendered Mihomo main configuration returned by:

- `GET /api/sub/{subscription_token}?format=mihomo`
- `GET /api/sub/{subscription_token}/mihomo/provider`

It is the normative source for system-managed `proxy-groups` topology after mixin merge. If this contract conflicts with shorter summaries in `SPEC.md` or `http-apis.md`, this file wins for final Mihomo group shape.

## Stable System Group Surface

The final Mihomo main config must keep these system-managed groups stable:

- Region triplets:
  - `🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
  - `🔒 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
  - `🤯 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
- High-quality groups:
  - `🔒 高质量`
  - `💎 高质量`
- Node-selection fallback groups:
  - `🚀 节点选择`
  - `💎 节点选择`
- Global probe group:
  - `🤯 All`
- Landing groups:
  - `🛬 {base}`
  - `🔒 落地`
- Relay groups:
  - `🛣️ {relay-base}`

The system must not synthesize or rename a hidden selector fallback into `🌟 节点选择`. The hidden fallback selector name is `💎 节点选择`.

## Region Group Topology

For every managed region in canonical order `Japan / HongKong / Taiwan / Korea / Singapore / US / Other`, the final config must emit a fixed triplet with these roles:

### `🔒 {Region}`

- `hidden` must be absent or `false`
- `type` must be `select`
- it is the visible manual-selection region group
- it must contain only that region's direct leaf candidates

Allowed members:

- direct system access candidates such as `{base}-reality`
- non-system direct candidates that belong to the same region

Forbidden members:

- `🛬 {base}`
- `*-ss-chain`
- `*-reality-chain`
- `DIRECT`
- any other system aggregate group
- any other region group

Reference restriction:

- `🔒 {Region}` may only be referenced by its paired `🌟 {Region}`

### `🌟 {Region}`

- `hidden: true`
- `type: fallback`
- it is the hidden fallback wrapper for the visible region group
- its `proxies` must be exactly `["🔒 {Region}"]`

This is the owner-facing system region entry that other system/user groups may reference when they want a regional fallback option without exposing the manual leaf list inline.

### `🤯 {Region}`

- `hidden: true`
- `type: url-test`
- it is the hidden active-probe view for the same region leaf set
- it must not reference `🔒 {Region}`
- it must not reference `🌟 {Region}`
- it must contain only that region's direct leaf candidates

Its allowed and forbidden leaf membership rules are the same as `🔒 {Region}`.

## Aggregate Group Topology

### `🔒 高质量`

- visible owner-facing manual group
- `type: select`
- it must include region entries, landing entries, and raw reality access entries

Required shape:

1. canonical region entries: `🌟 Japan`, `🌟 HongKong`, `🌟 Taiwan`, `🌟 Korea`, `🌟 Singapore`, `🌟 US`, `🌟 Other`
2. zero or more `🛬 {base}` landing groups
3. zero or more raw `{base}-reality` direct entries

Forbidden members:

- `🔒 {Region}`
- `🤯 {Region}`
- `*-ss-chain`
- `*-reality-chain`

### `💎 高质量`

- `hidden: true`
- `type: fallback`
- its `proxies` must be exactly `["🔒 高质量", "🤯 All"]`

### `🚀 节点选择`

- visible node-selection group
- its exact business-facing option set is owned by the merged mixin + system injection flow
- if the system emits regional managed options here, they must use `🌟 {Region}` rather than `🔒 {Region}`

### `💎 节点选择`

- `hidden: true`
- `type: fallback`
- its `proxies` must be exactly `["🚀 节点选择", "🤯 All"]`

Explicit mixin references to `💎 节点选择` must remain `💎 节点选择`; the render flow must not rewrite them to `🌟 节点选择`.

### `🤯 All`

- `hidden: true`
- `type: url-test`
- it is the global active-probe wrapper
- it must reference the per-region probe groups in canonical order:
  - `🤯 Japan`
  - `🤯 HongKong`
  - `🤯 Taiwan`
  - `🤯 Korea`
  - `🤯 Singapore`
  - `🤯 US`
  - `🤯 Other`

## Landing And Relay Constraints

### `🛬 {base}`

- it is a landing-only group
- it may expose `*-ss-chain` and `*-reality-chain`
- it must not be injected into any region group

### `🛣️ {relay-base}`

- hidden relay infrastructure group
- it must not be injected into any region group
- it must not be injected into `🔒 高质量`

## Forbidden Graph Shapes

The final Mihomo config must reject these shapes:

- `🔒 {Region} -> 🌟 {Region}`
- `🤯 {Region} -> 🌟 {Region}`
- `🤯 {Region} -> 🔒 {Region}`
- any region group containing `🛬 {base}`
- any region group containing `*-ss-chain` or `*-reality-chain`
- any visible aggregate that directly references `🔒 {Region}`
- any generated hidden selector fallback named `🌟 节点选择`

## Ordering Constraints

Where the system injects canonical system-managed options, it must keep these orders stable:

- region order: `Japan`, `HongKong`, `Taiwan`, `Korea`, `Singapore`, `US`, `Other`
- `🔒 高质量` order:
  1. `🌟 {Region}` in canonical region order
  2. `🛬 {base}` groups
  3. raw `{base}-reality` entries
- `🤯 All` order:
  1. `🤯 Japan`
  2. `🤯 HongKong`
  3. `🤯 Taiwan`
  4. `🤯 Korea`
  5. `🤯 Singapore`
  6. `🤯 US`
  7. `🤯 Other`

## Acceptance Mapping

An implementation or test run only satisfies this contract when all of the following are true:

- every region emits `🔒 / 🌟 / 🤯` together
- `🔒 {Region}` is visible and leaf-only
- `🌟 {Region}` is hidden fallback and wraps only `🔒 {Region}`
- `🤯 {Region}` is hidden url-test and probes the same leaf set directly
- `🔒 高质量` uses `🌟 {Region}`, never `🔒 {Region}`
- `💎 高质量` and `💎 节点选择` remain hidden fallback wrappers
- no final YAML path reintroduces `🌟 节点选择`
