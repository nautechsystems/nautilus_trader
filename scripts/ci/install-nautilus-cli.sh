#!/usr/bin/env bash
set -euo pipefail

# Install Nautilus CLI from prebuilt tarball with retries.
# Falls back to building from source if needed.
# Set NAUTILUS_CLI_FORCE_SOURCE=1 to always build from source (e.g., on nightly branch).

BIN_DIR="${BIN_DIR:-"$HOME/.local/bin"}"
export PATH="$BIN_DIR:$PATH"

INSTALL_URL="https://packages.nautechsystems.io/cli/nautilus-cli/install.sh"
INSTALL_ATTEMPTS="${INSTALL_ATTEMPTS:-5}"
CURL_RETRIES="${CURL_RETRIES:-5}"
CURL_CONNECT_TIMEOUT="${CURL_CONNECT_TIMEOUT:-20}"
CURL_MAX_TIME="${CURL_MAX_TIME:-300}"

if ! [[ "$INSTALL_ATTEMPTS" =~ ^[0-9]+$ ]] || [ "$INSTALL_ATTEMPTS" -lt 1 ]; then
  echo "INSTALL_ATTEMPTS must be a positive integer" >&2
  exit 1
fi

cargo_install_cli() {
  for attempt in $(seq 1 "$INSTALL_ATTEMPTS"); do
    if cargo install -q --path crates/cli --bin nautilus --locked --force --root "$HOME/.local"; then
      return 0
    fi

    if [ "$attempt" -lt "$INSTALL_ATTEMPTS" ]; then
      echo "cargo install failed, retrying... (attempt ${attempt}/${INSTALL_ATTEMPTS})"
      sleep $((2 ** attempt))
    else
      echo "cargo install failed (attempt ${attempt}/${INSTALL_ATTEMPTS})"
    fi
  done

  return 1
}

# Check if forced to build from source
if [ "${NAUTILUS_CLI_FORCE_SOURCE:-0}" = "1" ]; then
  echo "Building Nautilus CLI from source (NAUTILUS_CLI_FORCE_SOURCE=1)..."
  cargo_install_cli
else
  echo "Installing Nautilus CLI to $BIN_DIR..."
  work_dir="$(mktemp -d)"
  trap 'rm -rf "$work_dir"' EXIT
  installer_path="${work_dir}/install.sh"

  # Filter the known upstream cleanup trap noise from the installer
  if ! curl -fL \
    --retry "$CURL_RETRIES" \
    --retry-all-errors \
    --connect-timeout "$CURL_CONNECT_TIMEOUT" \
    --max-time "$CURL_MAX_TIME" \
    -o "$installer_path" "$INSTALL_URL"; then
    echo "Failed to download CLI installer"
    prebuilt_status=1
  elif ! bash "$installer_path" -b "$BIN_DIR" 2> >(sed '/^bash: line 1: tmp: unbound variable$/d' >&2); then
    prebuilt_status=1
  else
    prebuilt_status=0
  fi

  if [ "$prebuilt_status" -ne 0 ]; then
    if command -v nautilus > /dev/null 2>&1; then
      echo "Installer exit ignored after successful install (known upstream cleanup trap bug)"
    else
      echo "Prebuilt install failed; building CLI from source..."
      cargo_install_cli
    fi
  fi
fi

nautilus --version
