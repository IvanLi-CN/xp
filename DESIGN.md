---
name: xp
description: Self-hosted Xray cluster control plane with a restrained operational UI.
colors:
  background: "oklch(98.5% 0.01 205)"
  foreground: "oklch(20% 0.03 240)"
  card: "oklch(100% 0 0)"
  primary: "oklch(63% 0.22 205)"
  primary-foreground: "oklch(98% 0.01 205)"
  secondary: "oklch(93% 0.04 44)"
  secondary-foreground: "oklch(25% 0.02 45)"
  muted: "oklch(95% 0.01 215)"
  muted-foreground: "oklch(45% 0.03 240)"
  accent: "oklch(96% 0.02 205)"
  accent-foreground: "oklch(24% 0.03 240)"
  border: "oklch(89% 0.01 240)"
  input: "oklch(86% 0.01 240)"
  info: "oklch(66% 0.14 230)"
  success: "oklch(69% 0.17 150)"
  warning: "oklch(78% 0.16 85)"
  destructive: "oklch(59% 0.22 25)"
  dark-background: "oklch(18% 0.02 250)"
  dark-foreground: "oklch(95% 0.01 205)"
  dark-card: "oklch(22% 0.02 250)"
  dark-primary: "oklch(70% 0.18 205)"
  dark-border: "oklch(30% 0.02 250)"
typography:
  display:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "0"
  headline:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "0"
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0"
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "0"
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.33
    letterSpacing: "0"
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \"Liberation Mono\", monospace"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "0"
rounded:
  sm: "0.75rem"
  md: "0.875rem"
  lg: "1rem"
  xl: "1.25rem"
spacing:
  page: "1.5rem"
  page-compact: "1rem"
  card: "1.5rem"
  card-compact: "1rem"
  table-y: "0.75rem"
  table-x: "0.75rem"
  table-compact-y: "0.5rem"
  table-compact-x: "0.625rem"
  field-gap: "0.5rem"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 1rem"
    height: "2.5rem"
  button-compact:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-foreground}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: "0.375rem 0.75rem"
    height: "2rem"
  input-default:
    backgroundColor: "{colors.background}"
    textColor: "{colors.foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 0.75rem"
    height: "2.5rem"
  badge-status:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.accent-foreground}"
    typography: "{typography.label}"
    rounded: "{rounded.xl}"
    padding: "0.125rem 0.625rem"
  table-shell:
    backgroundColor: "{colors.card}"
    textColor: "{colors.foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.lg}"
---

# Design System: xp

## 1. Overview

**Creative North Star: "The Quiet Control Room"**

xp feels like a compact control room for one careful operator: status is always visible, actions are close to the resource they affect, and the interface stays composed when the cluster is unhealthy. The system uses familiar product UI patterns because the operator is here to manage nodes, endpoints, users, quotas, subscriptions, and runtime configuration, not to admire a brand surface.

The physical scene is a self-hosted operator checking a cluster from a laptop or 27-inch monitor during a maintenance window, sometimes late at night, with logs nearby and limited patience for visual noise. That scene supports both `xp-light` for normal work and `xp-dark` for dim-room maintenance; neither theme is decorative.

The visual system rejects cyber cosplay, neon hacker motifs, terminal rain, circuit decoration, hero metrics, decorative gradients, and repeated identical card grids. It favors dense tables, restrained status badges, clear copy, and a stable app shell built on Tailwind CSS v4, shadcn/ui, Radix primitives, Sonner, and Iconify Tabler icons.

**Key Characteristics:**

- Restrained operational color with cyan as a rare action and selection accent.
- Native-feeling system typography, tuned for compact labels and machine values.
- Low elevation, thin borders, tonal layers, and rounded but not playful surfaces.
- Persistent `system`, `light`, and `dark` theme preference through `xp_ui_theme`.
- Persistent `comfortable` and `compact` density preference through `xp_ui_density`.

## 2. Colors

The palette is a cool operational neutral system with one saturated cyan action voice, amber warmth only for secondary neutral contrast, and semantic colors reserved for state.

### Primary

- **Cluster Cyan** (`oklch(63% 0.22 205)`): Primary actions, active navigation, current selection, focus rings, and key links. Use it sparingly; the accent should identify the active path or action, not decorate inactive content.
- **Night Shift Cyan** (`oklch(70% 0.18 205)`): Dark theme primary. It stays bright enough for action recognition without becoming neon.

### Secondary

- **Warm Config Surface** (`oklch(93% 0.04 44)`): Secondary badges, neutral emphasis, and low-risk grouping in light theme. This prevents the product from reading as a single blue-gray scale.
- **Deep Utility Surface** (`oklch(29% 0.03 250)`): Dark theme secondary surface for sidebars, menus, and grouped controls.

### Tertiary

- **Info Blue** (`oklch(66% 0.14 230)`): Informational badges and backend health states.
- **Quota Green** (`oklch(69% 0.17 150)`): Success and healthy quota state.
- **Cycle Amber** (`oklch(78% 0.16 85)`): Warning states, partial cluster responses, and risky but recoverable conditions.
- **Failure Red** (`oklch(59% 0.22 25)`): Destructive actions, failed requests, and states where connection or policy is broken.

### Neutral

- **Pale Control Surface** (`oklch(98.5% 0.01 205)`): Light theme page background.
- **Ink Slate** (`oklch(20% 0.03 240)`): Light theme body text and high-importance labels.
- **Panel White** (`oklch(100% 0 0)`): Current card and popover surface. Do not expand this into new pure-white layers unless replacing the existing token.
- **Soft Blue Mute** (`oklch(95% 0.01 215)`): Muted rows, subtle panels, and skeleton-like quiet states.
- **Hairline Border** (`oklch(89% 0.01 240)`): Dividers, table frames, input borders, and panel outlines.
- **Deep Control Surface** (`oklch(18% 0.02 250)`): Dark theme page background.
- **Night Panel** (`oklch(22% 0.02 250)`): Dark theme card and popover surface.
- **Night Hairline** (`oklch(30% 0.02 250)`): Dark theme dividers and panel outlines.

### Named Rules

**The One Action Voice Rule.** Primary cyan is for action, selection, links, and focus. It should not be sprinkled through inactive cards, icons, or decoration.

**The State Color Rule.** Info, success, warning, and destructive colors are semantic. Do not reuse them for brand flavor.

## 3. Typography

**Display Font:** system sans (`-apple-system`, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif)\
**Body Font:** system sans (`-apple-system`, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif)\
**Label/Mono Font:** system sans for labels; `ui-monospace`, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace for IDs, tokens, URLs, ports, and quota values.

**Character:** The type system should feel native, efficient, and exact. It relies on weight, alignment, and spacing rather than display fonts or oversized headings.

### Hierarchy

- **Display** (600, `1.5rem`, `1.2`): Page titles and the most important shell-level headings only.
- **Headline** (600, `1.25rem`, `1.25`): Section headers in dashboards, detail pages, and configuration screens.
- **Title** (600, `1.125rem`, `1`): Card titles, table group labels, and dialog titles.
- **Body** (400, `0.875rem`, `1.5`): Primary interface copy, descriptions, table body text, and form help. Keep prose to 65 to 75ch when it is explanatory; data tables may run wider.
- **Label** (500, `0.75rem`, `1.33`): Badges, compact buttons, table headers, field labels, and metadata.
- **Mono** (500, `0.75rem`, `1.4`): Machine values. Use consistent wrapping and copy affordances so values remain inspectable.

### Named Rules

**The Product Type Rule.** Do not introduce display fonts for labels, buttons, tables, data, or navigation. One tuned system family is the product voice.

## 4. Elevation

xp uses a hybrid of thin borders, tonal layering, and very small shadows. Depth should identify surfaces and interactive state without making the admin UI feel like a stack of floating cards. Most panels are flat at rest with `border-border/60` or `border-border/70`; shadows stay at `shadow-xs` or `shadow-sm`.

### Shadow Vocabulary

- **Hairline Lift** (`box-shadow: var(--tw-shadow-xs)` through Tailwind `shadow-xs`): Inputs, keyboard hints, and low-risk controls that need tactile separation.
- **Panel Lift** (`box-shadow: var(--tw-shadow-sm)` through Tailwind `shadow-sm`): Cards, table shells, and primary buttons at rest.
- **Focus Ring** (`box-shadow: 0 0 0 3px color-mix(in oklab, var(--ring) 20%, transparent)` as Tailwind `focus-visible:ring-[3px] focus-visible:ring-ring/20`): Keyboard and validation focus. This is interaction feedback, not decorative glow.

### Named Rules

**The Flat First Rule.** If a surface can be explained with a border and tonal background, do that before adding elevation.

## 5. Components

Components should feel precise and repeatable. They use shadcn/ui primitives, app-level wrappers, and Iconify Tabler icons through the `Icon` component.

### Buttons

- **Shape:** Rounded rectangle with `rounded-xl` for default buttons and `rounded-lg` for compact buttons.
- **Primary:** `bg-primary text-primary-foreground`, `h-10 px-4 py-2`, `text-sm font-medium`, and `shadow-sm`.
- **Hover / Focus:** Hover darkens the background to `primary/90`; focus uses the shared 3px ring. Disabled states use opacity and block pointer events.
- **Secondary / Ghost / Danger:** Secondary uses outline or neutral surface treatment, ghost uses accent hover only, and danger uses `bg-destructive text-destructive-foreground`.
- **Icons:** Buttons can include Tabler icons, but icon-only controls must have accessible labels or tooltips where meaning is not obvious.

### Chips

- **Style:** Status chips are rounded full, compact, and semantically colored with low-alpha backgrounds such as `info/14`, `success/14`, `warning/18`, or `destructive/14`.
- **State:** Use chips for state and compact metadata, not decoration. Health, leader, term, alerts, quota, and endpoint status should read quickly and align across pages.

### Cards / Containers

- **Corner Style:** `rounded-2xl` for current cards and panels, backed by the root radius scale.
- **Background:** `bg-card` for primary panels, `bg-muted/35` for quiet panels, and `bg-background` for nested control surfaces.
- **Shadow Strategy:** `shadow-sm` only when a surface needs separation from the page background.
- **Border:** `border border-border/60` or `border-border/70`. Never use thick side-stripe accents.
- **Internal Padding:** `1.5rem` in comfortable density and `1rem` in compact density through `--xp-card-padding`.

### Inputs / Fields

- **Style:** `h-10`, `rounded-xl`, `border-input`, `bg-background`, `px-3 py-2`, and `text-sm`.
- **Focus:** Change border to `ring` and apply a 3px low-alpha ring.
- **Error / Disabled:** Error uses semantic destructive treatment near the field; disabled uses opacity plus blocked cursor. Preserve the submitted value when an API error returns.

### Navigation

- **Style:** App shell uses a stable sidebar plus top status area. Navigation groups are labeled, icons come from `tabler:`, and the current route is visibly selected.
- **Typography:** Labels stay compact and readable; route labels should match product nouns.
- **States:** Hover uses accent surface, active uses primary or accent with clear contrast, and mobile uses a Sheet-style drawer rather than a custom navigation pattern.

### Tables

- **Style:** Table shells use `xp-table-wrap` with horizontal overflow, rounded corners, a border, and `shadow-sm`.
- **Density:** Comfortable uses `0.75rem` cell padding; compact uses `0.5rem 0.625rem`.
- **Rows:** Zebra rows use `bg-muted/25`; separators use `border-border/60`.
- **Data:** IDs, URLs, tokens, ports, and quota values should use monospace treatment and predictable alignment.

### Command and Dialog Surfaces

- **Command Palette:** Command-K is an expert entry point, not a marketing flourish. Results should be grouped by operational destination.
- **Dialogs:** Use dialogs for blocking decisions, destructive confirmation, and focused forms that require isolation. Prefer inline or page-level flows for normal editing.
- **Sheets:** Use Sheet for mobile navigation or secondary panels where the user should remain oriented to the current page.

## 6. Do's and Don'ts

### Do:

- **Do** use `xp-light` and `xp-dark` through `UiPrefs`, with `xp_ui_theme` persisted as `system`, `light`, or `dark`.
- **Do** use `xp_ui_density` to drive comfortable and compact table, card, and form spacing.
- **Do** keep primary cyan rare: actions, active navigation, links, selection, and focus only.
- **Do** render every icon through the `Icon` component with `tabler:` names.
- **Do** keep operational values inspectable with monospace text, copy affordances, and safe wrapping.
- **Do** show loading, empty, error, disabled, focus, and destructive states for reusable components.

### Don't:

- **Don't** make the UI look like a cyber or hacker toy: no neon green console pastiche, terminal rain, circuit-board decoration, or fake intrusion aesthetics.
- **Don't** use hero metrics, sales copy, decorative gradients, glass cards, or repeated icon-card grids inside the authenticated product.
- **Don't** use `border-left` or `border-right` greater than `1px` as a colored side stripe on cards, list items, callouts, or alerts.
- **Don't** use gradient text, display fonts in UI labels, bounce or elastic motion, or page-load choreography.
- **Don't** introduce new pure black or pure white neutrals beyond existing compatibility tokens; tint new neutrals toward the product hue.
- **Don't** mix icon libraries or bypass the Tabler/Iconify wrapper.
