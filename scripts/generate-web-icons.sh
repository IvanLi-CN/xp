#!/usr/bin/env sh
set -eu

# Generate favicon / PWA icons and the inline web mark from canonical sources.
# This script is a developer tool (assets are committed); CI/build does not depend on
# ImageMagick or librsvg.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICON_IN="${1:-$ROOT/web/assets-src/xp-icon-source.png}"
MARK_IN="${2:-$ROOT/web/assets-src/xp-mark-source.png}"
MASKABLE_IN="${3:-$ROOT/web/assets-src/xp-logo-bicolor-maskable.svg}"
OUTDIR="$ROOT/web/public"

if ! command -v magick >/dev/null 2>&1; then
  echo "error: missing dependency: magick (ImageMagick)" >&2
  exit 1
fi

if ! command -v rsvg-convert >/dev/null 2>&1; then
  echo "error: missing dependency: rsvg-convert (librsvg)" >&2
  exit 1
fi

if [ ! -f "$ICON_IN" ]; then
  echo "error: missing source icon at: $ICON_IN" >&2
  exit 1
fi

if [ ! -f "$MARK_IN" ]; then
  echo "error: missing source mark at: $MARK_IN" >&2
  exit 1
fi

if [ ! -f "$MASKABLE_IN" ]; then
  echo "error: missing maskable source at: $MASKABLE_IN" >&2
  exit 1
fi

mkdir -p "$OUTDIR"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

MASK0="$TMP/mask0.png"
FLOOD="$TMP/flood.png"
FLOOD_INV="$TMP/flood_inv.png"
MASK="$TMP/mask.png"
MARK1024="$TMP/mark1024.png"
TRIMMED="$TMP/trimmed.png"
CANONICAL="$TMP/canonical.png"

# Build an alpha mask that keeps the icon body while removing the low-saturation gray background
# (and the bottom-right sparkle). Then fill "holes" caused by white strokes/nodes.
magick "$ICON_IN" -colorspace HSB -channel G -separate +channel -blur 0x2 -threshold 10% "$MASK0"
magick "$MASK0" -fill white -draw "color 0,0 floodfill" "$FLOOD"
magick "$FLOOD" -negate "$FLOOD_INV"
magick "$MASK0" "$FLOOD_INV" -compose lighten -composite "$MASK"

# Apply the mask as alpha; keep original RGB as-is for maximum fidelity.
magick "$ICON_IN" "$MASK" -alpha off -compose copyopacity -composite "$MARK1024"

# Make a canonical square icon with reasonable padding. Trim any source margin before
# re-extending to a stable 1024^2 canvas for the generated platform icons.
magick "$MARK1024" -trim +repage "$TRIMMED"
magick "$TRIMMED" -resize 960x960 -gravity center -background none -extent 1024x1024 "$CANONICAL"

# Export a standard set of icons into web/public/ (Vite copies them into web/dist root).
magick "$CANONICAL" -define icon:auto-resize=16,32,48 "$OUTDIR/favicon.ico"
magick "$CANONICAL" -resize 16x16 "$OUTDIR/favicon-16x16.png"
magick "$CANONICAL" -resize 32x32 "$OUTDIR/favicon-32x32.png"
magick "$CANONICAL" -resize 180x180 "$OUTDIR/apple-touch-icon.png"
magick "$CANONICAL" -resize 192x192 "$OUTDIR/android-chrome-192x192.png"
magick "$CANONICAL" -resize 512x512 "$OUTDIR/android-chrome-512x512.png"
rsvg-convert -w 192 -h 192 "$MASKABLE_IN" -o "$OUTDIR/android-chrome-192x192-maskable.png"
rsvg-convert -w 512 -h 512 "$MASKABLE_IN" -o "$OUTDIR/android-chrome-512x512-maskable.png"
# The inline mark has its own transparent source so the hexagonal app icon does
# not leak into the product header and login surfaces.
magick "$MARK_IN" -background none -trim +repage -resize 960x960 -gravity center -background none -extent 1024x1024 -resize 256x256 "$OUTDIR/xp-mark.png"
cp "$ROOT/web/assets-src/xp-logo-bicolor.svg" "$OUTDIR/xp-logo-bicolor.svg"
cp "$ROOT/web/assets-src/xp-logo-monochrome.svg" "$OUTDIR/safari-pinned-tab.svg"

echo "generated icons into: $OUTDIR" >&2
