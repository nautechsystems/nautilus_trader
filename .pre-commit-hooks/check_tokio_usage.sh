#!/usr/bin/env bash
# Enforces tokio usage conventions:
# 1. Sync core crates must not have tokio as a regular dependency (dev-deps OK)
# 2. Common crate must have tokio as optional only
# 3. tokio::time::*, tokio::spawn, tokio::sync::* should be fully qualified
#
# Use '// tokio-import-ok' comment to allow specific exceptions

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping tokio usage checks"
  exit 0
fi

# Color output
RED='\033[0;31m'
NC='\033[0m' # No Color

VIOLATIONS=0

# Marker comment to allow exceptions
ALLOW_MARKER="tokio-import-ok"

# Sync core crates that must not depend on tokio (alphabetical order)
SYNC_CRATES=(
  "analysis"
  "backtest"
  "core"
  "cryptography"
  "data"
  "execution"
  "indicators"
  "model"
  "portfolio"
  "risk"
  "serialization"
  "system"
  "trading"
)

################################################################################
# Part 1: Check tokio is not a regular dependency for sync core crates
################################################################################

echo "Checking tokio dependencies in sync core crates..."

for crate in "${SYNC_CRATES[@]}"; do
  manifest="crates/$crate/Cargo.toml"

  [[ ! -f "$manifest" ]] && continue

  # Check if tokio appears as a regular dependency (not in [dev-dependencies])
  # Use awk to only check lines before [dev-dependencies] or [build-dependencies]
  if awk '
    /^\[dev-dependencies\]/ { exit }
    /^\[build-dependencies\]/ { exit }
    /^tokio[[:space:]]*=/ { found=1; exit }
    END { exit !found }
  ' "$manifest" 2> /dev/null; then
    echo -e "${RED}Error:${NC} Sync core crate '$crate' has tokio as a regular dependency"
    echo "  File: $manifest"
    echo "  tokio should only be a dev-dependency for sync core crates"
    echo
    VIOLATIONS=$((VIOLATIONS + 1))
  fi
done

################################################################################
# Part 2: Check common crate has tokio as optional
################################################################################

echo "Checking common crate tokio dependency..."

COMMON_MANIFEST="crates/common/Cargo.toml"
if [[ -f "$COMMON_MANIFEST" ]]; then
  # Extract the tokio line and check it contains "optional = true"
  tokio_line=$(rg "^tokio[[:space:]]*=" "$COMMON_MANIFEST" 2> /dev/null || true)
  if [[ -n "$tokio_line" ]] && [[ ! "$tokio_line" =~ optional[[:space:]]*=[[:space:]]*true ]]; then
    echo -e "${RED}Error:${NC} Common crate must have tokio as an optional dependency"
    echo "  File: $COMMON_MANIFEST"
    echo "  tokio should be: tokio = { workspace = true, optional = true }"
    echo
    VIOLATIONS=$((VIOLATIONS + 1))
  fi
fi

################################################################################
# Part 3: Enforce fully qualified tokio usage (no imports of time/spawn/sync)
################################################################################

echo "Checking tokio import conventions..."

# Check for use tokio::time::* imports
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  # Skip if marked as allowed
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"

  # Check if it's Duration specifically (should use std::time::Duration)
  if [[ "$line_content" =~ Duration ]]; then
    echo -e "${RED}Error:${NC} Use std::time::Duration instead of tokio::time::Duration in $file:$line_num"
    echo "  Found: $trimmed"
    echo "  tokio::time::Duration is just a re-export of std::time::Duration"
  else
    echo -e "${RED}Error:${NC} tokio::time should be fully qualified in $file:$line_num"
    echo "  Found: $trimmed"
    echo "  Use tokio::time::sleep, tokio::time::timeout, etc. inline instead of importing"
  fi
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::time::" crates --type rust 2> /dev/null || true)

# Check for use tokio::spawn imports
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  # Skip if marked as allowed
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} tokio::spawn should be fully qualified in $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Use tokio::spawn(...) inline instead of importing"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::spawn" crates --type rust 2> /dev/null || true)

# Check for use tokio::try_join imports
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  # Skip if marked as allowed
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} tokio::try_join should be fully qualified in $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Use tokio::try_join!(...) inline instead of importing"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::try_join" crates --type rust 2> /dev/null || true)

# Check for use tokio::sync::* imports (Mutex, RwLock, mpsc, etc.)
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  # Skip if marked as allowed
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} tokio::sync should be fully qualified in $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Use tokio::sync::Mutex, tokio::sync::RwLock, etc. inline instead of importing"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "^[[:space:]]*use tokio::sync::" crates --type rust 2> /dev/null || true)

################################################################################
# Part 4: Enforce get_runtime().spawn() in adapter crates (not tokio::spawn)
################################################################################

echo "Checking adapter crates use get_runtime().spawn()..."

# Find tokio::spawn in adapter source files (excluding tests)
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  # Skip test files and directories
  [[ "$file" =~ /tests/ ]] && continue

  # Skip if marked as allowed
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  # Skip if line is inside a test module (after #[cfg(test)])
  # Find the line number where #[cfg(test)] appears
  test_module_line=$(rg -n "^#\[cfg\(test\)\]" "$file" 2> /dev/null | head -1 | cut -d: -f1)
  if [[ -n "$test_module_line" ]] && [[ "$line_num" -gt "$test_module_line" ]]; then
    continue
  fi

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} Use get_runtime().spawn() instead of tokio::spawn in adapters: $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Adapters must use get_runtime().spawn() for Python FFI compatibility"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "tokio::spawn\(" crates/adapters --type rust 2> /dev/null || true)

################################################################################
# Part 5: Enforce shorter import path for get_runtime
################################################################################

echo "Checking get_runtime import paths..."

# Check for longer path: live::runtime::get_runtime (should be live::get_runtime)
while IFS=: read -r file line_num line_content; do
  [[ -z "$file" ]] && continue

  # Skip if marked as allowed
  [[ "$line_content" =~ $ALLOW_MARKER ]] && continue

  trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
  echo -e "${RED}Error:${NC} Use shorter import path for get_runtime in $file:$line_num"
  echo "  Found: $trimmed"
  echo "  Use: nautilus_common::live::get_runtime (not live::runtime::get_runtime)"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n "live::runtime::get_runtime" crates --type rust 2> /dev/null || true)

################################################################################
# Summary
################################################################################

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS tokio usage violation(s)${NC}"
  echo
  echo "Add '// tokio-import-ok' comment to allow specific exceptions"
  exit 1
fi

echo "✓ All tokio usage is valid"
exit 0
