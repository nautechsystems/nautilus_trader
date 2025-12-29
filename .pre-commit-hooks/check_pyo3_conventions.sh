#!/usr/bin/env bash
# Enforces PyO3 naming conventions:
# - Functions with #[pyo3(name = "...")] must have Rust names prefixed with py_

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

# Check adapter module naming
echo "Checking adapter pyclass module names..."
ADAPTER_VIOLATIONS=0

# Find pyclass declarations in adapters/ that use generic ".adapters" module instead of specific adapter name
while IFS=: read -r file line_num _; do
  [[ -z "$file" ]] && continue

  # Extract expected adapter name from path (e.g., crates/adapters/okx/... -> okx)
  if [[ "$file" =~ crates/adapters/([^/]+)/ ]]; then
    expected="${BASH_REMATCH[1]}"
    # Convert underscores to match crate naming (e.g., coinbase_intx)
    echo -e "${RED}Error:${NC} pyclass uses generic '.adapters' module in $file:$line_num"
    echo "  Expected module ending: .$expected\""
    echo
    ADAPTER_VIOLATIONS=$((ADAPTER_VIOLATIONS + 1))
  fi
done < <(rg -n 'pyo3::pyclass\(.*module\s*=\s*"[^"]*\.adapters"' crates/adapters --type rust 2> /dev/null || true)

if [ $ADAPTER_VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $ADAPTER_VIOLATIONS adapter module naming violation(s)${NC}"
  echo
  echo "Convention:"
  echo "  - Adapter pyclasses must use the specific adapter module, not '.adapters'"
  echo "  - Example: module = \"nautilus_trader.core.nautilus_pyo3.okx\" (not .adapters)"
  exit 1
fi

echo "✓ All adapter pyclass module names are valid"
exit 0
