#!/usr/bin/env bash
# Enforces Ustr interning conventions: no redundant re-interning.
#
# Ustr implements Deref<Target = str>, so Ustr::from(&existing_ustr) silently
# coerces to From<&str> and performs a full hash + string-cache lookup for a
# value that is already interned. Clippy cannot see this because the coercion
# erases the type. These rules catch the laundering shapes textually.
#
# Rules (applied to production Rust code under crates/):
#   1. No Ustr::from(<expr>.inner().as_str()) or Ustr::from(&<expr>.inner()).
#      Every `inner()` method that can compile in these shapes returns a Ustr
#      (the non-Ustr `inner()` impls return struct references with no
#      as_str/deref-to-str path), so both shapes re-intern an interned value.
#      Fix: use `<expr>.inner()` directly.
#   2. No Ustr::from(&<recv>.<field>) or Ustr::from(<recv>.<field>.as_str())
#      for (path prefix, field) pairs in the curated map below, where the
#      field is verified Ustr on the venue message structs under that prefix.
#
# Use '// ustr-ok' inline comment to allow specific exceptions (for example,
# same-named String fields on other structs under a prefix).
#
# Test code (files under tests/, matching *_tests.rs, or an item gated by
# `#[cfg(test)]`) is excluded. Production code that follows a gated item in the
# same file is still checked.
#
# Limitation: redundant re-interns through local variables or function
# parameters of type Ustr (for example Ustr::from(cloid_hex.as_str()) where
# the parameter is &Ustr) are not textually distinguishable from legitimate
# first-time interns and are not covered.

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping Ustr convention checks"
  exit 0
fi

RED='\033[0;31m'
NC='\033[0m'

VIOLATIONS=0
ALLOW_MARKER="ustr-ok"

# Verified Ustr message fields from the re-interning audit. When a new
# adapter message struct declares a Ustr field, add a "<path-prefix>|<field>"
# entry here so consumers cannot launder it back through &str.
USTR_FIELD_MAP=(
  "crates/adapters/binance/src/futures|symbol"
  "crates/adapters/binance/src/spot/sbe|symbol"
  "crates/adapters/binance/src/spot/websocket|symbol"
  "crates/adapters/bitmex/src/websocket|symbol"
  "crates/adapters/deribit/src|instrument_name"
  "crates/adapters/derive/src|channel"
  "crates/adapters/dydx/src/common|ticker"
  "crates/adapters/okx/src|inst_id"
  "crates/adapters/polymarket/src|asset_id"
  "crates/adapters/tardis/src/machine|symbol"
)

# Normalize Windows backslash paths to POSIX so path matching works under
# Git Bash / MSYS2.
normalize_path() {
  printf '%s' "${1//\\//}"
}

is_test_path() {
  local file
  file=$(normalize_path "$1")
  [[ "$file" =~ /tests/ ]] && return 0
  [[ "$file" =~ /tests\.rs$ ]] && return 0
  [[ "$file" =~ _test\.rs$ ]] && return 0
  [[ "$file" =~ _tests\.rs$ ]] && return 0
  return 1
}

is_doc_comment() {
  local content="$1"
  [[ "$content" =~ ^[[:space:]]*/// ]] && return 0
  [[ "$content" =~ ^[[:space:]]*//! ]] && return 0
  return 1
}

# Return 0 if the given line number falls inside a test-gated item in the same
# file. Only `#[cfg(test)]` and `#[cfg(all(..., test, ...))]` gate an item for
# tests alone; `any(..., test)`, `not(test)`, and a quoted feature name such as
# `feature = "testers"` all keep the item in production builds. The attribute
# gates individual imports and functions as well as inline `mod tests` blocks,
# and any of them can sit above production code, so the exclusion covers only the
# gated item itself. rustfmt aligns an item's attributes and its closing brace
# with the item, so the attribute's own indentation bounds the block.
is_in_test_module() {
  local file="$1"
  local line_num="$2"

  # States: 1 attribute seen, 2 inside the item header, 3 inside the item body
  awk -v target="$line_num" '
    {
      gated = 0

      if (state == 3) {
        gated = 1
        if ($0 ~ close_re) {
          state = 0
        }
      } else if (state == 2) {
        gated = 1
        if ($0 !~ /^[[:space:]]*\/\//) {
          if ($0 ~ /\{[[:space:]]*$/) {
            state = 3
          } else if ($0 ~ /[;}][[:space:]]*$/) {
            state = 0
          }
        }
      } else if (state == 1) {
        gated = 1
        if ($0 !~ /^[[:space:]]*#\[/ && $0 !~ /^[[:space:]]*\/\//) {
          if ($0 ~ /\{[[:space:]]*$/) {
            state = 3
          } else if ($0 ~ /[;}][[:space:]]*$/) {
            state = 0
          } else {
            state = 2
          }
        }
      } else if ($0 ~ /^[[:space:]]*#\[cfg\((test\)|all\((test[,)]|[^]]*,[[:space:]]*test[,)]))/) {
        gated = 1
        state = 1
        match($0, /^[[:space:]]*/)
        close_re = "^" substr($0, 1, RLENGTH) "\\}[;,]?[[:space:]]*$"
      }

      if (NR == target) {
        inside = gated
      }
    }
    END { exit(inside ? 0 : 1) }
  ' "$file"
}

report() {
  local rule="$1"
  local file="$2"
  local line="$3"
  local content="$4"
  local hint="$5"

  local trimmed="${content#"${content%%[![:space:]]*}"}"
  echo -e "${RED}Error ($rule):${NC} $file:$line"
  echo "  Found: $trimmed"
  [[ -n "$hint" ]] && echo "  Hint:  $hint"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
}

check_hit() {
  local rule="$1"
  local file="$2"
  local line_num="$3"
  local content="$4"
  local hint="$5"

  [[ -z "$file" ]] && return
  local norm_file
  norm_file=$(normalize_path "$file")
  is_test_path "$norm_file" && return
  is_in_test_module "$file" "$line_num" && return
  is_doc_comment "$content" && return
  [[ "$content" =~ $ALLOW_MARKER ]] && return

  report "$rule" "$norm_file" "$line_num" "$content" "$hint"
}

################################################################################
# Rule 1: identifier inner() values laundered back through &str
################################################################################

echo "Checking Ustr re-interning of identifier inner() values..."

while IFS=: read -r file line_num content; do
  check_hit "rule1" "$file" "$line_num" "$content" \
    "Use '<expr>.inner()' directly; the value is already interned"
done < <(rg -n --no-heading \
  '(ustr::)?Ustr::from\(&?[A-Za-z_][A-Za-z0-9_\.]*\.inner\(\)\.as_str\(\)\)' \
  crates --type rust 2> /dev/null || true)

while IFS=: read -r file line_num content; do
  check_hit "rule1" "$file" "$line_num" "$content" \
    "Use '<expr>.inner()' directly; the value is already interned"
done < <(rg -n --no-heading \
  '(ustr::)?Ustr::from\(&[A-Za-z_][A-Za-z0-9_\.]*\.inner\(\)\)' \
  crates --type rust 2> /dev/null || true)

################################################################################
# Rule 2: curated Ustr message fields laundered back through &str
################################################################################

echo "Checking Ustr re-interning of curated message fields..."

for entry in "${USTR_FIELD_MAP[@]}"; do
  prefix="${entry%%|*}"
  field="${entry##*|}"

  # Ustr::from(&<recv-chain>.<field>) where the field is a verified Ustr
  while IFS=: read -r file line_num content; do
    check_hit "rule2" "$file" "$line_num" "$content" \
      "'$field' is already a Ustr under $prefix; use the field directly"
  done < <(rg -n --no-heading \
    "(ustr::)?Ustr::from\(&[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*\.$field\)" \
    "$prefix" --type rust 2> /dev/null || true)

  # Ustr::from(<recv-chain>.<field>.as_str()) where the field is a verified Ustr
  while IFS=: read -r file line_num content; do
    check_hit "rule2" "$file" "$line_num" "$content" \
      "'$field' is already a Ustr under $prefix; use the field directly"
  done < <(rg -n --no-heading \
    "(ustr::)?Ustr::from\(&?[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*\.$field\.as_str\(\)\)" \
    "$prefix" --type rust 2> /dev/null || true)
done

################################################################################
# Summary
################################################################################

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS Ustr convention violation(s)${NC}"
  echo
  echo "Add '// ustr-ok' inline comment to allow specific exceptions"
  exit 1
fi

echo "✓ All Ustr conventions are valid"
exit 0
