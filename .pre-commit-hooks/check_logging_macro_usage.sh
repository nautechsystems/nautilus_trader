#!/usr/bin/env bash
# Enforces logging macro usage conventions:
# - Logging macros (trace, debug, info, warn, error) must be fully qualified
# - Use log::debug!(...) or tracing::info!(...) instead of importing the macros
# - Other imports from log/tracing crates are allowed (Level, LevelFilter, etc.)
#
# Handles both single-line and multi-line use statements (e.g., rustfmt-wrapped imports)
# Handles all visibility modifiers: pub, pub(crate), pub(super), pub(self), pub(in path)

set -euo pipefail

# Color output
RED='\033[0;31m'
NC='\033[0m' # No Color

# Track if we found violations
VIOLATIONS=0

# Pattern to find files with potential violations
# Uses word boundaries to match forbidden macros in use statements
FILE_PATTERN='^\s*(pub(\([^)]*\))?\s+)?use\s+(log|tracing)::[^;]*\b(trace|debug|info|warn|error)\b'

# Find files containing potential violations
if command -v rg &> /dev/null; then
  # Use ripgrep to find files (fast)
  candidate_files=$(rg -l "$FILE_PATTERN" crates --type rust 2> /dev/null || true)
else
  # Fallback to grep + find (null-terminated for safe filename handling)
  # Note: -r flag omitted for BSD/macOS compatibility (GNU extension)
  candidate_files=$(find crates -name '*.rs' -type f -print0 2> /dev/null |
    xargs -0 grep -lZ '\btrace\b\|\bdebug\b\|\binfo\b\|\bwarn\b\|\berror\b' 2> /dev/null |
    xargs -0 grep -l '^\s*\(pub\)\?.*use\s\+\(log\|tracing\)::' 2> /dev/null || true)
fi

# Process each candidate file to check for actual violations
while IFS= read -r file; do
  [[ -z "$file" ]] && continue

  # Skip style guide docs
  if [[ "$file" == *"logging_style_guide"* ]] || [[ "$file" == *"LOGGING"* ]]; then
    continue
  fi

  # Read file and accumulate multi-line use statements
  line_num=0
  in_use_statement=false
  use_statement=""
  use_start_line=0

  while IFS= read -r line; do
    line_num=$((line_num + 1))

    # Detect start of use log:: or use tracing:: statement
    if echo "$line" | grep -qE '^\s*(pub(\([^)]*\))?\s+)?use\s+(log|tracing)::'; then
      in_use_statement=true
      use_statement="$line"
      use_start_line=$line_num
    elif [ "$in_use_statement" = true ]; then
      # Continue accumulating multiline statement
      use_statement="$use_statement $line"
    fi

    # Check if statement is complete (ends with semicolon)
    if [ "$in_use_statement" = true ] && echo "$use_statement" | grep -qE ';\s*$'; then
      # Normalize: remove comments and extra whitespace
      normalized=$(echo "$use_statement" | sed -e 's|//.*||g' -e 's/[[:space:]]\+/ /g' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')

      # Check if normalized statement contains a forbidden macro as a word
      if echo "$normalized" | grep -qE '\b(trace|debug|info|warn|error)\b'; then
        echo -e "${RED}Error:${NC} Invalid logging macro import in $file:$use_start_line"
        echo "  Found: $normalized"
        echo "  Logging macros (trace, debug, info, warn, error) must be fully qualified."
        echo "  Use log::debug!(...) or tracing::info!(...) instead of importing the macros."
        echo
        VIOLATIONS=$((VIOLATIONS + 1))
      fi

      # Reset for next use statement
      in_use_statement=false
      use_statement=""
      use_start_line=0
    fi
  done < "$file"
done <<< "$candidate_files"

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS logging macro import violation(s)${NC}"
  exit 1
fi

echo "✓ All logging macro usage is fully qualified"
exit 0
