#!/usr/bin/env bash
set -euo pipefail

# Install the osv-scanner prebuilt binary from GitHub releases.
#
# Version comes from tools.toml (single source of truth).
# Installs to $OSV_SCANNER_PREFIX (default: ~/.cargo/bin) which is already
# on PATH for anyone who uses the cargo-install tools in install-tools.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OSV_SCANNER_VERSION="$(bash "$SCRIPT_DIR/tool-version.sh" osv-scanner)"

INSTALL_DIR="${OSV_SCANNER_PREFIX:-$HOME/.cargo/bin}"

get_installed_version() {
  osv-scanner --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || echo ""
}

# Skip if already at the required version
if command -v osv-scanner > /dev/null 2>&1; then
  INSTALLED_VER="$(get_installed_version)"
  if [[ "$INSTALLED_VER" == "$OSV_SCANNER_VERSION" ]]; then
    echo "osv-scanner $OSV_SCANNER_VERSION is already installed."
    exit 0
  fi
  echo "Installed version ($INSTALLED_VER) differs from required ($OSV_SCANNER_VERSION)"
fi

# Detect OS
OS_RAW="$(uname -s)"
case "$OS_RAW" in
  Linux*) OS=linux ;;
  Darwin*) OS=darwin ;;
  MINGW* | MSYS* | CYGWIN*) OS=windows ;;
  *)
    echo "Error: unsupported OS: $OS_RAW" >&2
    exit 1
    ;;
esac

# Detect architecture
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
  x86_64 | amd64) ARCH=amd64 ;;
  aarch64 | arm64) ARCH=arm64 ;;
  *)
    echo "Error: unsupported architecture: $ARCH_RAW" >&2
    exit 1
    ;;
esac

EXT=""
if [[ "$OS" == "windows" ]]; then
  EXT=".exe"
fi

ASSET="osv-scanner_${OS}_${ARCH}${EXT}"
BASE_URL="https://github.com/google/osv-scanner/releases/download/v${OSV_SCANNER_VERSION}"

echo "Installing osv-scanner ${OSV_SCANNER_VERSION} for ${OS}/${ARCH}..."

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT
cd "$TMP_DIR"

echo "Downloading ${ASSET}..."
curl --retry 5 --retry-delay 5 -fLsS -o "$ASSET" "${BASE_URL}/${ASSET}"

echo "Downloading checksums..."
curl --retry 5 --retry-delay 5 -fLsS -o osv-scanner_SHA256SUMS "${BASE_URL}/osv-scanner_SHA256SUMS"

# Verify checksum. macOS ships shasum; Linux ships sha256sum; either can read
# the osv-scanner_SHA256SUMS format (`<hash>  <file>`).
echo "Verifying checksum..."
EXPECTED_HASH="$(awk -v asset="$ASSET" '$2 == asset { print $1; exit }' osv-scanner_SHA256SUMS)"
if [[ -z "$EXPECTED_HASH" ]]; then
  echo "Error: could not find checksum for $ASSET in osv-scanner_SHA256SUMS" >&2
  exit 1
fi

if command -v sha256sum > /dev/null 2>&1; then
  ACTUAL_HASH="$(sha256sum "$ASSET" | awk '{print $1}')"
elif command -v shasum > /dev/null 2>&1; then
  ACTUAL_HASH="$(shasum -a 256 "$ASSET" | awk '{print $1}')"
else
  echo "Error: neither sha256sum nor shasum is available for checksum verification" >&2
  exit 1
fi

if [[ "$ACTUAL_HASH" != "$EXPECTED_HASH" ]]; then
  echo "Error: checksum mismatch for $ASSET" >&2
  echo "  Expected: $EXPECTED_HASH" >&2
  echo "  Actual:   $ACTUAL_HASH" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
TARGET="${INSTALL_DIR}/osv-scanner${EXT}"
mv "$ASSET" "$TARGET"
chmod +x "$TARGET"

# Final verification. If another osv-scanner shadows $INSTALL_DIR on PATH,
# surface it instead of silently pointing at the old one.
if ! command -v osv-scanner > /dev/null 2>&1; then
  echo "osv-scanner installed to $TARGET"
  echo "Warning: $INSTALL_DIR is not on PATH. Add it to use osv-scanner directly."
  exit 0
fi

FINAL_VER="$(get_installed_version)"
if [[ "$FINAL_VER" != "$OSV_SCANNER_VERSION" ]]; then
  echo "Error: version mismatch after install" >&2
  echo "  Required: $OSV_SCANNER_VERSION" >&2
  echo "  Found:    $FINAL_VER (at $(command -v osv-scanner))" >&2
  echo "Another osv-scanner binary may be shadowing $TARGET on PATH." >&2
  exit 1
fi

echo "osv-scanner installed successfully:"
osv-scanner --version
