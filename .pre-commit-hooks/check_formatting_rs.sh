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
# Check: blank line above control flow
#
# An `if`, `match`, `for`, `while`, `loop`, or `spawn` line must have a blank
# line above unless:
# a) It is the first expression in a block (preceded by line ending with `{`)
# b) An identifier from the line or its first body line appears on the line
#    directly above
# c) The line above is a comment or attribute (attached to the statement)
# d) The line above ends with `\` (string continuation)
# e) `if` and `match` are expressions or arguments: the line above ends with
#    `=`, `,`, `(`, or `|`; `if` also after `)`, a leading `|`, a
#    multi-alternative match pattern, an `else`, or as a match arm guard
# f) `for`, `while`, and `loop` follow a loop label
# g) `spawn` continues a method chain
#
# One awk pass reads every file and applies the rules; bash only formats the
# violations it reports.
# ---------------------------------------------------------------------------

# rg exits 0 (files listed), 1 (no files), or 2+ (error)
rg_exit=0
rust_files=$(rg --files crates examples docs --type rust --sort path 2> /dev/null) || rg_exit=$?
if [ $rg_exit -gt 1 ]; then
  echo "ERROR: ripgrep failed with exit code $rg_exit"
  exit 1
fi

control_flow_output=""
if [[ -n "$rust_files" ]]; then
  control_flow_output=$(LC_ALL=C awk '
    function is_match_guard(start,    j) {
      for (j = start; j <= total; j++) {
        if (j > start && lines[j] ~ /^[[:space:]]*if[[:space:]]/) return 0
        if (lines[j] ~ /=>[[:space:]]*$/) return 1
        if (lines[j] ~ /[{;]/) return 0
      }
      return 0
    }

    function condition_text(keyword, trimmed,    rest, label_end) {
      rest = trimmed
      if (keyword == "for" || keyword == "while" || keyword == "loop") {
        if (substr(rest, 1, 1) == quote) {
          label_end = index(rest, ": ")
          if (label_end > 0) rest = substr(rest, label_end + 2)
        }
      }
      if (keyword != "spawn") sub("^" keyword " ", "", rest)
      return rest
    }

    function shares_identifier(keyword, text, previous,    count, parts, p, present, rest, ident) {
      count = split(previous, parts, /[^a-zA-Z0-9_]+/)
      for (p = 1; p <= count; p++) {
        if (parts[p] != "") present[parts[p]] = 1
      }

      rest = text
      while (match(rest, /[a-zA-Z_][a-zA-Z0-9_]*/)) {
        ident = substr(rest, RSTART, RLENGTH)
        rest = substr(rest, RSTART + RLENGTH)
        if (ident in reserved) continue
        if (keyword == "spawn" && (ident == "spawn" || ident == "tokio")) continue
        if (ident in present) return 1
      }
      return 0
    }

    function is_exempt(keyword, i, trimmed, previous, prev_trimmed) {
      if (prev_trimmed == "") return 1
      if (prev_trimmed ~ /\{[[:space:]]*$/) return 1
      if (prev_trimmed ~ /^\/\// || prev_trimmed ~ /^\*[[:space:]]/ || prev_trimmed ~ /^\*\//) return 1
      if (prev_trimmed ~ /^\/\*/ || prev_trimmed ~ /^#\[/) return 1
      if (prev_trimmed ~ /\\[[:space:]]*$/) return 1

      if (keyword == "if") {
        if (prev_trimmed ~ /\}[[:space:]]*else[[:space:]]*$/ || prev_trimmed ~ /^else[[:space:]]*$/) return 1
        if (prev_trimmed ~ /=[[:space:]]*$/ && prev_trimmed !~ /[=!<>]=[[:space:]]*$/) return 1
        if (prev_trimmed ~ /[,()|][[:space:]]*$/ || prev_trimmed ~ /^\|/) return 1
        if (prev_trimmed ~ /[[:alnum:]][[:space:]]*\|[[:space:]]*[[:alnum:]]/ && prev_trimmed !~ /\|\|/) return 1
        if (is_match_guard(i)) return 1
      } else if (keyword == "match") {
        if (prev_trimmed ~ /=[[:space:]]*$/ && prev_trimmed !~ /[=!<>]=[[:space:]]*$/) return 1
        if (prev_trimmed ~ /[,(|][[:space:]]*$/) return 1
      } else if (keyword == "spawn") {
        if (trimmed ~ /^\./) return 1
      } else if (prev_trimmed ~ label_line) {
        return 1
      }

      if (shares_identifier(keyword, condition_text(keyword, trimmed), previous)) return 1
      if (i < total && shares_identifier(keyword, lines[i + 1], previous)) return 1
      return 0
    }

    BEGIN {
      quote = sprintf("%c", 39)
      # Bytes above 0x7F admit the non-ASCII identifier continuations that
      # ripgrep matched with \w under LC_ALL=C.
      label = "(" quote "[a-zA-Z_][a-zA-Z0-9_\200-\377]*:[[:space:]]*)?"
      label_line = "^" quote "[a-zA-Z_]"
      prefilter = "^(if|match|for|while|loop|" quote ")"
      keyword_count = split("if match for while loop spawn", keywords, " ")
      patterns["if"] = "^if[[:space:]]"
      patterns["match"] = "^match[[:space:]]"
      patterns["for"] = "^" label "for[[:space:]]"
      patterns["while"] = "^" label "while[[:space:]]"
      patterns["loop"] = "^" label "loop[[:space:]]"
      patterns["spawn"] = "^spawn\\(|\\.spawn\\(|::spawn\\("

      reserved_words = "if else let mut ref true false return break continue match"
      reserved_words = reserved_words " as in for while loop fn struct enum impl trait pub"
      reserved_words = reserved_words " use mod const static type where async await move unsafe"
      reserved_words = reserved_words " extern crate super dyn self Self"
      word_count = split(reserved_words, words, " ")
      for (w = 1; w <= word_count; w++) reserved[words[w]] = 1
    }

    {
      file = $0
      total = 0
      while ((status = (getline line < file)) > 0) lines[++total] = line
      close(file)
      if (status < 0) {
        print "ERROR: cannot read " file > "/dev/stderr"
        exit 1
      }

      for (i = 2; i <= total; i++) {
        trimmed = lines[i]
        sub(/^[[:space:]]+/, "", trimmed)
        if (trimmed !~ prefilter && index(trimmed, "spawn(") == 0) continue
        for (k = 1; k <= keyword_count; k++) {
          keyword = keywords[k]
          if (trimmed !~ patterns[keyword]) continue
          previous = lines[i - 1]
          prev_trimmed = previous
          sub(/^[[:space:]]+/, "", prev_trimmed)
          if (is_exempt(keyword, i, trimmed, previous, prev_trimmed)) continue
          violations[keyword]++
          records[keyword, violations[keyword]] = keyword "\t" i "\t" file "\n" trimmed "\n" prev_trimmed
        }
      }
      split("", lines)
    }

    END {
      for (k = 1; k <= keyword_count; k++) {
        keyword = keywords[k]
        for (n = 1; n <= violations[keyword] + 0; n++) print records[keyword, n]
      }
    }
  ' <<< "$rust_files")
fi

# Each violation is a keyword, line, and path record followed by the offending
# line and the line above on their own lines, so source text never splits fields
# and the path, as the last field, keeps any tab it contains.
report_control_flow() {
  local keyword="$1"
  local record_keyword line_num file current previous

  while IFS=$'\t' read -r record_keyword line_num file &&
    IFS= read -r current &&
    IFS= read -r previous; do
    [[ "$record_keyword" == "$keyword" ]] || continue

    echo -e "${RED}Error:${NC} Missing blank line above \`$keyword\` in $file:$line_num"
    echo "  ${current:0:100}"
    echo "  Line above: ${previous:0:100}"
    echo
    VIOLATIONS=$((VIOLATIONS + 1))
  done <<< "$control_flow_output"
}

echo "Checking for blank line above \`if\` statements (Rust)..."
report_control_flow if

echo "Checking for blank line above \`match\` blocks (Rust)..."
report_control_flow match

echo "Checking for blank line above \`for\` loops (Rust)..."
report_control_flow for

echo "Checking for blank line above \`while\` loops (Rust)..."
report_control_flow while

echo "Checking for blank line above \`loop\` blocks (Rust)..."
report_control_flow loop

echo "Checking for blank line above \`spawn\` calls (Rust)..."
report_control_flow spawn

# Check: module declaration ordering in `mod.rs`
#
# External module declarations must be alphabetized within these sections:
# `#[macro_use]`, public, restricted, cfg-gated, private, and test-only.
# Adjacent non-empty sections must have exactly one blank line between them.

echo "Checking module declaration ordering (Rust)..."

CONTROL_FLOW_VIOLATIONS=$VIOLATIONS
MODULE_ORDER_VIOLATIONS=0

while IFS= read -r file; do
  module_output=$(LC_ALL=C awk '
    function reset_block() {
      previous_category = -1
      highest_category = -1
      previous_name = ""
      blank_lines = 0
      blank_run = 0
      blank_run_max = 0
      has_intervening_comment = 0
    }

    function category_name(category) {
      if (category == 0) return "macro prelude"
      if (category == 1) return "public"
      if (category == 2) return "restricted"
      if (category == 3) return "cfg-gated"
      if (category == 4) return "private"
      return "test-only"
    }

    function report(message) {
      printf "%d\t%s\n", FNR, message
    }

    function has_direct_test(attributes, start, rest, position, character, depth, token) {
      start = index(attributes, "#[cfg(all(")
      while (start > 0) {
        rest = substr(attributes, start + length("#[cfg(all("))
        depth = 0
        token = ""

        for (position = 1; position <= length(rest); position++) {
          character = substr(rest, position, 1)
          if (depth > 0) {
            if (character == "(") depth++
            if (character == ")") depth--
          } else if (character == "(") {
            depth = 1
            token = ""
          } else if (character == "," || character == ")") {
            if (token == "test") return 1
            token = ""
            if (character == ")") break
          } else {
            token = token character
          }
        }

        attributes = substr(rest, position + 1)
        start = index(attributes, "#[cfg(all(")
      }

      return 0
    }

    BEGIN {
      reset_block()
      attributes = ""
      in_attribute = 0
      in_block_comment = 0
    }

    {
      line = $0

      if (in_attribute) {
        attributes = attributes line
        if (line ~ /]/) in_attribute = 0
        next
      }

      if (in_block_comment) {
        if (line ~ /[*]\//) in_block_comment = 0
        next
      }

      if (line ~ /^[[:space:]]*$/) {
        if (previous_category >= 0) {
          blank_lines++
          blank_run++
          if (blank_run > blank_run_max) blank_run_max = blank_run
        }
        next
      }

      if (line ~ /^\/\*/) {
        if (previous_category >= 0) has_intervening_comment = 1
        blank_run = 0
        if (line !~ /[*]\//) in_block_comment = 1
        next
      }

      if (line ~ /^\/\//) {
        if (previous_category >= 0) has_intervening_comment = 1
        blank_run = 0
        next
      }

      if (line ~ /^#\[/) {
        blank_run = 0
        attributes = attributes line
        if (line !~ /]/) in_attribute = 1
        next
      }

      if (line ~ /^(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;([[:space:]]*\/\/.*)?$/) {
        compact_attributes = attributes
        gsub(/[[:space:]]/, "", compact_attributes)

        name = line
        sub(/[[:space:]]*\/\/.*$/, "", name)
        sub(/[[:space:]]*;[[:space:]]*$/, "", name)
        sub(/^.*mod[[:space:]]+/, "", name)

        if (compact_attributes ~ /#\[macro_use\]/) {
          category = 0
        } else if (compact_attributes ~ /#\[cfg\(test\)\]/ || has_direct_test(compact_attributes)) {
          category = 5
        } else if (compact_attributes ~ /#\[cfg\(/) {
          category = 3
        } else if (line ~ /^pub[[:space:]]+mod[[:space:]]+/) {
          category = 1
        } else if (line ~ /^pub\([^)]*\)[[:space:]]+mod[[:space:]]+/) {
          category = 2
        } else {
          category = 4
        }

        if (previous_category >= 0 && category != previous_category) {
          valid_spacing = blank_lines == 1 ||
                          (has_intervening_comment && blank_lines > 0 && blank_run_max == 1)
          if (!valid_spacing) {
            message = "Expected one blank line before " category_name(category)
            report(message " module `" name "`, found " blank_lines)
          }
        }

        if (category < highest_category) {
          message = "Module `" name "` is in the wrong section; expected order: "
          report(message "macro prelude, public, restricted, cfg-gated, private, test-only")
        } else if (category > highest_category) {
          highest_category = category
        }

        if (category == previous_category && name < previous_name) {
          message = "Module `" name "` is not alphabetized in the "
          report(message category_name(category) " section")
        }

        previous_category = category
        previous_name = name
        blank_lines = 0
        blank_run = 0
        blank_run_max = 0
        has_intervening_comment = 0
        attributes = ""
        next
      }

      reset_block()
      attributes = ""
    }
  ' "$file")

  if [[ -z "$module_output" ]]; then
    continue
  fi

  while IFS=$'\t' read -r line_num message; do
    echo -e "${RED}Error:${NC} $message in $file:$line_num"
    echo
    MODULE_ORDER_VIOLATIONS=$((MODULE_ORDER_VIOLATIONS + 1))
  done <<< "$module_output"
done < <(rg --files crates examples docs -g 'mod.rs' -g '!**/generated/**' 2> /dev/null)

VIOLATIONS=$((VIOLATIONS + MODULE_ORDER_VIOLATIONS))

# ---------------------------------------------------------------------------
# Report results
# ---------------------------------------------------------------------------

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS formatting violation(s) (Rust)${NC}"
  echo
  if [ $CONTROL_FLOW_VIOLATIONS -gt 0 ]; then
    echo -e "${YELLOW}To fix control flow:${NC} Add a blank line above \`if\`, \`match\`, \`for\`, \`while\`, \`loop\`, and \`spawn\`"
    echo "Exceptions: first expression in a block, or line above shares an identifier with the condition"
  fi
  if [ $MODULE_ORDER_VIOLATIONS -gt 0 ]; then
    echo -e "${YELLOW}To fix modules:${NC} Order and alphabetize sections as public, restricted, cfg-gated, private, test-only"
  fi
  exit 1
fi

echo "Formatting conventions are valid (Rust)"
exit 0
