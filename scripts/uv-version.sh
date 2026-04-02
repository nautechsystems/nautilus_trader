#!/usr/bin/env bash
set -euo pipefail

# Extract the uv version from [tool.uv] required-version in pyproject.toml
#
# Usage: uv-version.sh
# Example output: 0.11.2

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYPROJECT="${SCRIPT_DIR}/../pyproject.toml"

if [[ ! -f "$PYPROJECT" ]]; then
  echo "Error: pyproject.toml not found at $PYPROJECT" >&2
  exit 1
fi

# Extract version from: required-version = "==0.11.2"
# Strips the == prefix to return the bare version
VERSION=$(awk '
  /^\[tool\.uv\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^required-version/ { gsub(/[" =]/, "", $3); print $3; exit }
' "$PYPROJECT")

if [[ -z "$VERSION" ]]; then
  echo "Error: Could not find required-version in [tool.uv]" >&2
  exit 1
fi

echo -n "$VERSION"
