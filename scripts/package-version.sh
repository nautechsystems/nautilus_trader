#!/usr/bin/env bash
#
# get_version.sh
#
# Extracts "version" from pyproject.toml.

PYPROJECT_FILE="pyproject.toml"

# Try to parse using Python's `tomllib` (Python 3.11+)
if python -c "import tomllib" &> /dev/null; then
  python -c "
import tomllib
with open('$PYPROJECT_FILE', 'rb') as f:
    data = tomllib.load(f)
print(data['project']['version'])
" | tr -d '\n\r '
else
  # Fallback: grep & sed one-liner
  grep -E '^version\s*=' "$PYPROJECT_FILE" |
    sed -E 's/^version\s*=\s*\"([^\"]*)\".*/\1/' |
    tr -d '\n\r '
fi
