#!/bin/bash
set -euo pipefail

# Detect available Python interpreter (honor PYTHON env var, then probe common names)
detect_python() {
  if [[ -n "${PYTHON:-}" ]] && command -v "$PYTHON" &> /dev/null; then
    echo "$PYTHON"
  elif command -v python3 &> /dev/null; then
    echo "python3"
  elif command -v python &> /dev/null; then
    echo "python"
  elif command -v py &> /dev/null; then
    # Windows py launcher
    echo "py -3"
  else
    return 1
  fi
}

# Find Python interpreter
PYTHON_CMD=$(detect_python) || {
  echo "Error: No Python interpreter found (tried: \$PYTHON, python3, python, py)" >&2
  exit 1
}

# Get Python version
VERSION=$($PYTHON_CMD --version 2>&1 | cut -d' ' -f2 | tr -d '\n\r ')

# Validate that we got a version
if [[ -z "$VERSION" ]]; then
  echo "Error: Could not extract Python version from '$PYTHON_CMD'" >&2
  exit 1
fi

# Output version (without trailing newline for consistency)
echo -n "$VERSION"
