#!/usr/bin/env bash
# Enforces logging conventions:
# 1. Logging macros (trace, debug, info, warn, error) must be fully qualified
#    Use log::debug!(...) or tracing::info!(...) instead of importing the macros
#    Other imports from log/tracing crates are allowed (Level, LevelFilter, etc.)
#    Handles both single-line and multi-line use statements (e.g., rustfmt-wrapped imports)
#    Handles all visibility modifiers: pub, pub(crate), pub(super), pub(self), pub(in path)
# 2. Log messages must not end with a terminating period
#    Use '// log-period-ok' comment on or within 3 lines above to allow exceptions

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping logging macro usage checks"
  exit 0
fi

# Color output
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Track if we found violations
VIOLATIONS=0

# Pattern to find files with potential violations
# Uses word boundaries to match forbidden macros in use statements
FILE_PATTERN='^\s*(pub(\([^)]*\))?\s+)?use\s+(log|tracing)::[^;]*\b(trace|debug|info|warn|error)\b'

# Find files containing potential violations
candidate_files=$(rg -l "$FILE_PATTERN" crates --type rust 2> /dev/null || true)

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
  echo
else
  echo "✓ All logging macro usage is fully qualified"
fi

# Check for terminating periods in log messages
echo "Checking for terminating periods in log messages..."

PERIOD_VIOLATIONS=0

# Match log/tracing macro calls where format string ends with a terminating period
# Excludes ellipsis (...") which indicates an ongoing action
# Checks both single-line and multi-line macro calls

# Single-line: log::info!("Message.")
period_output=$(rg -n --no-heading \
  '(log|tracing)::(trace|debug|info|warn|error)!\(.*[^.]\."' \
  crates --type rust 2> /dev/null || true)

# Multi-line: log::info!(\n    "Message.") and structured: log::info!(\n    field = val;\n    "Message.")
# Allows zero or more field lines (ending with ; or ,) before the format string
# Filter output to only lines containing the terminating period
period_output_multi=$(rg -n --no-heading -U \
  '(log|tracing)::(trace|debug|info|warn|error)!\(\s*\n(\s*[^\n]*[;,]\s*\n)*\s*"[^"]*[^.]\."' \
  crates --type rust 2> /dev/null | grep '[^.]\."' || true)

# Combine results
combined_output="${period_output}"
if [[ -n "$period_output_multi" ]]; then
  combined_output+=$'\n'"${period_output_multi}"
fi

# Deduplicate by file:line
seen_keys=""

if [[ -n "$combined_output" ]]; then
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue

    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"

      # Deduplicate
      key="$file:$line_num"
      case "$seen_keys" in
        *"|$key|"*) continue ;;
      esac
      seen_keys+="|$key|"

      # Check for allow marker on matched line or within 3 lines above
      context_start=$((line_num > 3 ? line_num - 3 : 1))
      context=$(sed -n "${context_start},${line_num}p" "$file" 2> /dev/null || true)
      if [[ "$context" =~ log-period-ok ]]; then
        continue
      fi

      trimmed="${line_content#"${line_content%%[![:space:]]*}"}"
      echo -e "${RED}Error:${NC} Log message with terminating period in $file:$line_num"
      echo "  ${trimmed:0:100}"
      PERIOD_VIOLATIONS=$((PERIOD_VIOLATIONS + 1))
    fi
  done <<< "$combined_output"

  if [ $PERIOD_VIOLATIONS -gt 0 ]; then
    echo
    echo -e "${RED}Found $PERIOD_VIOLATIONS log message period violation(s)${NC}"
    echo
    echo -e "${YELLOW}To fix:${NC} Remove the terminating period from log messages"
    echo "  log::info!(\"Starting server.\") -> log::info!(\"Starting server\")"
    echo
    echo "Add '// log-period-ok' comment to allow specific exceptions"
  fi
else
  echo "✓ No terminating periods in log messages"
fi

# Exit with error if any violations found
if [ $VIOLATIONS -gt 0 ] || [ $PERIOD_VIOLATIONS -gt 0 ]; then
  exit 1
fi

echo
echo "✓ All logging conventions are valid"
exit 0
