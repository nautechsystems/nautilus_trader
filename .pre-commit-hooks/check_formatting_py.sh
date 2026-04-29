#!/usr/bin/env bash

# Enforces formatting conventions in Python code.

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping formatting checks (Python)"
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
# a) It is the first statement in a block (preceded by line ending with `:`)
# b) It is the first statement after a docstring closing (`"""` or `'''`)
# c) It is part of an `elif`/`else` chain
# d) The line above is a comment (attached to the `if`)
# e) The line above is a decorator
# f) The previous line is a comprehension `for` clause (the `if` is a filter)
# g) The line above is more indented (exiting a block body, e.g. sequential guards)
# h) An identifier from the `if` condition appears on the line directly above
# i) The `if` is a continuation (prev line ends with `\`, `(`, `[`, `,`)
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`if\` statements (Python)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading '^\s*if\s' nautilus_trader tests examples docs python build.py --type py 2> /dev/null) || rg_exit=$?
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

      # Skip ternary/expression `if` (not an `if` statement)
      # An `if` statement line ends with `:`, `(`, `[`, `{`, or `\`
      # Check raw line first (avoids false strip of `#` inside strings like `"#["`)
      is_statement=false
      if [[ "$trimmed" =~ :[[:space:]]*$ ]] ||
        [[ "$trimmed" =~ [\(\[\{][[:space:]]*$ ]] ||
        [[ "$trimmed" =~ \\[[:space:]]*$ ]]; then
        is_statement=true
      else
        stripped_comment="${trimmed%%#*}"
        stripped_comment="${stripped_comment%"${stripped_comment##*[![:space:]]}"}"
        if [[ -n "$stripped_comment" ]] &&
          { [[ "$stripped_comment" =~ :[[:space:]]*$ ]] ||
            [[ "$stripped_comment" =~ [\(\[\{][[:space:]]*$ ]] ||
            [[ "$stripped_comment" =~ \\[[:space:]]*$ ]]; }; then
          is_statement=true
        fi
      fi
      if ! $is_statement; then
        prev_content="$content"
        continue
      fi

      prev_trimmed="${prev_content#"${prev_content%%[![:space:]]*}"}"

      # Exempt: blank line above
      if [[ -z "$prev_trimmed" ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: first statement in a block (prev line ends with `:`)
      # Check raw line first (avoids false strip of `#` inside strings like `"#["`)
      # Then check with trailing comment stripped for `def foo():  # noqa` patterns
      if [[ "$prev_trimmed" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi
      prev_no_comment="${prev_trimmed%%#*}"
      prev_no_comment="${prev_no_comment%"${prev_no_comment##*[![:space:]]}"}"
      if [[ -n "$prev_no_comment" ]] && [[ "$prev_no_comment" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: first statement after docstring
      if [[ "$prev_trimmed" =~ \"\"\" ]] || [[ "$prev_trimmed" =~ \'\'\' ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: comprehension `if` filter (prev line is a comprehension `for`)
      if [[ "$prev_trimmed" =~ ^for[[:space:]] ]] && ! [[ "$prev_trimmed" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: comment above (attached to the `if`)
      if [[ "$prev_trimmed" =~ ^# ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: decorator above
      if [[ "$prev_trimmed" =~ ^@ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: continuation (prev line ends with `\`, `(`, `[`, `,`)
      if [[ "$prev_trimmed" =~ [,\(\[][[:space:]]*$ ]] ||
        [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: previous line is more indented (exiting a block body, e.g. sequential guards)
      prev_indent=${#prev_content}
      prev_indent=$((prev_indent - ${#prev_trimmed}))
      match_indent=${#content}
      match_indent=$((match_indent - ${#trimmed}))
      if [[ $prev_indent -gt $match_indent ]]; then
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
          if | elif | else | and | or | not | in | is | True | False | None | return) ;;
          break | continue | pass | raise | yield | lambda | del | assert | global) ;;
          class | def | for | while | with | try | except | finally | import | from) ;;
          as | async | await | nonlocal) ;;
          isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
            if | elif | else | and | or | not | in | is | True | False | None | return) ;;
            break | continue | pass | raise | yield | lambda | del | assert | global) ;;
            class | def | for | while | with | try | except | finally | import | from) ;;
            as | async | await | nonlocal | match | case) ;;
            isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
# Check: blank line above `match` statements
#
# A `match` must have a blank line above unless:
# a) It is the first statement in a block (preceded by line ending with `:`)
# b) It is the first statement after a docstring closing
# c) The line above is a comment
# d) The line above is a decorator
# e) The previous line is more indented (exiting a block body)
# f) An identifier from the `match` expression appears on the line above
# g) The `match` is a continuation (prev line ends with `\`, `(`, `[`, `,`)
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`match\` statements (Python)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading '^\s*match\s' nautilus_trader tests examples docs python build.py --type py 2> /dev/null) || rg_exit=$?
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

      # Skip non-statement `match` (e.g. variable named `match` in assignment)
      is_statement=false
      if [[ "$trimmed" =~ :[[:space:]]*$ ]] ||
        [[ "$trimmed" =~ [\(\[\{][[:space:]]*$ ]] ||
        [[ "$trimmed" =~ \\[[:space:]]*$ ]]; then
        is_statement=true
      else
        stripped_comment="${trimmed%%#*}"
        stripped_comment="${stripped_comment%"${stripped_comment##*[![:space:]]}"}"
        if [[ -n "$stripped_comment" ]] &&
          { [[ "$stripped_comment" =~ :[[:space:]]*$ ]] ||
            [[ "$stripped_comment" =~ [\(\[\{][[:space:]]*$ ]] ||
            [[ "$stripped_comment" =~ \\[[:space:]]*$ ]]; }; then
          is_statement=true
        fi
      fi
      if ! $is_statement; then
        prev_content="$content"
        continue
      fi

      prev_trimmed="${prev_content#"${prev_content%%[![:space:]]*}"}"

      # Exempt: blank line above
      if [[ -z "$prev_trimmed" ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: first statement in a block (prev line ends with `:`)
      if [[ "$prev_trimmed" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi
      prev_no_comment="${prev_trimmed%%#*}"
      prev_no_comment="${prev_no_comment%"${prev_no_comment##*[![:space:]]}"}"
      if [[ -n "$prev_no_comment" ]] && [[ "$prev_no_comment" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: first statement after docstring
      if [[ "$prev_trimmed" =~ \"\"\" ]] || [[ "$prev_trimmed" =~ \'\'\' ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: comment above
      if [[ "$prev_trimmed" =~ ^# ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: decorator above
      if [[ "$prev_trimmed" =~ ^@ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: continuation (prev line ends with `\`, `(`, `[`, `,`)
      if [[ "$prev_trimmed" =~ [,\(\[][[:space:]]*$ ]] ||
        [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: previous line is more indented (exiting a block body)
      prev_indent=${#prev_content}
      prev_indent=$((prev_indent - ${#prev_trimmed}))
      match_indent=${#content}
      match_indent=$((match_indent - ${#trimmed}))
      if [[ $prev_indent -gt $match_indent ]]; then
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
          if | elif | else | and | or | not | in | is | True | False | None | return) ;;
          break | continue | pass | raise | yield | lambda | del | assert | global) ;;
          class | def | for | while | with | try | except | finally | import | from) ;;
          as | async | await | nonlocal | match | case) ;;
          isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
            if | elif | else | and | or | not | in | is | True | False | None | return) ;;
            break | continue | pass | raise | yield | lambda | del | assert | global) ;;
            class | def | for | while | with | try | except | finally | import | from) ;;
            as | async | await | nonlocal | match | case) ;;
            isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
# Check: blank line above `for` statements
#
# A `for` must have a blank line above unless:
# a) It is a comprehension `for` (line does not end with `:`)
# b) It is the first statement in a block (preceded by line ending with `:`)
# c) It is the first statement after a docstring closing
# d) The line above is a comment
# e) The line above is a decorator
# f) The previous line is more indented (exiting a block body)
# g) An identifier from the `for` line appears on the line above
# h) The `for` is a continuation (prev line ends with `\`, `(`, `[`, `,`)
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`for\` statements (Python)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading '^\s*(async\s+)?for\s' nautilus_trader tests examples docs python build.py --type py 2> /dev/null) || rg_exit=$?
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

      # Skip comprehension `for` (not a `for` statement)
      # `async for` is always a statement; regular `for` must end with `:`, `(`, `[`, `{`, or `\`
      if ! [[ "$trimmed" =~ ^async ]]; then
        is_statement=false
        if [[ "$trimmed" =~ :[[:space:]]*$ ]] ||
          [[ "$trimmed" =~ [\(\[\{][[:space:]]*$ ]] ||
          [[ "$trimmed" =~ \\[[:space:]]*$ ]]; then
          is_statement=true
        else
          stripped_comment="${trimmed%%#*}"
          stripped_comment="${stripped_comment%"${stripped_comment##*[![:space:]]}"}"
          if [[ -n "$stripped_comment" ]] &&
            { [[ "$stripped_comment" =~ :[[:space:]]*$ ]] ||
              [[ "$stripped_comment" =~ [\(\[\{][[:space:]]*$ ]] ||
              [[ "$stripped_comment" =~ \\[[:space:]]*$ ]]; }; then
            is_statement=true
          fi
        fi
        if ! $is_statement; then
          prev_content="$content"
          continue
        fi
      fi

      prev_trimmed="${prev_content#"${prev_content%%[![:space:]]*}"}"

      # Exempt: blank line above
      if [[ -z "$prev_trimmed" ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: first statement in a block (prev line ends with `:`)
      if [[ "$prev_trimmed" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi
      prev_no_comment="${prev_trimmed%%#*}"
      prev_no_comment="${prev_no_comment%"${prev_no_comment##*[![:space:]]}"}"
      if [[ -n "$prev_no_comment" ]] && [[ "$prev_no_comment" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: first statement after docstring
      if [[ "$prev_trimmed" =~ \"\"\" ]] || [[ "$prev_trimmed" =~ \'\'\' ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: comment above
      if [[ "$prev_trimmed" =~ ^# ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: decorator above
      if [[ "$prev_trimmed" =~ ^@ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: continuation (prev line ends with `\`, `(`, `[`, `,`)
      if [[ "$prev_trimmed" =~ [,\(\[][[:space:]]*$ ]] ||
        [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: previous line is more indented (exiting a block body)
      prev_indent=${#prev_content}
      prev_indent=$((prev_indent - ${#prev_trimmed}))
      match_indent=${#content}
      match_indent=$((match_indent - ${#trimmed}))
      if [[ $prev_indent -gt $match_indent ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
      for_rest="${trimmed#async }"
      for_rest="${for_rest#for }"
      has_shared=false
      rest="$for_rest"
      while [[ "$rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
        ident="${BASH_REMATCH[1]}"
        rest="${BASH_REMATCH[2]}"
        case "$ident" in
          if | elif | else | and | or | not | in | is | True | False | None | return) ;;
          break | continue | pass | raise | yield | lambda | del | assert | global) ;;
          class | def | for | while | with | try | except | finally | import | from) ;;
          as | async | await | nonlocal) ;;
          isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
            if | elif | else | and | or | not | in | is | True | False | None | return) ;;
            break | continue | pass | raise | yield | lambda | del | assert | global) ;;
            class | def | for | while | with | try | except | finally | import | from) ;;
            as | async | await | nonlocal) ;;
            isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
# a) It is the first statement in a block (preceded by line ending with `:`)
# b) It is the first statement after a docstring closing
# c) The line above is a comment
# d) The line above is a decorator
# e) The previous line is more indented (exiting a block body)
# f) An identifier from the `while` condition appears on the line above
# g) The `while` is a continuation (prev line ends with `\`, `(`, `[`, `,`)
# ---------------------------------------------------------------------------

echo "Checking for blank line above \`while\` loops (Python)..."

rg_exit=0
output=$(rg -n -B 1 --no-heading '^\s*while\s' nautilus_trader tests examples docs python build.py --type py 2> /dev/null) || rg_exit=$?
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

      # Exempt: first statement in a block (prev line ends with `:`)
      if [[ "$prev_trimmed" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi
      prev_no_comment="${prev_trimmed%%#*}"
      prev_no_comment="${prev_no_comment%"${prev_no_comment##*[![:space:]]}"}"
      if [[ -n "$prev_no_comment" ]] && [[ "$prev_no_comment" =~ :[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: first statement after docstring
      if [[ "$prev_trimmed" =~ \"\"\" ]] || [[ "$prev_trimmed" =~ \'\'\' ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: comment above
      if [[ "$prev_trimmed" =~ ^# ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: decorator above
      if [[ "$prev_trimmed" =~ ^@ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: continuation (prev line ends with `\`, `(`, `[`, `,`)
      if [[ "$prev_trimmed" =~ [,\(\[][[:space:]]*$ ]] ||
        [[ "$prev_trimmed" =~ \\[[:space:]]*$ ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: previous line is more indented (exiting a block body)
      prev_indent=${#prev_content}
      prev_indent=$((prev_indent - ${#prev_trimmed}))
      match_indent=${#content}
      match_indent=$((match_indent - ${#trimmed}))
      if [[ $prev_indent -gt $match_indent ]]; then
        prev_content="$content"
        continue
      fi

      # Exempt: shared identifier with line above
      while_rest="${trimmed#while }"
      has_shared=false
      rest="$while_rest"
      while [[ "$rest" =~ ([a-zA-Z_][a-zA-Z0-9_]*)(.*) ]]; do
        ident="${BASH_REMATCH[1]}"
        rest="${BASH_REMATCH[2]}"
        case "$ident" in
          if | elif | else | and | or | not | in | is | True | False | None | return) ;;
          break | continue | pass | raise | yield | lambda | del | assert | global) ;;
          class | def | for | while | with | try | except | finally | import | from) ;;
          as | async | await | nonlocal | match | case) ;;
          isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
            if | elif | else | and | or | not | in | is | True | False | None | return) ;;
            break | continue | pass | raise | yield | lambda | del | assert | global) ;;
            class | def | for | while | with | try | except | finally | import | from) ;;
            as | async | await | nonlocal | match | case) ;;
            isinstance | issubclass | hasattr | getattr | setattr | type | len | range) ;;
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
# Report results
# ---------------------------------------------------------------------------

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS formatting violation(s) (Python)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC} Add a blank line above control flow blocks (\`if\`, \`match\`, \`for\`, \`while\`)"
  echo "Exceptions: first statement in a block, line above shares an identifier with the condition"
  exit 1
fi

echo "✓ Formatting conventions are valid (Python)"
exit 0
