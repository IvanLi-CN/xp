# XP 4:5 Poster Layout Hierarchy Research

## Question

The 4:5 XP poster currently places the supporting paragraph and the upper edge of
the large cluster visual in the same vertical band. This note identifies how to
separate those elements so the poster has one clear reading sequence.

## Official Design-System Findings

1. [Apple HIG: Layout](https://developer.apple.com/design/human-interface-guidelines/layout)
   says to place the most important material near the top and leading edge in
   reading order, give essential information sufficient space, and align related
   components to make them easier to scan. This supports treating the headline
   and its explanation as one left-aligned text group, rather than letting the
   diagram intrude into it.
2. [Fluent 2: Layout](https://fluent2.microsoft.design/layout) states that
   proximity makes elements appear related, while extra space creates focus and
   hierarchy. It specifically advises using layout space to direct the eye to
   high-importance areas. The copy and the diagram therefore need a deliberate
   gap, not a near-tangent overlap.
3. The same [Fluent layout guidance](https://fluent2.microsoft.design/layout)
   defines a grid as the foundation for placement, hierarchy, and balance, and
   says the most important content should occupy the largest regions. The poster
   should use discrete text, diagram, proof-point, and footer regions instead of
   allowing a large decorative ring to float across their boundaries.
4. [Fluent 2: Typography](https://fluent2.microsoft.design/typography) says
   clear typographic hierarchy makes content easy to navigate, recommends
   left-alignment for English text, and reserves centered copy for a short
   message or to support another element. This favors one left-aligned copy
   column and a separate centered diagram, not a partially overlapping hybrid.

## Current Geometry

In [`xp-poster-4x5.svg`](../desgin/images/xp-poster-4x5.svg), the supporting
copy is set at baselines `y=835` and `y=890`. The large mark starts at roughly
`y=844`, while its outer circle starts at `y=728`; the proof-point rail begins at
`y=1438`. The top node therefore touches the supporting-copy band, and the outer
ring reaches into the headline's lower territory. The issue is a collision of
regions and visual weight, not just insufficient line spacing.

## Recommended Revision

Use a strict, stacked composition with four regions:

- Brand and promise (`y=116–733`): Keep the current mark, eyebrow, and two-line
  headline. This is the poster's primary reading sequence.
- Supporting copy (around `y=835`): Set the description as one left-aligned line:
  `Self-hosted management for nodes, endpoints, access, and quotas.` It ends
  before the diagram region begins.
- Diagram (`y=900–1400`): Center the approved cluster mark in this dedicated zone.
  Its visible edge, including rings, must stay inside the zone. Do not place a
  node or ring above `y=900`.
- Proof and action (`y=1438–1935`): Retain the 2x2 proof grid and one footer
  action as an independent evidence band.

For the existing 1600 by 2000 SVG, a concrete starting geometry is:

- Reduce the large mark from `scale(0.85)` to `scale(0.65)` and center it around
  `(1005, 1150)`, e.g. `translate(672 817) scale(0.65)`.
- Keep the concentric treatment subordinate: use no outer ring larger than about
  `r=248` at `(1005, 1150)`, so it stays within `y=902–1398` and ends at least
  40 px before the proof rail. Removing the outer ring altogether is preferable
  if it remains visually louder than the approved mark.
- Preserve at least about 55 px between the final supporting-copy line and the
  first visible diagram edge. This is a deliberate group-to-group gap, not a
  paragraph leading value.
- Do not solve the issue by moving only the top node: the enclosing circles are
  part of the perceived visual object and must obey the same diagram boundary.

This makes the intended reading order unambiguous: brand and promise, supporting
explanation, cluster visual, then four concrete proof points. It also keeps the
approved logo geometry intact, instead of redrawing or distorting it to fit an
ambiguous overlap.

## Rejected Layout Fixes

- **A right-floating diagram beside the paragraph:** It retains two competing
  focal points in the same band and makes the paragraph's endpoint visually run
  into the diagram.
- **More opacity reduction only:** Lowering the ring contrast does not repair the
  implied grouping created by proximity.
- **Scaling the mark without bounding the rings:** The visible visual object is
  still too large if its decoration crosses the text and proof regions.
