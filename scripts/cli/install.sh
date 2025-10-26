#!/usr/bin/env bash
set -euo pipefail

# Nautilus CLI installer
# - Downloads the latest nautilus binary for the current platform from R2
# - Verifies sha256 against the published checksums
# - Installs into /usr/local/bin or ~/.local/bin (override with -b)
# - Retries downloads up to 3 times with backoff and falls back to source build when possible

BASE_URL="${NAUTILUS_CLI_BASE_URL:-https://packages.nautechsystems.io/cli/nautilus-cli/latest}"
DEST_DIR=""
RETRIES=${NAUTILUS_CLI_RETRIES:-3}
BACKOFFS="1 2 4" # seconds; total 7s worst case

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
        aarch64 | arm64) echo "aarch64-unknown-linux-gnu" ;;
        *)
          echo "Unsupported Linux arch: $arch" >&2
          exit 1
          ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64) echo "x86_64-apple-darwin" ;;
        arm64) echo "aarch64-apple-darwin" ;;
        *)
          echo "Unsupported macOS arch: $arch" >&2
          exit 1
          ;;
      esac
      ;;
    MINGW* | MSYS* | CYGWIN*)
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
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum > /dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "No sha256 tool found (sha256sum/shasum)." >&2
    exit 1
  fi
}

# Append a cache-busting query param to reduce CDN staleness
cache_bust() {
  local url="$1"
  # Portable cache-buster: seconds + RANDOM (avoids BSD date %N)
  local ts
  ts="$(date +%s)${RANDOM:-0}"
  if [[ "$url" == *\?* ]]; then
    echo "${url}&_cb=${ts}"
  else
    echo "${url}?_cb=${ts}"
  fi
}

download_and_verify() {
  local tmp="$1"
  local art="$2"
  local base="$3"

  rm -f "$tmp/$art" "$tmp/checksums.txt"
  echo "Downloading: ${base}/${art}"
  curl -fsSL -o "$tmp/$art" "$(cache_bust "${base}/${art}")"
  echo "Fetching checksums..."
  curl -fsSL -o "$tmp/checksums.txt" "$(cache_bust "${base}/checksums.txt")"

  local expected actual
  expected="$(awk -v f="$art" '($2==f){print $1}' "$tmp/checksums.txt" || true)"
  if [ -z "$expected" ]; then
    echo "Checksum for $art not found in checksums.txt" >&2
    return 2
  fi

  actual="$(sha256_file "$tmp/$art")"
  if [ "$expected" != "$actual" ]; then
    echo "Checksum mismatch for $art" >&2
    echo "Expected: $expected" >&2
    echo "Actual:   $actual" >&2
    return 3
  fi
  echo "Checksum OK."
  return 0
}

install_from_archive() {
  local tmp="$1"
  local art="$2"

  local extract="$tmp/extract"
  mkdir -p "$extract"
  tar -xzf "$tmp/$art" -C "$extract"

  local bin_src
  bin_src="$(command ls "$extract"/nautilus 2> /dev/null || true)"
  [ -z "$bin_src" ] && {
    echo "Binary 'nautilus' not found in archive." >&2
    return 4
  }
  chmod +x "$bin_src"

  if [ -z "$DEST_DIR" ]; then
    if [ -w /usr/local/bin ]; then DEST_DIR="/usr/local/bin"; else DEST_DIR="${HOME}/.local/bin"; fi
  fi
  mkdir -p "$DEST_DIR"

  local install_cmd="install -m 0755"
  if ! command -v install > /dev/null 2>&1; then install_cmd="cp"; fi

  if [ -w "$DEST_DIR" ]; then
    $install_cmd "$bin_src" "$DEST_DIR/nautilus"
  else
    echo "Destination $DEST_DIR not writable. Try: sudo bash -s -- -b $DEST_DIR" >&2
    return 5
  fi

  echo "Installed to ${DEST_DIR}/nautilus"
  "${DEST_DIR}/nautilus" --version || true
  case ":$PATH:" in
    *":${DEST_DIR}:"*) ;;
    *) echo "Note: Add ${DEST_DIR} to PATH." ;;
  esac
}

fallback_build_from_source() {
  # Only attempt if this looks like a Nautilus repo checkout
  if [ -f "crates/cli/Cargo.toml" ]; then
    echo "Falling back to building from source (cargo install)"
    local build_success=0
    if command -v make > /dev/null 2>&1; then
      make install-cli || cargo install --path crates/cli --bin nautilus --locked --force || build_success=1
    else
      cargo install --path crates/cli --bin nautilus --locked --force || build_success=1
    fi

    if [ $build_success -ne 0 ]; then
      echo "Failed to build from source" >&2
      return 1
    fi

    # If a DEST_DIR is given, copy the installed binary there to preserve caller expectations
    local cargo_home
    cargo_home="${CARGO_HOME:-$HOME/.cargo}"
    if [ -n "${DEST_DIR}" ]; then
      mkdir -p "$DEST_DIR"
      if [ -x "$cargo_home/bin/nautilus" ]; then
        cp "$cargo_home/bin/nautilus" "$DEST_DIR/nautilus"
        echo "Copied cargo-installed binary to ${DEST_DIR}/nautilus"
      else
        echo "Warning: Could not find built binary at $cargo_home/bin/nautilus" >&2
        return 1
      fi
    else
      # No DEST_DIR provided; ensure cargo bin dir is on PATH or offer guidance
      if [ -x "$cargo_home/bin/nautilus" ]; then
        "$cargo_home/bin/nautilus" --version || true
        case ":$PATH:" in
          *":$cargo_home/bin:"*) ;;
          *) echo "Note: Add $cargo_home/bin to PATH to use 'nautilus' globally." ;;
        esac
      fi
    fi
    return 0
  fi
  echo "Source fallback not available (no crates/cli)." >&2
  return 1
}

main() {
  local target ext art tmp attempt
  target="$(detect_target)"
  ext="tar.gz"
  art="nautilus-${target}.${ext}"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  # Short-circuit: we do not publish macOS Intel prebuilt binaries.
  if [[ "$target" == "x86_64-apple-darwin" ]]; then
    echo "Prebuilt CLI for macOS Intel (x86_64) is not available."
    echo "Attempting to build from source..."
    if fallback_build_from_source; then
      echo "Fallback install from source succeeded."
      return 0
    fi
    echo "Could not install from source. Either run inside the NautilusTrader repo or use a supported platform (macOS arm64)." >&2
    return 1
  fi

  attempt=1
  while [ "$attempt" -le "$RETRIES" ]; do
    if download_and_verify "$tmp" "$art" "$BASE_URL"; then
      install_from_archive "$tmp" "$art"
      return 0
    fi
    if [ "$attempt" -lt "$RETRIES" ]; then
      local delay
      delay=$(echo "$BACKOFFS" | awk -v n=$attempt '{print $n}')
      delay=${delay:-2}
      echo "Retrying in ${delay}s (attempt $((attempt + 1))/$RETRIES)..."
      sleep "$delay"
    fi
    attempt=$((attempt + 1))
  done

  echo "Failed to install prebuilt CLI after $RETRIES attempts."
  if fallback_build_from_source; then
    echo "Fallback install succeeded."
    return 0
  fi
  echo "Giving up." >&2
  return 1
}

main "$@"
