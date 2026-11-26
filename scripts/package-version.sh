#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 || ($# -eq 1 && "$1" != --check) ]]; then
  printf 'Usage: %s [--check]\n' "${0##*/}" >&2
  exit 2
fi

# Resolve pyproject.toml relative to this script's location
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYPROJECT_FILE="${SCRIPT_DIR}/../python/pyproject.toml"
CORE_BUILD_FILE="${SCRIPT_DIR}/../crates/core/build.rs"

# Check that pyproject.toml exists
if [[ ! -f "$PYPROJECT_FILE" ]]; then
  echo "Error: pyproject.toml not found at $PYPROJECT_FILE" >&2
  exit 1
fi

# Honor PYTHON before probing interpreter names
run_python() {
  if [[ -n "${PYTHON:-}" ]] && command -v "$PYTHON" &> /dev/null; then
    "$PYTHON" "$@"
  elif command -v python3 &> /dev/null; then
    python3 "$@"
  elif command -v python &> /dev/null; then
    python "$@"
  elif command -v py &> /dev/null; then
    py -3 "$@"
  else
    return 1
  fi
}

# Try to parse using Python's `tomllib` (Python 3.11+) if available
if run_python -c 'import tomllib' &> /dev/null; then
  VERSION=$(run_python -c '
import sys
import tomllib
with open(sys.argv[1], "rb") as f:
    data = tomllib.load(f)
print(data["project"]["version"])
' "$PYPROJECT_FILE" | tr -d '\n\r ')
else
  # Fallback: grep & sed one-liner (works without Python)
  VERSION=$(grep -E '^version[[:space:]]*=' "$PYPROJECT_FILE" |
    sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]*)".*/\1/' |
    tr -d '\n\r ')
fi

# Validate that we got a version
if [[ -z "$VERSION" ]]; then
  echo "Error: Could not extract version from $PYPROJECT_FILE" >&2
  exit 1
fi

if [[ "${1:-}" == --check ]]; then
  CORE_VERSION=$(awk -F'"' '/^const NAUTILUS_VERSION: &str = / {print $2}' "$CORE_BUILD_FILE")
  SUFFIX=${VERSION#"$CORE_VERSION"}
  if [[ "$SUFFIX" == "$VERSION" || ! "$SUFFIX" =~ ^($|[a-z+-]|\.[a-z]) ]]; then
    printf 'Error: Python package version %s does not match Rust version %s\n' "$VERSION" "$CORE_VERSION" >&2
    exit 1
  fi
fi

printf '%s' "$VERSION"
