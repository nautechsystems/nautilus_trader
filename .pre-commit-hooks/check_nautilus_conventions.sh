#!/usr/bin/env bash
# Enforces Nautilus conventions:
# 1. Nautilus domain types should not be fully qualified in code
#    (identifiers, data types, enums, etc. should be imported and used directly)
#
# Use '// nautilus-import-ok' comment to allow specific exceptions

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping Nautilus convention checks"
  exit 0
fi

# Color output
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

ALLOW_MARKER="nautilus-import-ok"

echo "Checking for fully qualified Nautilus types..."

# Use rg with context (-B 5) to get surrounding lines in one pass
# Format: file:line:content (with -- separators for context)
output=$(rg -n -B 5 \
  --no-heading \
  'nautilus_[a-z_]+(?:::[a-z_]+)+::[A-Z][A-Za-z0-9_]+' \
  crates tests examples \
  --type rust \
  2> /dev/null || true)

[[ -z "$output" ]] && {
  echo "✓ All Nautilus conventions are valid"
  exit 0
}

VIOLATIONS=0
seen_violations="" # Track unique violations (POSIX compatible, no associative arrays)

# Process output - context lines have "-" separator, matches have ":" separator
current_context=""
while IFS= read -r line; do
  # Context lines (before match) use "-" as separator after line number
  if [[ "$line" =~ ^([^:]+)-([0-9]+)- ]]; then
    current_context+="$line"$'\n'
    continue
  fi

  # Match lines use ":" as separator
  if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
    file="${BASH_REMATCH[1]}"
    line_num="${BASH_REMATCH[2]}"
    line_content="${BASH_REMATCH[3]}"

    # Skip if already reported this location (POSIX compatible check)
    key="$file:$line_num"
    case "$seen_violations" in
      *"|$key|"*)
        current_context=""
        continue
        ;;
    esac

    # Check if allow marker in context or current line
    if [[ "$current_context" =~ $ALLOW_MARKER ]] || [[ "$line_content" =~ $ALLOW_MARKER ]]; then
      current_context=""
      continue
    fi

    # Trim leading whitespace for pattern checks
    trimmed="${line_content#"${line_content%%[![:space:]]*}"}"

    # Skip use statements (including pub(crate)/pub(super)/pub(in ...) variants)
    # Skip comments, mod declarations, extern crate
    # Skip if inside a multi-line use { ... } block (check context for "use {")
    pub_vis_use='^pub[[:space:]]*[(][^)]*[)][[:space:]]*use[[:space:]]'
    pub_vis_extern='^pub[[:space:]]*[(][^)]*[)][[:space:]]*extern[[:space:]]'
    if [[ "$trimmed" =~ ^use[[:space:]] ]] ||
      [[ "$trimmed" =~ $pub_vis_use ]] ||
      [[ "$trimmed" =~ ^pub[[:space:]]+use[[:space:]] ]] ||
      [[ "$trimmed" =~ $pub_vis_extern ]] ||
      [[ "$trimmed" =~ ^pub[[:space:]]+extern[[:space:]] ]] ||
      [[ "$trimmed" =~ ^extern[[:space:]] ]] ||
      [[ "$trimmed" =~ ^// ]] ||
      [[ "$trimmed" =~ ^\*|^/\* ]] ||
      [[ "$trimmed" =~ ^mod[[:space:]] ]] ||
      [[ "$current_context" =~ use[[:space:]]*[\{] ]]; then
      current_context=""
      continue
    fi

    # Extract the matched pattern for error message
    if [[ "$line_content" =~ (nautilus_[a-z_]+(::[a-z_]+)+::[A-Z][A-Za-z0-9_]+) ]]; then
      matched="${BASH_REMATCH[1]}"
      echo -e "${RED}Error:${NC} Fully qualified Nautilus type in $file:$line_num"
      echo "  Found: $matched"
      echo "  Import the type and use it directly instead of fully qualifying"
      echo "  Line: ${trimmed:0:100}"
      echo
      seen_violations+="|$key|"
      VIOLATIONS=$((VIOLATIONS + 1))
    fi

    current_context=""
  fi

  # Separator between matches (--) - reset context
  [[ "$line" == "--" ]] && current_context=""
done <<< "$output"

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS Nautilus convention violation(s)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC} Import the type at the top of the file and use it directly"
  echo "Example: use nautilus_model::identifiers::InstrumentId;"
  echo "         Then use: InstrumentId::new(...)"
  echo
  echo "Add '// nautilus-import-ok' comment to allow specific exceptions"
  exit 1
fi

echo "✓ All Nautilus conventions are valid"
exit 0
