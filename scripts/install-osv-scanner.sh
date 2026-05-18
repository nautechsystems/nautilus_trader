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
INSTALL_ATTEMPTS="${INSTALL_ATTEMPTS:-5}"
CURL_RETRIES="${CURL_RETRIES:-5}"
CURL_CONNECT_TIMEOUT="${CURL_CONNECT_TIMEOUT:-20}"
CURL_MAX_TIME="${CURL_MAX_TIME:-300}"

if ! [[ "$INSTALL_ATTEMPTS" =~ ^[0-9]+$ ]] || [ "$INSTALL_ATTEMPTS" -lt 1 ]; then
  echo "INSTALL_ATTEMPTS must be a positive integer" >&2
  exit 1
fi

get_installed_version() {
  osv-scanner --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || echo ""
}

download_file() {
  local output_path="$1"
  local url="$2"

  curl -fLsS \
    --retry "$CURL_RETRIES" \
    --retry-all-errors \
    --connect-timeout "$CURL_CONNECT_TIMEOUT" \
    --max-time "$CURL_MAX_TIME" \
    -o "$output_path" "$url"
}

verify_checksum() {
  local expected_hash actual_hash

  expected_hash="$(awk -v asset="$ASSET" '$2 == asset { print $1; exit }' osv-scanner_SHA256SUMS)"
  if [[ -z "$expected_hash" ]]; then
    echo "Error: could not find checksum for $ASSET in osv-scanner_SHA256SUMS" >&2
    return 2
  fi

  if command -v sha256sum > /dev/null 2>&1; then
    actual_hash="$(sha256sum "$ASSET" | awk '{print $1}')"
  elif command -v shasum > /dev/null 2>&1; then
    actual_hash="$(shasum -a 256 "$ASSET" | awk '{print $1}')"
  else
    echo "Error: neither sha256sum nor shasum is available for checksum verification" >&2
    return 2
  fi

  if [[ "$actual_hash" != "$expected_hash" ]]; then
    echo "Error: checksum mismatch for $ASSET" >&2
    echo "  Expected: $expected_hash" >&2
    echo "  Actual:   $actual_hash" >&2
    return 3
  fi
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

verified=false
for attempt in $(seq 1 "$INSTALL_ATTEMPTS"); do
  rm -f "$ASSET" osv-scanner_SHA256SUMS

  echo "Downloading ${ASSET} (attempt ${attempt}/${INSTALL_ATTEMPTS})..."
  if ! download_file "$ASSET" "${BASE_URL}/${ASSET}"; then
    echo "Failed to download ${ASSET}"
  else
    echo "Downloading checksums..."
    if ! download_file osv-scanner_SHA256SUMS "${BASE_URL}/osv-scanner_SHA256SUMS"; then
      echo "Failed to download checksums"
    else
      echo "Verifying checksum..."
      if verify_checksum; then
        verified=true
        break
      elif [ "$?" -eq 2 ]; then
        exit 1
      fi
      echo "Checksum verification failed"
    fi
  fi

  if [ "$attempt" -lt "$INSTALL_ATTEMPTS" ]; then
    sleep $((2 ** attempt))
  fi
done

if [ "$verified" != "true" ]; then
  echo "Error: failed to download and verify osv-scanner assets after ${INSTALL_ATTEMPTS} attempts" >&2
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
