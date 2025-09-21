#!/usr/bin/env bash
set -euo pipefail

# Install Nautilus CLI from prebuilt tarball with retries.
# Falls back to building from source if needed.

BIN_DIR="${BIN_DIR:-"$HOME/.local/bin"}"
export PATH="$BIN_DIR:$PATH"

INSTALL_URL="https://packages.nautechsystems.io/cli/nautilus-cli/install.sh"

echo "Installing Nautilus CLI to $BIN_DIR..."
if ! curl -fL --connect-timeout 10 --retry 5 --retry-delay 2 --retry-max-time 60 --retry-all-errors "$INSTALL_URL" | bash -s -- -b "$BIN_DIR"; then
  if command -v nautilus >/dev/null 2>&1; then
    echo "Installer exit ignored (known cleanup trap bug)"
  else
    echo "Prebuilt install failed; building CLI from source..."
    cargo install -q --path crates/cli --bin nautilus --locked --force --root "$HOME/.local"
  fi
fi

nautilus --version
