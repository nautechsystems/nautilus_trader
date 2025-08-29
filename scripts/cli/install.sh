#!/usr/bin/env bash
set -euo pipefail

# Nautilus CLI installer
# - Downloads the latest nautilus binary for the current platform from R2
# - Verifies sha256 against the published checksums
# - Installs into /usr/local/bin or ~/.local/bin (override with -b)

BASE_URL="${NAUTILUS_CLI_BASE_URL:-https://packages.nautechsystems.io/cli/nautilus-cli/latest}"
DEST_DIR=""

usage() {
  echo "Usage: install.sh [-b /install/dir]" >&2
  exit 1
}

while getopts "b:h" opt; do
  case "$opt" in
    b) DEST_DIR="$OPTARG" ;;
    h) usage ;;
    *) usage ;;
  esac
done

detect_target() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)
  case "$os" in
    Linux)
      case "$arch" in
        x86_64) echo "x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
        *) echo "Unsupported Linux arch: $arch" >&2; exit 1 ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64) echo "x86_64-apple-darwin" ;;
        arm64) echo "aarch64-apple-darwin" ;;
        *) echo "Unsupported macOS arch: $arch" >&2; exit 1 ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "Windows shell detected. Please use PowerShell installer." >&2
      exit 1
      ;;
    *)
      echo "Unsupported OS: $os" >&2
      exit 1
      ;;
  esac
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "No sha256 tool found (sha256sum/shasum)." >&2
    exit 1
  fi
}

TARGET="$(detect_target)"
EXT="tar.gz"
ART="nautilus-${TARGET}.${EXT}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading: ${BASE_URL}/${ART}"
curl -fsSL -o "${TMPDIR}/${ART}" "${BASE_URL}/${ART}"

echo "Fetching checksums..."
curl -fsSL -o "${TMPDIR}/checksums.txt" "${BASE_URL}/checksums.txt"

EXPECTED="$(awk -v f="${ART}" '($2==f){print $1}' "${TMPDIR}/checksums.txt" || true)"
if [ -z "${EXPECTED}" ]; then
  echo "Checksum for ${ART} not found in checksums.txt" >&2
  exit 1
fi

ACTUAL="$(sha256_file "${TMPDIR}/${ART}")"
if [ "${EXPECTED}" != "${ACTUAL}" ]; then
  echo "Checksum mismatch for ${ART}" >&2
  echo "Expected: ${EXPECTED}" >&2
  echo "Actual:   ${ACTUAL}" >&2
  exit 1
fi
echo "Checksum OK."

EXTRACT="${TMPDIR}/extract"
mkdir -p "${EXTRACT}"
tar -xzf "${TMPDIR}/${ART}" -C "${EXTRACT}"

BIN_SRC="$(command ls "${EXTRACT}"/nautilus 2>/dev/null || true)"
[ -z "${BIN_SRC}" ] && { echo "Binary 'nautilus' not found in archive." >&2; exit 1; }
chmod +x "${BIN_SRC}"

if [ -z "${DEST_DIR}" ]; then
  if [ -w /usr/local/bin ]; then DEST_DIR="/usr/local/bin"; else DEST_DIR="${HOME}/.local/bin"; fi
fi
mkdir -p "${DEST_DIR}"

INSTALL_CMD="install -m 0755"
if ! command -v install >/dev/null 2>&1; then INSTALL_CMD="cp"; fi

if [ -w "${DEST_DIR}" ]; then
  ${INSTALL_CMD} "${BIN_SRC}" "${DEST_DIR}/nautilus"
else
  echo "Destination ${DEST_DIR} not writable. Try: sudo bash -s -- -b ${DEST_DIR}" >&2
  exit 1
fi

echo "Installed to ${DEST_DIR}/nautilus"
"${DEST_DIR}/nautilus" --version || true
case ":$PATH:" in
  *":${DEST_DIR}:"*) ;;
  *) echo "Note: Add ${DEST_DIR} to PATH." ;;
esac

