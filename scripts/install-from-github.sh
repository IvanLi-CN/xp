#!/usr/bin/env sh
set -eu

REPO="${XP_GITHUB_REPO:-IvanLi-CN/xp}"
VERSION="${XP_VERSION:-latest}"
INSTALL_DIR="${XP_INSTALL_DIR:-/usr/local/bin}"

usage() {
  cat <<'EOF'
install-from-github.sh

Download and install `xp` and `xp-ops` from GitHub Releases (Linux musl binaries).

Usage:
  sh install-from-github.sh [--repo OWNER/REPO] [--version latest|SEMVER|vSEMVER] [--install-dir PATH] [--dry-run]

Environment variables:
  XP_GITHUB_REPO   (default: IvanLi-CN/xp)
  XP_VERSION       (default: latest)
  XP_INSTALL_DIR   (default: /usr/local/bin)
EOF
}

log() {
  printf '%s\n' "$*" >&2
}

die() {
  log "error: $*"
  exit 1
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    die "missing dependency: $1"
  fi
}

download() {
  url="$1"
  dest="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
    return 0
  fi

  if command -v wget >/dev/null 2>&1; then
    wget -qO "$dest" "$url"
    return 0
  fi

  die "missing dependency: curl or wget"
}

sha256_file() {
  path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
    return 0
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
    return 0
  fi

  if command -v openssl >/dev/null 2>&1; then
    # openssl output: "SHA2-256(path)= <hex>"
    openssl dgst -sha256 "$path" | awk '{print $2}'
    return 0
  fi

  die "missing dependency: sha256sum (or shasum/openssl)"
}

expected_sha256() {
  checksums_path="$1"
  asset_name="$2"

  # checksums.txt format: "<sha256>  <filename>"
  sha="$(awk -v name="$asset_name" '$2==name {print $1; exit 0}' "$checksums_path" || true)"
  if [ -z "${sha:-}" ]; then
    die "checksum_mismatch: missing ${asset_name} in checksums.txt"
  fi
  printf '%s' "$sha"
}

verify_asset() {
  checksums_path="$1"
  asset_name="$2"
  file_path="$3"

  expected="$(expected_sha256 "$checksums_path" "$asset_name")"
  actual="$(sha256_file "$file_path")"

  if [ "$actual" != "$expected" ]; then
    die "checksum_mismatch: ${asset_name}"
  fi
}

backup_if_exists() {
  sudo_prefix="$1"
  path="$2"
  ts="$3"

  if [ -e "$path" ]; then
    if [ -n "$sudo_prefix" ]; then
      $sudo_prefix mv "$path" "${path}.bak.${ts}"
    else
      mv "$path" "${path}.bak.${ts}"
    fi
  fi
}

install_bin() {
  sudo_prefix="$1"
  src="$2"
  dest="$3"

  if command -v install >/dev/null 2>&1; then
    if [ -n "$sudo_prefix" ]; then
      $sudo_prefix install -m 0755 "$src" "$dest"
    else
      install -m 0755 "$src" "$dest"
    fi
    return 0
  fi

  if [ -n "$sudo_prefix" ]; then
    $sudo_prefix cp "$src" "$dest"
    $sudo_prefix chmod 0755 "$dest"
  else
    cp "$src" "$dest"
    chmod 0755 "$dest"
  fi
}

DRY_RUN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --repo)
      [ $# -ge 2 ] || die "--repo requires a value"
      REPO="$2"
      shift 2
      ;;
    --version)
      [ $# -ge 2 ] || die "--version requires a value"
      VERSION="$2"
      shift 2
      ;;
    --install-dir)
      [ $# -ge 2 ] || die "--install-dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift 1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      die "unknown argument: $1"
      ;;
  esac
done

os="$(uname -s)"
if [ "$os" != "Linux" ]; then
  die "unsupported_platform: OS=$os (linux only)"
fi

arch="$(uname -m)"
case "$arch" in
  x86_64|amd64)
    platform="x86_64"
    ;;
  aarch64|arm64)
    platform="aarch64"
    ;;
  *)
    die "unsupported_platform: ARCH=$arch (supported: x86_64, aarch64)"
    ;;
esac

xp_asset="xp-linux-${platform}"
xp_ops_asset="xp-ops-linux-${platform}"
checksums_asset="checksums.txt"

if [ "$VERSION" = "latest" ]; then
  base_url="https://github.com/${REPO}/releases/latest/download"
else
  v="$VERSION"
  case "$v" in
    v*) ;;
    *) v="v${v}" ;;
  esac
  base_url="https://github.com/${REPO}/releases/download/${v}"
fi

need_cmd awk
need_cmd uname
need_cmd date

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

checksums_path="${tmp_dir}/${checksums_asset}"
xp_path="${tmp_dir}/${xp_asset}"
xp_ops_path="${tmp_dir}/${xp_ops_asset}"

log "repo: ${REPO}"
log "version: ${VERSION}"
log "platform: linux/${platform}"
log "install dir: ${INSTALL_DIR}"

log "downloading: ${checksums_asset}"
download "${base_url}/${checksums_asset}" "$checksums_path"

log "downloading: ${xp_ops_asset}"
download "${base_url}/${xp_ops_asset}" "$xp_ops_path"
log "downloading: ${xp_asset}"
download "${base_url}/${xp_asset}" "$xp_path"

log "verifying checksums..."
verify_asset "$checksums_path" "$xp_ops_asset" "$xp_ops_path"
verify_asset "$checksums_path" "$xp_asset" "$xp_path"

chmod 0755 "$xp_ops_path" "$xp_path" 2>/dev/null || true

dest_xp_ops="${INSTALL_DIR}/xp-ops"
dest_xp="${INSTALL_DIR}/xp"

if [ "$DRY_RUN" = "1" ]; then
  log "dry-run: would install ${xp_ops_asset} -> ${dest_xp_ops}"
  log "dry-run: would install ${xp_asset} -> ${dest_xp}"
  exit 0
fi

sudo_prefix=""
if [ "$(id -u)" -ne 0 ]; then
  # If INSTALL_DIR does not exist yet, try to create it as the current user first.
  # This avoids creating user-home paths as root-owned when `sudo` is available.
  if [ ! -d "$INSTALL_DIR" ]; then
    if mkdir -p "$INSTALL_DIR" 2>/dev/null; then
      :
    else
      if command -v sudo >/dev/null 2>&1; then
        sudo mkdir -p "$INSTALL_DIR"
      else
        die "permission_denied: cannot create ${INSTALL_DIR} (run as root or install sudo)"
      fi
    fi
  fi

  if [ ! -w "$INSTALL_DIR" ] || { [ -e "$dest_xp_ops" ] && [ ! -w "$dest_xp_ops" ]; } || { [ -e "$dest_xp" ] && [ ! -w "$dest_xp" ]; }; then
    if command -v sudo >/dev/null 2>&1; then
      sudo_prefix="sudo"
    else
      die "permission_denied: need write access to ${INSTALL_DIR} (run as root or install sudo)"
    fi
  fi
fi

ts="$(date +%s)"

if [ -n "$sudo_prefix" ]; then
  $sudo_prefix mkdir -p "$INSTALL_DIR"
else
  mkdir -p "$INSTALL_DIR"
fi

backup_if_exists "$sudo_prefix" "$dest_xp_ops" "$ts"
backup_if_exists "$sudo_prefix" "$dest_xp" "$ts"

install_bin "$sudo_prefix" "$xp_ops_path" "$dest_xp_ops"
install_bin "$sudo_prefix" "$xp_path" "$dest_xp"

log "installed: ${dest_xp_ops}"
log "installed: ${dest_xp}"

if "$dest_xp_ops" --version >/dev/null 2>&1; then
  "$dest_xp_ops" --version >&2 || true
fi
if "$dest_xp" --version >/dev/null 2>&1; then
  "$dest_xp" --version >&2 || true
fi
