#!/usr/bin/env bash
# Enforces error variable naming conventions:
# 1. Rust: Always use Err(e) for error variables [not Err(error), Err(err), etc.]
# 2. Python: Always use 'except Exception as e:' [not as ex, as exc, etc.]

set -euo pipefail

# Color output
RED='\033[0;31m'
NC='\033[0m' # No Color

# Track if we found violations
VIOLATIONS=0

################################################################################
# Check Rust files for Err(variable) patterns
################################################################################

# We want to catch patterns like:
# - Err(err) => ...
# - Err(error) => ...
# - if let Err(err) = ...
# But NOT:
# - Err(e) => ...
# - Err(e @ ...) => ...

echo "Checking Rust error variable naming..."

# Create a temporary file to store the search results
rust_results=$(mktemp)
trap 'rm -f "$rust_results"' EXIT

# Search for Err( patterns in Rust files
if command -v rg &> /dev/null; then
  # Use ripgrep - look for Err( in Rust files
  rg -n 'Err\(' crates --type rust 2> /dev/null > "$rust_results" || true
else
  # Fall back to grep + find
  find crates -name '*.rs' -type f -exec grep -Hn 'Err(' {} + 2> /dev/null > "$rust_results" || true
fi

while IFS=: read -r file line_num line_content; do
  # Skip empty lines
  [[ -z "$file" ]] && continue

  # Extract the variable name from patterns like Err(xxx)
  # Only flag the lazy/generic names: Err(err) and Err(error)
  # Allow descriptive names like Err(timeout_error), Err(poisoned), etc.

  # Check if line contains the lazy patterns Err(err) or Err(error)
  if echo "$line_content" | grep -qE 'Err\((err|error)\)'; then
    # Extract what's inside Err()
    var_name=$(echo "$line_content" | grep -oE 'Err\((err|error)\)' | sed 's/Err(\(.*\))/\1/' | head -1)

    # Trim leading whitespace from line for display
    trimmed_line="${line_content#"${line_content%%[![:space:]]*}"}"
    echo -e "${RED}Error:${NC} Invalid Rust error variable name in $file:$line_num"
    echo "  Found: Err($var_name)"
    echo "  Expected: Err(e) or a descriptive name"
    echo "  Line: $trimmed_line"
    echo
    VIOLATIONS=$((VIOLATIONS + 1))
  fi
done < "$rust_results"

################################################################################
# Check Python files for 'except ... as variable:' patterns (all exception types)
################################################################################

echo "Checking Python exception variable naming..."

# Require ripgrep for Python checks (complexity requires multiline mode)
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping Python exception checks"
  exit 0
fi

# Create a temporary file to store the search results
python_results=$(mktemp)
trap 'rm -f "$rust_results" "$python_results"' EXIT

# Search for except blocks with 'as xxx:' where xxx is not 'e'
# Checks single-line except blocks (except ExceptionType as xxx:)
# Also handles tuples on one line: except (Type1, Type2) as xxx:
# NOTE: Does NOT detect multiline tuple syntax (rare in practice):
#   except (
#       ValueError,
#   ) as xxx:
# Pattern matches except blocks where 'except' and 'as xxx:' are on the same line
rg -H -n '^[[:space:]]*except[*]?.*[[:space:]]as[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*[[:space:]]*:' \
  --type py \
  --type-add 'pyx:*.pyx' \
  --type pyx \
  . 2> /dev/null > "$python_results" || true

while IFS= read -r line; do
  # Skip empty lines
  [[ -z "$line" ]] && continue

  # Extract file, line_num, and line_content (preserving colons in content)
  # Format: file:line_num:line_content
  file=$(echo "$line" | cut -d: -f1)
  line_num=$(echo "$line" | cut -d: -f2)
  line_content=$(echo "$line" | cut -d: -f3-)

  # Extract variable name from 'as xxx:' pattern
  var_name=$(echo "$line_content" | grep -oE 'as[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*[[:space:]]*:' | sed 's/as[[:space:]]*\([^:]*\):.*/\1/' | tr -d ' ')

  # Check if it's not 'e'
  if [[ -n "$var_name" ]] && [[ "$var_name" != "e" ]]; then
    trimmed_line="${line_content#"${line_content%%[![:space:]]*}"}"
    echo -e "${RED}Error:${NC} Invalid Python exception variable name in $file:$line_num"
    echo "  Found: as $var_name:"
    echo "  Expected: as e:"
    echo "  Line: $trimmed_line"
    echo
    VIOLATIONS=$((VIOLATIONS + 1))
  fi
done < "$python_results"

################################################################################
# Report results
################################################################################

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS error variable naming violation(s)${NC}"
  echo
  echo "Conventions:"
  echo "  - Rust: Use Err(e) or descriptive names (not Err(err) or Err(error))"
  echo "  - Python: Always use 'as e:' for all exception types (not as exc, as err, etc.)"
  exit 1
fi

echo "✓ All error variable names are valid"
exit 0
