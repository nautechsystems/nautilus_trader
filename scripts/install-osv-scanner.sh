#!/usr/bin/env bash
set -euo pipefail

# Install the pinned OSV Scanner binary from its GitHub release and verify its checksum

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OSV_SCANNER_VERSION="$(bash "$SCRIPT_DIR/tool-version.sh" osv-scanner)"
STABLE_VERSION_PATTERN='^[0-9]+\.[0-9]+\.[0-9]+$'
VERSION_PATTERN='[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?'
VERSION_OUTPUT_PATTERN="(^|[^0-9A-Za-z.])(${VERSION_PATTERN})([^0-9A-Za-z.+-]|$)"

INSTALL_DIR="${OSV_SCANNER_PREFIX:-$HOME/.cargo/bin}"
INSTALL_ATTEMPTS="${INSTALL_ATTEMPTS:-5}"
CURL_RETRIES="${CURL_RETRIES:-5}"
CURL_CONNECT_TIMEOUT="${CURL_CONNECT_TIMEOUT:-20}"
CURL_MAX_TIME="${CURL_MAX_TIME:-300}"

if ! [[ "$OSV_SCANNER_VERSION" =~ $STABLE_VERSION_PATTERN ]]; then
  echo "Error: osv-scanner version pin must be a stable X.Y.Z release: $OSV_SCANNER_VERSION" >&2
  exit 1
fi

for setting in INSTALL_ATTEMPTS CURL_RETRIES CURL_CONNECT_TIMEOUT CURL_MAX_TIME; do
  value=${!setting}
  if ! [[ "$value" =~ ^[0-9]+$ ]] || [ "$value" -lt 1 ]; then
    echo "$setting must be a positive integer" >&2
    exit 1
  fi
done
if [ "$INSTALL_ATTEMPTS" -gt 10 ]; then
  echo "INSTALL_ATTEMPTS must not exceed 10" >&2
  exit 1
fi

get_version() {
  local executable=$1 output=""

  output=$("$executable" --version 2>&1) || true
  if [[ "$output" =~ $VERSION_OUTPUT_PATTERN ]]; then
    printf '%s' "${BASH_REMATCH[2]}"
  fi
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

if [[ -z "${OSV_SCANNER_PREFIX:-}" ]] && command -v osv-scanner > /dev/null 2>&1; then
  INSTALLED_VERSION="$(get_version "$(command -v osv-scanner)")"
  if [[ "$INSTALLED_VERSION" == "$OSV_SCANNER_VERSION" ]]; then
    echo "osv-scanner $OSV_SCANNER_VERSION is already installed."
    exit 0
  fi
  echo "Installed version ($INSTALLED_VERSION) differs from required ($OSV_SCANNER_VERSION)"
fi

case "$(uname -s)" in
  Linux*) OS=linux ;;
  Darwin*) OS=darwin ;;
  MINGW* | MSYS* | CYGWIN*) OS=windows ;;
  *)
    echo "Error: unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) ARCH=amd64 ;;
  aarch64 | arm64) ARCH=arm64 ;;
  *)
    echo "Error: unsupported architecture: $(uname -m)" >&2
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

INSTALL_DIR_REQUESTED="$INSTALL_DIR"
if ! mkdir -p "$INSTALL_DIR_REQUESTED" ||
  ! INSTALL_DIR=$(cd "$INSTALL_DIR_REQUESTED" && pwd -P); then
  echo "Error: could not create install directory: $INSTALL_DIR_REQUESTED" >&2
  exit 1
fi

TARGET="${INSTALL_DIR}/osv-scanner${EXT}"
if [[ -L "$TARGET" || (-e "$TARGET" && ! -f "$TARGET") ]]; then
  echo "Error: install target is not a regular file: $TARGET" >&2
  exit 1
fi
if [[ -n "${OSV_SCANNER_PREFIX:-}" && -f "$TARGET" ]]; then
  TARGET_VERSION="$(get_version "$TARGET")"
  if [[ "$TARGET_VERSION" == "$OSV_SCANNER_VERSION" ]]; then
    echo "osv-scanner $OSV_SCANNER_VERSION is already installed at $TARGET."
    exit 0
  fi
fi

TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT
cd "$TEMP_DIR"

verified=false
attempt=1
while [ "$attempt" -le "$INSTALL_ATTEMPTS" ]; do
  rm -f "$ASSET" osv-scanner_SHA256SUMS

  echo "Downloading ${ASSET} (attempt ${attempt}/${INSTALL_ATTEMPTS})..."
  if ! download_file "$ASSET" "${BASE_URL}/${ASSET}"; then
    echo "Failed to download ${ASSET}"
  elif ! download_file osv-scanner_SHA256SUMS "${BASE_URL}/osv-scanner_SHA256SUMS"; then
    echo "Failed to download checksums"
  elif verify_checksum; then
    verified=true
    break
  elif [ "$?" -eq 2 ]; then
    exit 1
  else
    echo "Checksum verification failed"
  fi

  if [ "$attempt" -lt "$INSTALL_ATTEMPTS" ]; then
    sleep $((2 ** attempt))
  fi
  attempt=$((attempt + 1))
done

if [ "$verified" != "true" ]; then
  echo "Error: failed to download and verify OSV Scanner after ${INSTALL_ATTEMPTS} attempts" >&2
  exit 1
fi

if ! chmod +x "$ASSET"; then
  echo "Error: could not make downloaded asset executable: $ASSET" >&2
  exit 1
fi
ASSET_VERSION="$(get_version "$TEMP_DIR/$ASSET")"
if [[ "$ASSET_VERSION" != "$OSV_SCANNER_VERSION" ]]; then
  echo "Error: downloaded asset version mismatch" >&2
  echo "  Required: $OSV_SCANNER_VERSION" >&2
  echo "  Found:    ${ASSET_VERSION:-no version output}" >&2
  exit 1
fi

TARGET_TEMP=$(mktemp "${INSTALL_DIR}/.osv-scanner.XXXXXX")
if ! cp "$ASSET" "$TARGET_TEMP" || ! chmod +x "$TARGET_TEMP" || ! mv -f "$TARGET_TEMP" "$TARGET"; then
  rm -f "$TARGET_TEMP"
  echo "Error: could not install osv-scanner to $TARGET" >&2
  exit 1
fi

# Drop any cached path to the previous binary before resolving it again
hash -r

if ! command -v osv-scanner > /dev/null 2>&1; then
  echo "osv-scanner installed to $TARGET"
  echo "Warning: $INSTALL_DIR is not on PATH. Add it to use osv-scanner directly."
  exit 0
fi

FINAL_VERSION="$(get_version "$(command -v osv-scanner)")"
if [[ "$FINAL_VERSION" != "$OSV_SCANNER_VERSION" ]]; then
  echo "Error: version mismatch after install" >&2
  echo "  Required: $OSV_SCANNER_VERSION" >&2
  echo "  Found:    $FINAL_VERSION (at $(command -v osv-scanner))" >&2
  echo "Another osv-scanner binary may be shadowing $TARGET on PATH." >&2
  exit 1
fi

echo "osv-scanner installed successfully:"
osv-scanner --version
