# Mihomo Subscription Node Import 主题历史

> 本文件记录主题生命周期、兼容性和背景；规范正文以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- 2026-10-24: 新增 active 主题；Profile 字段和保存 API 保持不变。

## Replacements / Background

- The topic makes copied subscription content a static per-user `extra_proxies_yaml` snapshot, not a live `proxy-provider`.
- [ADR 0028](../../adr/0028-mihomo-static-subscription-import.md) records the static snapshot boundary.

## Related Changes

- 2026-10-24: `cca3c1455960cbd93b8bda22fba493c012865756` added the parser, API, Web Dialog, and focused tests.
- 2026-10-24: The implementation was hardened with bounded iterative YAML preflight before deserialization, explicit block-scalar and compact-sequence handling, NEL/LS/PS rejection, Hysteria2 credential delimiter coverage, and the formatted style budget baseline; full local Rust/Web/Storybook gates passed.
- 2026-10-24: The hardening implementation was recorded in signed-off commit `51c5f544e797dcbf12f77e319b12a12538501647`.
- 2026-10-24: The 3.22 inventory keeps the historical `v3.22.5` source commit, records the additive import contract under `contractSourceCommit`, and separates historical `serverApiRoutes`; 3.21/3.20 omit import support.
- 2026-10-24: Storybook preview evidence covers desktop and Storybook-native 393x852 mobile layouts; Add and explicit Save were observed as separate draft and persistence actions.
- 2026-10-24: The final read-only API/security, Rust data-flow, and Web state/visual review surfaces reported no P0-P2 findings; no PR, merge, push, or release was performed.

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
