# XP Product Poster Research

## Scope

This note turns established product-poster principles into a text-only art direction
for XP. It is a design brief, not a new product claim. Feature statements below are
limited to the repository's documented capabilities.

## Research Findings

Mature product posters work as a short, deliberate reading sequence rather than a
feature inventory.

1. Establish one dominant message, then add context and proof. Adobe describes a
   poster reading sequence as hook, context, then detail; it identifies message,
   hierarchy, imagery, typography, color, whitespace, and a CTA or key detail as
   core poster elements. Source: [Adobe Express: Poster layout design](
   https://www.adobe.com/express/learn/blog/poster-layout-design).
2. Put the most important content at the start of the reading order and align related
   elements to make the composition easy to scan. Source: [Apple HIG: Layout](
   https://developer.apple.com/design/human-interface-guidelines/layout).
3. Make hierarchy through a small, deliberate type scale. Font size, weight, and
   color can express importance, while too many typefaces weaken hierarchy and
   legibility. Source: [Apple HIG: Typography](
   https://developer.apple.com/design/human-interface-guidelines/typography).
4. Use space as structure: proximity signals relationship, and more surrounding
   whitespace makes an element feel more important. Source: [Fluent 2: Layout](
   https://fluent2.microsoft.design/layout).
5. Use brand color to anchor identity and primary focus, not to decorate every
   surface. Overusing it dilutes hierarchy; semantic colors need a consistent
   meaning. [Fluent 2: Color](https://fluent2.microsoft.design/color)
6. Preserve legibility and do not rely on color alone. Fluent specifies at least
   `4.5:1` contrast for standard text and `3:1` for large text; Apple likewise
   directs designers to validate contrast and readable type. Sources:
   [Fluent 2: Typography](https://fluent2.microsoft.design/typography) and
   [Apple HIG: Accessibility](
   https://developer.apple.com/design/human-interface-guidelines/accessibility).

## Information Hierarchy for a Technical Infrastructure Product

For a control-plane product, the poster should resolve four questions in order:

1. **What is it?** Product mark and name: `XP`.
2. **Why does it matter?** One outcome-led statement, not a collection of protocol
   names.
3. **How does it help?** Three short, verifiable capability statements.
4. **Where does the reader go next?** One persistent CTA: the GitHub repository or
   a QR code resolving to it.

This order keeps a technical reader from having to parse an architecture diagram
before understanding the product. It also avoids treating logo, feature copy,
technical labels, and CTA as competing headlines.

## Proposed XP Poster: Text Description

### Core Message

Use the existing XP mark as the immediate brand signal and set `XP` beside it. The
single headline reads:

> One control plane for your Xray cluster.

The supporting line reads:

> Self-hosted management for nodes, endpoints, access, and quotas.

The wording is grounded in XP's documented role as a multi-host Xray cluster manager
with centralized endpoint, user, quota, and subscription management.

### Composition

Use a calm, dark landscape canvas. In the upper-left reading origin, place the XP
mark and product name; below it, give the headline the largest type and a short,
two-line maximum measure. Reserve the center-right as the sole visual: a precise
six-node network derived from the approved logo geometry. A smaller central node
represents the control plane; six equal outer nodes form the managed cluster. Keep
the diagram abstract and non-data-bearing: no invented uptime, latency, node count,
or status claims.

Set three proof points as a compact, left-aligned row or stack beneath the headline:

- `Raft-backed desired state`
- `Xray runtime reconciliation`
- `Systemd, OpenRC, and container deployment`

These are documented XP capabilities. Do not add a fourth item or turn this band
into an exhaustive feature list. Put one QR code and the GitHub repository address
at the lower edge as the only action. Avoid release versions, dates, and comparison
claims because they age quickly.

### Visual Direction

Use the approved two-color system without introducing a third accent: teal
`#4CB1AB` and green `#72CCA3`, with white and near-black as neutral support colors.
The teal should hold the cluster's main structural form; green should identify only
the central controller and the active path. The remaining nodes and copy stay white
or neutral. This makes the control relationship legible without turning a product
poster into a status dashboard.

Choose one sans-serif family with two or three weights. The headline is bold, the
supporting line regular, and technical proof points medium. Keep generous empty
space around the logo/headline block and the network graphic. Centered text is
reserved for the brief headline only; all explanatory copy shares one left edge for
scanning. Check final text/background combinations against the contrast thresholds
above, and pair any color-coded controller/path distinction with position or label.

### Explicit Exclusions

- No decorative gradients, neon glow, artificial 3D depth, or stock server imagery.
- No UI screenshot collage, dense topology diagram, raw configuration, or credentials.
- No unsupported claims about speed, availability, security, scale, or number of
  managed nodes.
- No competing QR codes, app-store badges, or multiple calls to action.

## Repository Evidence

The project description and listed capabilities are documented in
[`README.md`](../../README.md). The approved two-color mark uses teal `#4CB1AB` and
green `#72CCA3` in
[`web/assets-src/xp-logo-bicolor.svg`](../../web/assets-src/xp-logo-bicolor.svg).
