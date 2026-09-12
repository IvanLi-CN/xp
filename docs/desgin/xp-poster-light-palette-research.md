# XP Light Poster Palette Research

## Decision

Use the official [Radix Colors](https://www.radix-ui.com/colors) `Slate` neutral scale with its
`Mint` accent scale. This is a two-scale system, not a recolored dark poster:

- `Slate` owns the light canvas, surfaces, device volume, rules, and background grid. It
  prevents the background from reading as green.
- `Mint` owns readable green ink, active connections, selected-state fills, and secondary
  graphic detail.
- The approved XP mark remains an immutable brand exception: outer network and links are
  `#4CB1AB`; the hub is `#72CCA3`. Do not substitute either with a nearby system token.

Radix defines steps 1-2 as backgrounds, 3-5 as component backgrounds, 6-8 as borders, 9-10
as solid fills, and 11-12 as text. It explicitly identifies `Mint` as a scale whose solid
steps 9-10 require dark foreground text. [Radix scale guidance](https://www.radix-ui.com/
colors/docs/palette-composition/understanding-the-scale) and the
[official source values](https://github.com/radix-ui/colors/blob/main/src/light.ts) are the
source of every non-brand color below.

## Role Mapping

- Canvas: `slate1`, `#FCFCFD`.
  Full 4:5 background. Keep it flat and neutral.
- Quiet band or secondary surface: `slate2`, `#F9F9FB`.
  Footer field and recessed diagram zones.
- Light object face: `slate3`, `#F0F0F3`.
  Server and control-plane faces; never render them charcoal.
- Surface separation: `slate6`, `#D9D9E0`.
  Rules, card seams, and sparse grid strokes.
- Strong structural edge: `slate7`, `#CDCED6`.
  Device outlines and selected neutral edges only.
- Display green ink: `mint12`, `#16433C`.
  Headline, compact proof labels, and any green copy below display size.
- Supporting green ink: `mint11`, `#027864`.
  Eyebrows and large supporting labels. Do not use for small body copy.
- Brand-compatible midtone: `mint8`, `#4CBBA5`.
  Active routes, icon strokes, and diagram outlines. It is close to the outer mark color but does
  not replace it.
- Soft selected fill: `mint3`, `#DDF9F2`.
  Desired-state slab, low-emphasis node halo, and pale information panels.
- Selected fill or hover-depth plane: `mint4`, `#C8F4E9`.
  A second plane in the central mechanism. Use `mint12` for any overlaid text.
- Brand network and links: XP approved outer, `#4CB1AB`.
  The exact six-node mark and only its immediate brand echoes.
- Brand hub or live point: XP approved hub, `#72CCA3`.
  The exact hub and sparse active-state markers; never use as text on a light field.
- Body copy: `slate11`, `#60646C`.
  Supporting sentence and URL. It remains neutral so green retains hierarchy.

The authoritative `Slate` values are
[lines 112-124](https://github.com/radix-ui/colors/blob/main/src/light.ts#L112-L124). `Mint`
values are in the [Mint source range][radix-mint].
The approved XP values are in
[web/assets-src/xp-logo-bicolor.svg](../../web/assets-src/xp-logo-bicolor.svg:9).

## Poster Application

1. Set the canvas to `slate1`; use `slate6` at reduced opacity for the hex grid. The grid must
   be neutral, not green.
2. Draw the central control plane and all managed nodes with `slate2`/`slate3` faces and
   `slate6`/`slate7` structure. This makes the illustration light-first rather than a dark
   object painted pale.
3. Use `mint3`/`mint4` for only the desired-state plane and selected data surfaces. Use
   `mint8` for the active topology paths.
4. Set the headline and proof labels in `mint12`. Set body copy in `slate11`. Use `mint11`
   only for large secondary labels; it is not the default text color.
5. Keep the XP mark exactly `#4CB1AB` plus `#72CCA3`. The hub and `mint9`/`mint10` are light
   solids, so pair them with `mint12`, not white, when they carry text.

## Project Alignment

The existing Web theme uses a high-chroma cyan primary (`oklch(63% 0.22 205)`) and blue-gray
foregrounds, as defined in [web/src/styles.css](../../web/src/styles.css:41).
That application token set is correct for the product UI but is not a poster palette: copying
its cyan foreground and dark device treatment caused the rejected light-poster direction. The
approved logo instead fixes the public brand signal to `#4CB1AB` and `#72CCA3`; this palette
preserves those values and builds a mature light composition around them.

## Exclusions

- No green canvas, green footer field, or dark device shell.
- No near-black `slate12` headline or device face.
- No blue accent family and no unreferenced one-off hex values.
- No white text on `mint9` or `mint10`; Radix documents those two Mint solid steps for dark
  foreground text.

[radix-mint]: https://github.com/radix-ui/colors/blob/main/src/light.ts#L1456-L1468
