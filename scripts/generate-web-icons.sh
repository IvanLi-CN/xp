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
ASSET_META="$ROOT/web/assets-src/icon-assets.json"

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

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

sha256_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | cut -d ' ' -f 1
  else
    shasum -a 256 | cut -d ' ' -f 1
  fi
}

MASK0="$TMP/mask0.png"
FLOOD="$TMP/flood.png"
FLOOD_INV="$TMP/flood_inv.png"
MASK="$TMP/mask.png"
MARK1024="$TMP/mark1024.png"
TRIMMED="$TMP/trimmed.png"
CANONICAL="$TMP/canonical.png"
APPLE="$TMP/apple-touch-icon.png"
REGULAR192="$TMP/android-chrome-192x192.png"
REGULAR512="$TMP/android-chrome-512x512.png"
MASKABLE192="$TMP/android-chrome-192x192-maskable.png"
MASKABLE512="$TMP/android-chrome-512x512-maskable.png"
FAVICON="$TMP/favicon.ico"
FAVICON16="$TMP/favicon-16x16.png"
FAVICON32="$TMP/favicon-32x32.png"
BICOLOR_SVG="$TMP/xp-logo-bicolor.svg"
PINNED_TAB_SVG="$TMP/safari-pinned-tab.svg"

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
magick "$CANONICAL" -define icon:auto-resize=16,32,48 "$FAVICON"
magick "$CANONICAL" -resize 16x16 "$FAVICON16"
magick "$CANONICAL" -resize 32x32 "$FAVICON32"
# Apple touch icons must be opaque, uncropped squares. Use the full-bleed
# maskable source as the platform-owned square derivation.
rsvg-convert -w 180 -h 180 "$MASKABLE_IN" -o "$APPLE"
magick "$CANONICAL" -resize 192x192 "$REGULAR192"
magick "$CANONICAL" -resize 512x512 "$REGULAR512"
rsvg-convert -w 192 -h 192 "$MASKABLE_IN" -o "$MASKABLE192"
rsvg-convert -w 512 -h 512 "$MASKABLE_IN" -o "$MASKABLE512"
# The inline mark has its own transparent source so the hexagonal app icon does
# not leak into the product header and login surfaces.
magick "$MARK_IN" -background none -trim +repage -resize 960x960 -gravity center -background none -extent 1024x1024 -resize 256x256 "$OUTDIR/xp-mark.png"
cp "$ROOT/web/assets-src/xp-logo-bicolor.svg" "$BICOLOR_SVG"
cp "$ROOT/web/assets-src/xp-logo-monochrome.svg" "$PINNED_TAB_SVG"

# Keep stable legacy aliases for existing installations, while all current
# install metadata points at content-versioned URLs below.
cp "$FAVICON" "$OUTDIR/favicon.ico"
cp "$FAVICON16" "$OUTDIR/favicon-16x16.png"
cp "$FAVICON32" "$OUTDIR/favicon-32x32.png"
cp "$APPLE" "$OUTDIR/apple-touch-icon.png"
cp "$REGULAR192" "$OUTDIR/android-chrome-192x192.png"
cp "$REGULAR512" "$OUTDIR/android-chrome-512x512.png"
cp "$MASKABLE192" "$OUTDIR/android-chrome-192x192-maskable.png"
cp "$MASKABLE512" "$OUTDIR/android-chrome-512x512-maskable.png"
cp "$BICOLOR_SVG" "$OUTDIR/xp-logo-bicolor.svg"
cp "$PINNED_TAB_SVG" "$OUTDIR/safari-pinned-tab.svg"

ICON_VERSION="$(printf '%s\n' \
	"$(sha256_file "$ICON_IN")" \
	"$(sha256_file "$MARK_IN")" \
	"$(sha256_file "$MASKABLE_IN")" \
	"$(sha256_file "$ROOT/web/assets-src/xp-logo-bicolor.svg")" \
	"$(sha256_file "$ROOT/web/assets-src/xp-logo-monochrome.svg")" | sha256_stdin | cut -c 1-12)"

versioned_name() {
  printf '%s.%s.%s\n' "$1" "$ICON_VERSION" "$2"
}

FAVICON_V="$(versioned_name favicon ico)"
FAVICON16_V="$(versioned_name favicon-16x16 png)"
FAVICON32_V="$(versioned_name favicon-32x32 png)"
APPLE_V="$(versioned_name apple-touch-icon png)"
REGULAR192_V="$(versioned_name android-chrome-192x192 png)"
REGULAR512_V="$(versioned_name android-chrome-512x512 png)"
MASKABLE192_V="$(versioned_name android-chrome-192x192-maskable png)"
MASKABLE512_V="$(versioned_name android-chrome-512x512-maskable png)"
BICOLOR_SVG_V="$(versioned_name xp-logo-bicolor svg)"
PINNED_TAB_SVG_V="$(versioned_name safari-pinned-tab svg)"

# Remove only previously generated content-versioned files. Stable aliases are
# retained for older installed clients and are not referenced by new metadata.
find "$OUTDIR" -maxdepth 1 -type f \( \
  -name 'favicon.*.ico' \
  -o -name 'favicon-16x16.*.png' \
  -o -name 'favicon-32x32.*.png' \
  -o -name 'apple-touch-icon.*.png' \
  -o -name 'android-chrome-192x192.*.png' \
  -o -name 'android-chrome-512x512.*.png' \
  -o -name 'android-chrome-192x192-maskable.*.png' \
  -o -name 'android-chrome-512x512-maskable.*.png' \
  -o -name 'xp-logo-bicolor.*.svg' \
  -o -name 'safari-pinned-tab.*.svg' \
\) -delete

cp "$FAVICON" "$OUTDIR/$FAVICON_V"
cp "$FAVICON16" "$OUTDIR/$FAVICON16_V"
cp "$FAVICON32" "$OUTDIR/$FAVICON32_V"
cp "$APPLE" "$OUTDIR/$APPLE_V"
cp "$REGULAR192" "$OUTDIR/$REGULAR192_V"
cp "$REGULAR512" "$OUTDIR/$REGULAR512_V"
cp "$MASKABLE192" "$OUTDIR/$MASKABLE192_V"
cp "$MASKABLE512" "$OUTDIR/$MASKABLE512_V"
cp "$BICOLOR_SVG" "$OUTDIR/$BICOLOR_SVG_V"
cp "$PINNED_TAB_SVG" "$OUTDIR/$PINNED_TAB_SVG_V"

cat > "$ASSET_META" <<EOF
{
	"version": "$ICON_VERSION",
	"favicon": "$FAVICON_V",
	"favicon16": "$FAVICON16_V",
	"favicon32": "$FAVICON32_V",
	"appleTouch": "$APPLE_V",
	"regular192": "$REGULAR192_V",
	"regular512": "$REGULAR512_V",
	"maskable192": "$MASKABLE192_V",
	"maskable512": "$MASKABLE512_V",
	"bicolorSvg": "$BICOLOR_SVG_V",
	"pinnedTabSvg": "$PINNED_TAB_SVG_V"
}
EOF

cat > "$ROOT/web/public/site.webmanifest" <<EOF
{
	"name": "xp",
	"short_name": "xp",
	"start_url": "/",
	"scope": "/",
	"display": "standalone",
	"theme_color": "#4CB1AB",
	"background_color": "#ffffff",
	"icons": [
		{
			"src": "/$REGULAR192_V",
			"sizes": "192x192",
			"type": "image/png",
			"purpose": "any"
		},
		{
			"src": "/$REGULAR512_V",
			"sizes": "512x512",
			"type": "image/png",
			"purpose": "any"
		},
		{
			"src": "/$MASKABLE192_V",
			"sizes": "192x192",
			"type": "image/png",
			"purpose": "maskable"
		},
		{
			"src": "/$MASKABLE512_V",
			"sizes": "512x512",
			"type": "image/png",
			"purpose": "maskable"
		}
	]
}
EOF

perl -0pi -e "s#href=\"/favicon(?:\.[0-9a-f]{12})?\.ico\"#href=\"/$FAVICON_V\"#g; s#href=\"/xp-logo-bicolor(?:\.[0-9a-f]{12})?\.svg\"#href=\"/$BICOLOR_SVG_V\"#g; s#href=\"/favicon-16x16(?:\.[0-9a-f]{12})?\.png\"#href=\"/$FAVICON16_V\"#g; s#href=\"/favicon-32x32(?:\.[0-9a-f]{12})?\.png\"#href=\"/$FAVICON32_V\"#g; s#href=\"/apple-touch-icon(?:\.[0-9a-f]{12})?\.png\"#href=\"/$APPLE_V\"#g; s#href=\"/safari-pinned-tab(?:\.[0-9a-f]{12})?\.svg\"#href=\"/$PINNED_TAB_SVG_V\"#g" "$ROOT/web/index.html"

echo "generated icons into: $OUTDIR" >&2
