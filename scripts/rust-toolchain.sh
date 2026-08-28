#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLCHAIN_FILE="${SCRIPT_DIR}/../rust-toolchain.toml"

if [[ ! -f "$TOOLCHAIN_FILE" ]]; then
  echo "Error: rust-toolchain.toml not found at $TOOLCHAIN_FILE" >&2
  exit 1
fi

VERSION=$(awk -F'"' '
  /^\[toolchain\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^[[:space:]]*channel[[:space:]]*=/ { print $2; exit }
' "$TOOLCHAIN_FILE")

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: [toolchain].channel must be an exact Rust version in $TOOLCHAIN_FILE, was '$VERSION'" >&2
  exit 1
fi

echo -n "$VERSION"
