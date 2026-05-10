#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping legacy nautilus_pyo3 check"
  exit 0
fi

echo "Checking for legacy nautilus_pyo3 references under python/..."

if matches=$(rg -n "nautilus_pyo3" python --glob '!_fixup.py' 2> /dev/null); then
  echo "Error: found legacy nautilus_pyo3 references under python/"
  echo
  echo "$matches"
  echo
  echo "Use the public Python surface or _libnautilus internals instead."
  exit 1
fi

echo "✓ No legacy nautilus_pyo3 references under python/"
