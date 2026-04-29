#!/usr/bin/env bash

# Enforces formatting conventions in Rust code.

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping formatting checks (Rust)"
  exit 0
fi

# Color output
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

VIOLATIONS=0

# ---------------------------------------------------------------------------
# Check: blank line above `if` statements
#
# An `if` must have a blank line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the `if` condition appears on the line directly above
# c) It is part of an `else if` chain
# d) The line above is a comment or attribute (attached to the `if`)
# e) The `if` is an expression or continuation (assignment, argument, match guard)
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`if\` statements (Rust)..."

# rg exits 0 (matches), 1 (no matches), or 2+ (error)
rg_exit=0
output=$(rg -n -B 1 --no-heading '^\s*if\s' crates tests examples docs --type rust 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

if [[ -n "$output" ]]; then

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

      # Exempt: string continuation (prev line ends with `\`)
      if [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
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

      # Exempt: match arm guard (prev line is multi-alternative pattern with `|`)
      if [[ "$prev_trimmed" =~ [[:alnum:]][[:space:]]*\|[[:space:]]*[[:alnum:]] ]] &&
        ! [[ "$prev_trimmed" =~ \|\| ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
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
        # Check first body line for shared identifiers with line above
        body_lines=$(sed -n "$((line_num + 1))p" "$file")
        body_rest="$body_lines"
        while [[ "$body_rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
          ident="${BASH_REMATCH[1]}"
          body_rest="${BASH_REMATCH[2]}"
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
      fi

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

fi

# ---------------------------------------------------------------------------
# Check: blank line above `match` blocks
#
# A `match` must have a blank line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the `match` expression appears on the line above
# c) The line above is a comment or attribute
# d) The `match` is an expression or continuation (assignment, argument)
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`match\` blocks (Rust)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading '^\s*match\s' crates tests examples docs --type rust 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

if [[ -n "$output" ]]; then

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

      # Exempt: comment or attribute above
      if [[ "$prev_trimmed" =~ ^// ]] || [[ "$prev_trimmed" =~ ^\*[[:space:]] ]] ||
        [[ "$prev_trimmed" =~ ^\*/ ]] || [[ "$prev_trimmed" =~ ^/\* ]] ||
        [[ "$prev_trimmed" =~ ^#\[ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: string continuation (prev line ends with `\`)
      if [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: `match` used as expression (prev line ends with `=` but not `==`/`!=`/`<=`/`>=`)
      if [[ "$prev_trimmed" =~ =[[:space:]]*$ ]] &&
        ! [[ "$prev_trimmed" =~ [=!\<\>]=[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: `match` as argument (prev line ends with `(`, `,`, `|`)
      if [[ "$prev_trimmed" =~ [,\(\|][[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
      expression="${trimmed#match }"
      has_shared=false
      rest="$expression"
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
        # Check first body line for shared identifiers with line above
        body_lines=$(sed -n "$((line_num + 1))p" "$file")
        body_rest="$body_lines"
        while [[ "$body_rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
          ident="${BASH_REMATCH[1]}"
          body_rest="${BASH_REMATCH[2]}"
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
      fi

      if ! $has_shared; then
        echo -e "${RED}Error:${NC} Missing blank line above \`match\` in $file:$line_num"
        echo "  ${trimmed:0:100}"
        echo "  Line above: ${prev_trimmed:0:100}"
        echo
        VIOLATIONS=$((VIOLATIONS + 1))
      fi

      prev_content="$content"
    fi
  done <<< "$output"

fi

# ---------------------------------------------------------------------------
# Check: blank line above `for` loops
#
# A `for` must have a blank line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the `for` line appears on the line above
# c) The line above is a comment or attribute
# d) The previous line is a loop label
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`for\` loops (Rust)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading "^\s*('[a-zA-Z_]\w*:\s*)?for\s" crates tests examples docs --type rust 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

if [[ -n "$output" ]]; then

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

      # Exempt: comment or attribute above
      if [[ "$prev_trimmed" =~ ^// ]] || [[ "$prev_trimmed" =~ ^\*[[:space:]] ]] ||
        [[ "$prev_trimmed" =~ ^\*/ ]] || [[ "$prev_trimmed" =~ ^/\* ]] ||
        [[ "$prev_trimmed" =~ ^#\[ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: string continuation (prev line ends with `\`)
      if [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: loop label on previous line
      label_pattern="^'[a-zA-Z_]"
      if [[ "$prev_trimmed" =~ $label_pattern ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
      # Strip optional loop label ('label: for ...)
      keyword_part="$trimmed"
      if [[ "$keyword_part" == \'* ]]; then
        keyword_part="${keyword_part#*: }"
      fi
      for_rest="${keyword_part#for }"
      has_shared=false
      rest="$for_rest"
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
        # Check first body line for shared identifiers with line above
        body_lines=$(sed -n "$((line_num + 1))p" "$file")
        body_rest="$body_lines"
        while [[ "$body_rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
          ident="${BASH_REMATCH[1]}"
          body_rest="${BASH_REMATCH[2]}"
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
      fi

      if ! $has_shared; then
        echo -e "${RED}Error:${NC} Missing blank line above \`for\` in $file:$line_num"
        echo "  ${trimmed:0:100}"
        echo "  Line above: ${prev_trimmed:0:100}"
        echo
        VIOLATIONS=$((VIOLATIONS + 1))
      fi

      prev_content="$content"
    fi
  done <<< "$output"

fi

# ---------------------------------------------------------------------------
# Check: blank line above `while` loops
#
# A `while` must have a blank line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the `while` condition appears on the line above
# c) The line above is a comment or attribute
# d) The previous line is a loop label
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`while\` loops (Rust)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading "^\s*('[a-zA-Z_]\w*:\s*)?while\s" crates tests examples docs --type rust 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

if [[ -n "$output" ]]; then

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

      # Exempt: comment or attribute above
      if [[ "$prev_trimmed" =~ ^// ]] || [[ "$prev_trimmed" =~ ^\*[[:space:]] ]] ||
        [[ "$prev_trimmed" =~ ^\*/ ]] || [[ "$prev_trimmed" =~ ^/\* ]] ||
        [[ "$prev_trimmed" =~ ^#\[ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: string continuation (prev line ends with `\`)
      if [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: loop label on previous line
      label_pattern="^'[a-zA-Z_]"
      if [[ "$prev_trimmed" =~ $label_pattern ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
      # Strip optional loop label ('label: while ...)
      keyword_part="$trimmed"
      if [[ "$keyword_part" == \'* ]]; then
        keyword_part="${keyword_part#*: }"
      fi
      while_rest="${keyword_part#while }"
      has_shared=false
      rest="$while_rest"
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
        # Check first body line for shared identifiers with line above
        body_lines=$(sed -n "$((line_num + 1))p" "$file")
        body_rest="$body_lines"
        while [[ "$body_rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
          ident="${BASH_REMATCH[1]}"
          body_rest="${BASH_REMATCH[2]}"
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
      fi

      if ! $has_shared; then
        echo -e "${RED}Error:${NC} Missing blank line above \`while\` in $file:$line_num"
        echo "  ${trimmed:0:100}"
        echo "  Line above: ${prev_trimmed:0:100}"
        echo
        VIOLATIONS=$((VIOLATIONS + 1))
      fi

      prev_content="$content"
    fi
  done <<< "$output"

fi

# ---------------------------------------------------------------------------
# Check: blank line above `loop` blocks
#
# A `loop` must have a blank line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the `loop` line appears on the line above
# c) The line above is a comment or attribute
# d) The previous line is a loop label
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`loop\` blocks (Rust)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading "^\s*('[a-zA-Z_]\w*:\s*)?loop\s" crates tests examples docs --type rust 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

if [[ -n "$output" ]]; then

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

      # Exempt: comment or attribute above
      if [[ "$prev_trimmed" =~ ^// ]] || [[ "$prev_trimmed" =~ ^\*[[:space:]] ]] ||
        [[ "$prev_trimmed" =~ ^\*/ ]] || [[ "$prev_trimmed" =~ ^/\* ]] ||
        [[ "$prev_trimmed" =~ ^#\[ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: string continuation (prev line ends with `\`)
      if [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: loop label on previous line
      label_pattern="^'[a-zA-Z_]"
      if [[ "$prev_trimmed" =~ $label_pattern ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
      # Strip optional loop label ('label: loop ...)
      keyword_part="$trimmed"
      if [[ "$keyword_part" == \'* ]]; then
        keyword_part="${keyword_part#*: }"
      fi
      loop_rest="${keyword_part#loop }"
      has_shared=false
      rest="$loop_rest"
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
        # Check first body line for shared identifiers with line above
        body_lines=$(sed -n "$((line_num + 1))p" "$file")
        body_rest="$body_lines"
        while [[ "$body_rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
          ident="${BASH_REMATCH[1]}"
          body_rest="${BASH_REMATCH[2]}"
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
      fi

      if ! $has_shared; then
        echo -e "${RED}Error:${NC} Missing blank line above \`loop\` in $file:$line_num"
        echo "  ${trimmed:0:100}"
        echo "  Line above: ${prev_trimmed:0:100}"
        echo
        VIOLATIONS=$((VIOLATIONS + 1))
      fi

      prev_content="$content"
    fi
  done <<< "$output"

fi

# ---------------------------------------------------------------------------
# Check: blank line above `spawn` calls
#
# A `spawn` call must have a blank line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the `spawn` line appears on the line above
# c) The line above is a comment or attribute
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`spawn\` calls (Rust)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading -e '^\s*spawn\(' -e '\.spawn\(' -e '::spawn\(' crates tests examples docs --type rust 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

if [[ -n "$output" ]]; then

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

      # Exempt: comment or attribute above
      if [[ "$prev_trimmed" =~ ^// ]] || [[ "$prev_trimmed" =~ ^\*[[:space:]] ]] ||
        [[ "$prev_trimmed" =~ ^\*/ ]] || [[ "$prev_trimmed" =~ ^/\* ]] ||
        [[ "$prev_trimmed" =~ ^#\[ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: string continuation (prev line ends with `\`)
      if [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: method chain continuation (`.spawn` as part of a builder chain)
      if [[ "$trimmed" =~ ^\. ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
      has_shared=false
      rest="$trimmed"
      while [[ "$rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
        ident="${BASH_REMATCH[1]}"
        rest="${BASH_REMATCH[2]}"
        case "$ident" in
          if | else | let | mut | ref | true | false | return | break | continue | match) ;;
          as | in | for | while | loop | fn | struct | enum | impl | trait | pub) ;;
          use | mod | const | static | type | where | async | await | move | unsafe) ;;
          extern | crate | super | dyn | self | Self) ;;
          spawn | tokio) ;;
          *)
            if [[ "$prev_content" =~ (^|[^a-zA-Z0-9_])${ident}([^a-zA-Z0-9_]|$) ]]; then
              has_shared=true
              break
            fi
            ;;
        esac
      done

      if ! $has_shared; then
        # Check first body line for shared identifiers with line above
        body_lines=$(sed -n "$((line_num + 1))p" "$file")
        body_rest="$body_lines"
        while [[ "$body_rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
          ident="${BASH_REMATCH[1]}"
          body_rest="${BASH_REMATCH[2]}"
          case "$ident" in
            if | else | let | mut | ref | true | false | return | break | continue | match) ;;
            as | in | for | while | loop | fn | struct | enum | impl | trait | pub) ;;
            use | mod | const | static | type | where | async | await | move | unsafe) ;;
            extern | crate | super | dyn | self | Self) ;;
            spawn | tokio) ;;
            *)
              if [[ "$prev_content" =~ (^|[^a-zA-Z0-9_])${ident}([^a-zA-Z0-9_]|$) ]]; then
                has_shared=true
                break
              fi
              ;;
          esac
        done
      fi

      if ! $has_shared; then
        echo -e "${RED}Error:${NC} Missing blank line above \`spawn\` in $file:$line_num"
        echo "  ${trimmed:0:100}"
        echo "  Line above: ${prev_trimmed:0:100}"
        echo
        VIOLATIONS=$((VIOLATIONS + 1))
      fi

      prev_content="$content"
    fi
  done <<< "$output"

fi

# ---------------------------------------------------------------------------
# Report results
# ---------------------------------------------------------------------------

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS formatting violation(s) (Rust)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC} Add a blank line above control flow blocks (\`if\`, \`match\`, \`for\`, \`while\`, \`loop\`, \`spawn\`)"
  echo "Exceptions: first expression in a block, or line above shares an identifier with the condition"
  exit 1
fi

echo "✓ Formatting conventions are valid (Rust)"
exit 0
