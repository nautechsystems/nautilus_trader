#!/usr/bin/env bash

# Enforces blank line above `if` statements in Rust code.
#
# An `if` must have a blank line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the `if` condition appears on the line directly above
# c) It is part of an `else if` chain
# d) The line above is a comment or attribute (attached to the `if`)
# e) The `if` is an expression or continuation (assignment, argument, match guard)

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping if-spacing checks"
  exit 0
fi

# Color output
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

VIOLATIONS=0

echo "Checking for blank line above \`if\` statements..."

# Find all `if` lines with 1 line of preceding context
# rg exits 0 (matches), 1 (no matches), or 2+ (error)
rg_exit=0
output=$(rg -n -B 1 --no-heading '^\s*if\s' crates tests examples docs --type rust 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

if [[ -z "$output" ]]; then
  echo "✓ Blank line above \`if\` conventions are valid"
  exit 0
fi

has_prev=false
prev_content=""

while IFS= read -r line; do

  # Separator between match groups
  if [[ "$line" == "--" ]]; then
    has_prev=false
    prev_content=""
    continue
  fi

  # Context line (line before match)
  if [[ "$line" =~ ^([^:]+)-([0-9]+)-(.*)$ ]]; then
    prev_content="${BASH_REMATCH[3]}"
    has_prev=true
    continue
  fi

  # Match line
  if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
    file="${BASH_REMATCH[1]}"
    line_num="${BASH_REMATCH[2]}"
    content="${BASH_REMATCH[3]}"
    trimmed="${content#"${content%%[![:space:]]*}"}"

    # No preceding line (first line of file or first in group)
    if ! $has_prev; then
      prev_content="$content"
      has_prev=true
      continue
    fi

    prev_trimmed="${prev_content#"${prev_content%%[![:space:]]*}"}"

    # Exempt: else if chain
    if [[ "$prev_trimmed" =~ \}[[:space:]]*else[[:space:]]*$ ]] ||
      [[ "$prev_trimmed" =~ ^else[[:space:]]*$ ]]; then
      prev_content="$content"
      continue
    fi

    # Exempt: blank line above
    if [[ -z "$prev_trimmed" ]]; then
      prev_content="$content"
      continue
    fi

    # Exempt: first expression in block (prev line opens a block)
    if [[ "$prev_trimmed" == "{" ]] || [[ "$prev_trimmed" =~ \{[[:space:]]*$ ]]; then
      prev_content="$content"
      continue
    fi

    # Exempt: comment or attribute above (attached to the `if`)
    if [[ "$prev_trimmed" =~ ^// ]] || [[ "$prev_trimmed" =~ ^\*[[:space:]] ]] ||
      [[ "$prev_trimmed" =~ ^\*/ ]] || [[ "$prev_trimmed" =~ ^/\* ]] ||
      [[ "$prev_trimmed" =~ ^#\[ ]]; then
      prev_content="$content"
      continue
    fi

    # Exempt: `if` used as expression (assignment, argument, or continuation)
    if [[ "$prev_trimmed" =~ =[[:space:]]*$ ]] &&
      ! [[ "$prev_trimmed" =~ [=!\<\>]=[[:space:]]*$ ]]; then
      prev_content="$content"
      continue
    fi
    if [[ "$prev_trimmed" =~ [,\(\)|][[:space:]]*$ ]] ||
      [[ "$prev_trimmed" =~ ^[|] ]]; then
      prev_content="$content"
      continue
    fi

    # Exempt: shared identifier with line above (pure bash, no subprocesses)
    condition="${trimmed#if }"
    has_shared=false
    rest="$condition"
    while [[ "$rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
      ident="${BASH_REMATCH[1]}"
      rest="${BASH_REMATCH[2]}"
      case "$ident" in
        if | else | let | mut | ref | true | false | return | break | continue | match) ;;
        as | in | for | while | loop | fn | struct | enum | impl | trait | pub) ;;
        use | mod | const | static | type | where | async | await | move | unsafe) ;;
        extern | crate | super | dyn | self | Self) ;;
        *)
          if [[ "$prev_content" =~ (^|[^a-zA-Z0-9_])${ident}([^a-zA-Z0-9_]|$) ]]; then
            has_shared=true
            break
          fi
          ;;
      esac
    done

    if ! $has_shared; then
      echo -e "${RED}Error:${NC} Missing blank line above \`if\` in $file:$line_num"
      echo "  ${trimmed:0:100}"
      echo "  Line above: ${prev_trimmed:0:100}"
      echo
      VIOLATIONS=$((VIOLATIONS + 1))
    fi

    prev_content="$content"
  fi
done <<< "$output"

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS blank-line-above-if violation(s)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC} Add a blank line above the \`if\` statement"
  echo "Exceptions: first expression in a block, or line above shares an identifier with the condition"
  exit 1
fi

echo "✓ Blank line above \`if\` conventions are valid"
exit 0
