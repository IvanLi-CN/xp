# XP Text-Bearing Logo Lockup Research

## Question

How should XP pair its approved six-node mark with the `XP` wordmark without
turning either into an arbitrary decorative shape?

## Evidence

### A lockup, a pictogram, and a wordmark have separate jobs

- GitHub uses the full pictogram-plus-wordmark lockup where the name must be
  communicated, including external campaigns, press, and partner placements;
  it reserves the standalone pictogram for GitHub-owned or already-established
  contexts. Source: [GitHub Brand Guidelines, p. 18](
  https://brand.github.com/GitHub-BrandGuidelines-2026.pdf).
- GitHub likewise describes the full lockup as the corporate-recognition form,
  while its pictogram serves a wide range of already branded product surfaces.
  [GitHub Brand Toolkit: Logo](https://brand.github.com/foundations/logo)
- Atlassian distributes isolated app logomarks and wordmark lockups as distinct
  assets, and says an app logomark is appropriate when the Atlassian context is
  clear. [Atlassian Design: Logos](https://atlassian.design/foundations/logos)

**Implication for XP:** the repository has an approved network mark, but no
committed text-bearing lockup contract. The mark is therefore the only
established compact icon. The complete text-bearing logo must be introduced as
a separate, fixed asset; neither a bare `XP` wordmark nor a mark-plus-nearby
live text may be presented as the canonical external logo before that asset is
approved.

### The relationship must be designed as a system, not improvised per layout

- Google describes product lockups as a fixed arrangement of its logo and a
  product name. It created Product Sans from the same geometric language as the
  logo because independent per-product treatments would not scale consistently.
  [Google Sans: Evolving Google's Typeface](https://design.google/library/google-sans-flex-font)
- Google’s identity work distinguishes a compact mark for constrained contexts
  from the logotype, develops specifications for spacing and product lockups,
  and tests the logotype across sizes and weights for legibility.
  [Evolving the Google Logo Identity](https://design.google/library/evolving-google-identity)

**Implication for XP:** treat `xp-logo-bicolor.svg` as the finished mark and
the owner-selected custom `XP` wordmark direction as a candidate second
primitive. The lockup may position and scale the approved versions of those
primitives, but must not merge paths, place the mark in a letter counter,
redraw letters from the mark’s nodes, or invent a different `X` / `P` per
asset.

### Optical dimensions and clear space require explicit rules

- GitHub requires clear space equal to 50% of the logo height around its
  lockup. [GitHub Brand Guidelines, p. 18](https://brand.github.com/GitHub-BrandGuidelines-2026.pdf)
- Atlassian requires clear space free of type and graphics outside application
  UI, and measures minimum clearance from its wordmark rather than treating
  surrounding whitespace as accidental. Source: [Atlassian Design: Logos](
  https://atlassian.design/foundations/logos).
- Google’s compact `G` is derived from its wordmark but receives extra visual
  weight and optical refinement so it remains balanced at small sizes and next
  to other elements. Source: [Evolving the Google Logo Identity](
  https://design.google/library/evolving-google-identity).

**Implication for XP:** use visible artwork bounds, not the 1024-by-1024 SVG
canvas, when aligning the icon. The approved mark’s visible bounds are
`696 × 696` inside its source canvas; its transparent margin must not be
counted as lockup spacing. The current wordmark candidate has an authored
`430 × 200` artwork box. The final lockup needs a measured optical-size
comparison at intended sizes, rather than assuming equal source SVG dimensions
are visually equal.

## XP Construction Constraints

These are project decisions derived from the evidence above, not claims that a
single universal logo ratio applies to every brand.

1. After the wordmark is formally approved, define one primary **horizontal
   lockup**: approved six-node mark on the left, approved `XP` wordmark on the
   right, sharing a vertical optical centre. It is the only text-bearing
   external logo.
2. Build the icon from the approved mark’s visible square (`164 164 696 696`)
   or an equivalent cropped symbol. Never align the wordmark against its padded
   1024-unit source canvas.
3. Start the mark’s visible height at the wordmark’s 200-unit visible height;
   validate 94%, 100%, and 106% mark scales in a small, medium, and large
   render. Select one result by optical mass and preserve it as a fixed asset.
   Do not create a stack, embedded-letter, or variable-scale alternative until
   there is a separately approved need.
4. Set the internal mark-to-wordmark gap from the selected mark height and
   freeze it in the SVG. It must be intentionally larger than the wordmark’s
   internal letter gap, so it reads as two coordinated primitives rather than
   an accidental third letter. The exact value should be chosen from the three
   optical prototypes, not guessed from a poster layout.
5. Reserve external clear space of at least `0.5H`, where `H` is the full
   rendered lockup height, free of text, rules, patterns, and other marks. This
   adopts the conservative GitHub rule for the XP primary lockup.
6. After approval, export only canonical, outlined SVGs: bicolor mark plus
   dark wordmark for light fields, and bicolor mark plus inverse wordmark for
   dark fields. The mark’s approved `#4CB1AB` and `#72CCA3` palette is
   unchanged.
7. Keep three explicitly named asset roles:
   - `xp-logo-bicolor.svg`: compact icon for favicon, PWA, app navigation, and
     other space-constrained or clearly XP-owned contexts.
   - `xp-product-logo-bicolor.svg` / inverse: external-facing primary lockup
     for posters, social preview, README mastheads, press, and partner use.
   - `xp-wordmark.svg` / inverse: controlled supporting asset only, not a
     substitute for the external primary logo.

## Rejected Directions

- Do not treat an icon and a wordmark as independent poster objects whose
  spacing changes with every layout.
- Do not combine the mark into the `P` counter, the `X` crossing, or another
  letter shape. That edits both approved primitives and creates an untested
  third mark instead of a lockup.
- Do not use a vertical stack merely to make the bounding box look symmetric.
  It introduces a separate lockup with no constrained-surface role.
- Do not add a new color, outline, shadow, ring, tile, or background to make
  the relationship feel more deliberate. Atlassian expressly disallows
  unapproved color combinations, outlines, shadows, and complex backgrounds
  that weaken a logo’s clarity. Source: [Atlassian Design: Logos](
  https://atlassian.design/foundations/logos).
- Do not use a system font or editable text in the final logo export. The
  current custom wordmark must remain outlined and reproducible.

## Repository Status and Next Design Gate

The committed product UI currently uses only `xp-mark.png` plus ordinary text
(`xp`, `cluster manager`); it does not contain a lockup. The custom wordmark
SVGs and prior lockup studies are currently untracked workspace work, so they
do not establish a repository-approved construction rule. This is consistent
with the product’s stated brand personality: calm, precise, operations-focused
and explicitly not a cyber or marketing treatment. [Product Context](../../PRODUCT.md)

The next design gate is therefore approval of the wordmark and one lockup
prototype, followed by a canonical asset specification:

Produce exactly three horizontal, same-construction prototypes that vary only
the mark scale (`94%`, `100%`, `106%`) and the corresponding optical gap. Show
them at a shared display size on both light and dark fields, then choose one.
The selected result becomes the canonical external lockup and may replace the
poster header. No other composition changes should be made in that iteration.
