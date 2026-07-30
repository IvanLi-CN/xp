---
title: Theme ECharts SVG tooltips and disable static-series emphasis
module: web
problem_type: chart-hover-regression
component: echarts-svg
tags:
  - echarts
  - svg
  - tooltip
  - theme
  - hover
status: active
related_specs:
  - docs/specs/r26nc-node-user-traffic-analytics/SPEC.md
  - docs/specs/m4n7c-node-tcp-connection-count/SPEC.md
---

# Theme ECharts SVG tooltips and disable static-series emphasis

## Context

Traffic and TCP connection charts render through ECharts with the SVG renderer. The admin UI
supports light and dark themes through CSS tokens and `UiPrefs` resolved theme state.

## Symptoms

- A dark-theme Traffic tooltip opened with ECharts' white default surface.
- Repeated pointer hover could make a static line and its area disappear from the SVG.
- CSS `var()` and `color-mix()` expressions were passed directly into chart options, creating a
  renderer boundary that was not represented by a stable color value.

## Root cause

ECharts does not inherit the application's tooltip surface tokens unless each tooltip option
provides them. Its emphasis state can rewrite SVG line and area presentation for static series.
The prior TCP fix disabled emphasis locally, but did not define a reusable contract for the next
chart. Browser CSS expressions are also not a durable SVG option value when a chart library owns
the renderer lifecycle. `UiPrefs` applies a restored root theme after the initial React render, so
a palette resolved only during render can otherwise keep values from the previous root theme.

## Resolution

- Use `useEChartsThemePalette` from `web/src/components/echarts-theme.ts` for Traffic and TCP.
  It subscribes to `UiPrefs` resolved theme, resolves CSS expressions through a browser color probe,
  and returns SVG-safe `rgba()` values. `UiPrefs` applies a restored root theme in a layout effect;
  the hook then runs a layout-phase revision before paint, so charts do not retain pre-restore token
  colors.
- Build tooltip presentation through `createThemedTooltipSurface`. It supplies the popover surface,
  text, border, shadow and a dashed axis pointer. The tooltip caps width against the viewport and
  permits wrapping on narrow displays. Formatter HTML must use a preferred `width` capped by
  `100%`, never a fixed `min-width` that can exceed the confined ECharts tooltip.
- Set `confine: true` on every static chart tooltip. Responsive formatter content constrains size;
  ECharts confinement separately constrains the tooltip position within the chart.
- In light theme, use a `0 4px 12px` shadow with 18% foreground opacity. Do not reuse the global
  `--xp-overlay` token there: its 58% opacity and dark-mode elevation are too heavy against a
  white popover. Dark tooltips retain their stronger existing elevation.
- Add `STATIC_LINE_SERIES_EMPHASIS` to every static line series that opens an axis tooltip. This
  prevents ECharts emphasis from replacing the normal SVG stroke or fill during hover.
- Escape dynamic text through `escapeEChartsHtml` before using ECharts HTML formatters.

## Boundaries

- Apply this helper to static Traffic and TCP series. They need a stable visual tooltip and no
  series-level hover animation.
- Do not use this as a shortcut for IP usage. IP usage owns cross-chart time/IP highlighting,
  `updateAxisPointer` events and custom series; preserve that interaction contract. Its existing
  line emphasis guard remains compatible but does not justify changing its event model.

## Verification

- `cd web && bun run test -- TrafficView.test.tsx TcpConnectionUsageView.test.tsx`
- `cd web && STORYBOOK_BASE_URL=http://127.0.0.1:<leased-port>`
  `E2E_REUSE_EXISTING_SERVER=1 bunx playwright test -c`
  `playwright.storybook.config.ts tests/storybook/traffic-hover.spec.ts`
  `tests/storybook/tcp-connections-hover.spec.ts`
- Inspect the Storybook `Components/TrafficView/TooltipPreview` state and the real pointer-hover
  screenshot in both themes.

## Evidence Boundaries

- Capture visual evidence directly from the product page fallback or the component story. Do not add
  a Storybook decorator solely to create a contrasting canvas, crop margin, or screenshot frame.
- A capture utility must adapt to the existing product surface. It must not introduce a background,
  border, padding wrapper, or other visible visual treatment that would not ship to administrators.
- When component and page evidence need different framing, prefer the page fallback for the primary
  desktop proof and use `trim_only` capture metadata rather than changing the component story. If a
  transparent component cannot satisfy a component-margin verifier without a synthetic frame,
  classify that artifact as page evidence instead of forcing the component path.

## Checklist

- Resolve CSS tokens before assigning SVG-relevant ECharts colors.
- Re-resolve the palette after a persisted theme is applied to the document root; depending only on
  the already-resolved preference misses that initial CSS-token transition.
- Give every new axis tooltip an explicit themed surface and axis pointer.
- Disable `emphasis` only for static series; audit linked or custom-series charts first.
- Test dark and light tooltip colors, visible line and area paths after pointer hover, and a narrow
  viewport tooltip boundary below every formatter's preferred width.
- Assert that the rendered Traffic story contains no screenshot-only colored evidence frame before
  accepting visual artifacts.
