#!/bin/bash
set -euo pipefail

# Resolve pyproject.toml relative to this script's location
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYPROJECT="${SCRIPT_DIR}/../pyproject.toml"

# Check that pyproject.toml exists
if [[ ! -f "$PYPROJECT" ]]; then
  echo "Error: pyproject.toml not found at $PYPROJECT" >&2
  exit 1
fi

# Extract pre-commit version, handling optional whitespace around >=
VERSION=$(awk -F'>=' '/pre-commit[[:space:]]*>=/ {split($2, a, ","); gsub(/[[:space:]"]/,"",a[1]); print a[1]; exit}' "$PYPROJECT")

# Validate that we got a version
if [[ -z "$VERSION" ]]; then
  echo "Error: Could not extract pre-commit version from $PYPROJECT" >&2
  exit 1
fi

# Output version (without trailing newline for consistency with other version scripts)
echo -n "$VERSION"
