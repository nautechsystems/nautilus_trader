#!/usr/bin/env bash
# Enforces PyO3 conventions:
# - Functions with #[pyo3(name = "...")] must have Rust names prefixed with py_
# - Adapter pyclasses must use specific adapter module names
# - Standard Python exceptions must use error helper functions

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping PyO3 convention checks"
  exit 0
fi

# Color output
RED='\033[0;31m'
NC='\033[0m' # No Color

# Track if we found violations
VIOLATIONS=0

echo "Checking PyO3 naming conventions..."

# Use ripgrep multiline mode to find violations in one pass
# Pattern: #[pyo3(name = ...)] followed by fn that doesn't start with py_
# This is much faster than bash loop processing
while IFS=: read -r file line_num match; do
  [[ -z "$file" ]] && continue

  # Extract function name from the match
  if [[ "$match" =~ fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*) ]]; then
    fn_name="${BASH_REMATCH[1]}"

    # Skip if already has py_ prefix
    [[ "$fn_name" =~ ^py_ ]] && continue

    echo -e "${RED}Error:${NC} PyO3 function missing py_ prefix in $file:$line_num"
    echo "  Function: fn $fn_name()"
    echo "  Expected: fn py_$fn_name()"
    echo
    VIOLATIONS=$((VIOLATIONS + 1))
  fi
done < <(rg -U -n '#\[pyo3\(name = "[^"]+"\)\](\s*\n|\s*#[^\n]*\n)*\s*(pub\s+)?(async\s+)?fn\s+[a-zA-Z_][a-zA-Z0-9_]*' crates --type rust 2> /dev/null || true)

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS PyO3 naming convention violation(s)${NC}"
  echo
  echo "Convention:"
  echo "  - Rust functions exposed via PyO3 must be prefixed with py_"
  echo "  - Use #[pyo3(name = \"name\")] to expose without the py_ prefix in Python"
  exit 1
fi

echo "✓ All PyO3 naming conventions are valid"

# Check adapter module naming: module paths must not nest under "adapters"
echo "Checking adapter module paths..."
ADAPTER_VIOLATIONS=0

while IFS=: read -r file line_num match; do
  [[ -z "$file" ]] && continue

  echo -e "${RED}Error:${NC} Module path nests under 'adapters' in $file:$line_num"
  echo "  Found: $(echo "$match" | xargs)"
  echo "  Use: nautilus_trader.<adapter_name> (not nautilus_trader.adapters.<adapter_name>)"
  echo
  ADAPTER_VIOLATIONS=$((ADAPTER_VIOLATIONS + 1))
done < <(rg -n 'module\s*=\s*"nautilus_trader\.adapters\.' crates/adapters --type rust 2> /dev/null || true)

if [ $ADAPTER_VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $ADAPTER_VIOLATIONS adapter module path violation(s)${NC}"
  echo
  echo "Convention:"
  echo "  - Adapter module paths are flat: nautilus_trader.<adapter_name>"
  echo "  - The 'adapters' directory is organizational only, not part of the module path"
  exit 1
fi

echo "✓ All adapter module paths are valid"

# Check for raw PyErr construction that should use error helpers
echo "Checking PyO3 error helper usage..."
RAW_ERR_VIOLATIONS=0

while IFS=: read -r file line_num match; do
  [[ -z "$file" ]] && continue

  # Skip the helper definitions themselves
  [[ "$file" == "crates/core/src/python/mod.rs" ]] && continue

  # Skip test assertions (e.g., is_instance_of::<pyo3::exceptions::PyRuntimeError>)
  [[ "$match" =~ is_instance_of ]] && continue

  # Determine which helper to suggest
  if [[ "$match" =~ PyValueError ]]; then
    suggestion="to_pyvalue_err"
  elif [[ "$match" =~ PyTypeError ]]; then
    suggestion="to_pytype_err"
  elif [[ "$match" =~ PyRuntimeError ]]; then
    suggestion="to_pyruntime_err"
  elif [[ "$match" =~ PyKeyError ]]; then
    suggestion="to_pykey_err"
  elif [[ "$match" =~ PyNotImplementedError ]]; then
    suggestion="to_pynotimplemented_err"
  elif [[ "$match" =~ PyException ]]; then
    suggestion="to_pyexception"
  else
    continue
  fi

  echo -e "${RED}Error:${NC} Raw PyO3 exception construction in $file:$line_num"
  echo "  Found: $(echo "$match" | xargs)"
  echo "  Use: $suggestion(...) instead"
  echo
  RAW_ERR_VIOLATIONS=$((RAW_ERR_VIOLATIONS + 1))
done < <(rg -n 'PyErr::new::<(pyo3::exceptions::)?Py(Value|Type|Runtime|Key|NotImplemented)Error|Py(Value|Type|Runtime|Key|NotImplemented)Error::new_err|PyErr::new::<(pyo3::exceptions::)?PyException|PyException::new_err' crates --type rust 2> /dev/null || true)

if [ $RAW_ERR_VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $RAW_ERR_VIOLATIONS raw PyO3 exception construction(s)${NC}"
  echo
  echo "Convention:"
  echo "  - Use to_pyvalue_err(...) instead of PyValueError::new_err(...)"
  echo "  - Use to_pytype_err(...) instead of PyTypeError::new_err(...)"
  echo "  - Use to_pyruntime_err(...) instead of PyRuntimeError::new_err(...)"
  echo "  - Use to_pykey_err(...) instead of PyKeyError::new_err(...)"
  echo "  - Use to_pyexception(...) instead of PyException::new_err(...)"
  echo "  - Use to_pynotimplemented_err(...) instead of PyNotImplementedError::new_err(...)"
  echo "  - Helpers are in nautilus_core::python"
  exit 1
fi

echo "✓ All PyO3 error constructions use helpers"
exit 0
