# 节点与用户 Traffic 统计实现状态（#r26nc）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: 已完成
- Lifecycle: active
- Catalog note: Node/User traffic rollups and detail tabs

## Coverage / rollout summary

- Backend sampling, retention, lifecycle cleanup, mirror/fan-out APIs and schema v2 are implemented.
- Node/User Traffic tabs share `TrafficView`; UTC range preference and fixed previous-period comparison are covered by Storybook stories.
- Gap and partial states are covered by the same Storybook stories.
- Traffic and TCP now share an ECharts SVG palette hook, themed tooltip surface,
  dashed axis pointer and static line emphasis guard. The palette re-resolves token
  colors when UiPrefs changes the resolved theme and after a persisted theme is first
  applied to the document root.
- Traffic tooltip content has a unit test, a deterministic Storybook preview and
  Playwright pointer-hover coverage in dark/light plus a 390px boundary check.
  TCP retains its hover regression coverage through the same helper.
- IP usage was audited but intentionally remains outside the helper: its linked pointer
  events and custom-series interaction are distinct from the static Traffic/TCP contract.

## Remaining Gaps

- No implementation or evidence delivery gaps remain. Owner approved the final page-fallback visual evidence for commit and PR convergence.

## Related Changes

- `k7m2n-node-history-fallback`

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../solutions/web/echarts-svg-tooltip-theming-hover-stability.md`
