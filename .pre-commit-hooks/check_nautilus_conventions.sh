#!/usr/bin/env bash
# Enforces Nautilus conventions:
# 1. Nautilus domain types should not be fully qualified in code
#    (identifiers, data types, enums, etc. should be imported and used directly)
# 2. Box-style banner comments are not allowed
# 3. std::fmt conventions: import Debug/Display (use as `impl Debug`/`impl Display`),
#    but fully qualify std::fmt::Formatter and std::fmt::Result (do not import them)
# 4. debug_struct should always use stringify! macro for its value
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
# Exclude nautilus_macros: proc-macros generate code for other crates; fully qualified paths required
output=$(rg -n -B 5 \
  --no-heading \
  --glob '!crates/persistence/macros/**' \
  'nautilus_[a-z_]+(?:::[a-z_]+)+::[A-Z][A-Za-z0-9_]+' \
  crates examples \
  --type rust \
  2> /dev/null || true)

VIOLATIONS=0

if [[ -z "$output" ]]; then
  echo "Nautilus import conventions are valid"
else
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
  else
    echo "Nautilus import conventions are valid"
  fi

fi # end of if [[ -z "$output" ]]

# Check for box-style banner comments
echo "Checking for box-style banner comments..."

BANNER_VIOLATIONS=0
banner_output=$(rg -n --no-heading '^\s*// ={5,}' crates --type rust 2> /dev/null || true)

if [[ -n "$banner_output" ]]; then
  echo
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"
      echo -e "${RED}Error:${NC} Box-style banner comment in $file:$line_num"
      echo "  ${line_content:0:80}"
      BANNER_VIOLATIONS=$((BANNER_VIOLATIONS + 1))
    fi
  done <<< "$banner_output"

  echo
  echo -e "${RED}Found $BANNER_VIOLATIONS box-style banner comment(s)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC} Remove box-style banners (// ====...====)"
  echo "Use module structure, impl blocks, or doc comments instead"
  echo "See: docs/developer_guide/rust.md#box-style-banner-comments"
else
  echo "No box-style banner comments found"
fi

# Check for std::fmt convention violations
echo "Checking for std::fmt convention violations..."

FMT_VIOLATIONS=0

# Check 1: impl std::fmt::Debug should be impl Debug
fmt_debug_output=$(rg -n --no-heading 'impl\s+std::fmt::Debug' crates --type rust 2> /dev/null || true)

if [[ -n "$fmt_debug_output" ]]; then
  echo
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"
      echo -e "${RED}Error:${NC} Use 'impl Debug' instead of 'impl std::fmt::Debug' in $file:$line_num"
      echo "  ${line_content:0:100}"
      FMT_VIOLATIONS=$((FMT_VIOLATIONS + 1))
    fi
  done <<< "$fmt_debug_output"
fi

# Check 1b: impl std::fmt::Display should be impl Display
fmt_display_output=$(rg -n --no-heading 'impl\s+std::fmt::Display' crates --type rust 2> /dev/null || true)

if [[ -n "$fmt_display_output" ]]; then
  echo
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"
      echo -e "${RED}Error:${NC} Use 'impl Display' instead of 'impl std::fmt::Display' in $file:$line_num"
      echo "  ${line_content:0:100}"
      FMT_VIOLATIONS=$((FMT_VIOLATIONS + 1))
    fi
  done <<< "$fmt_display_output"
fi

# Check 2: Formatter and Result should not be imported from std::fmt
# Match patterns like: use std::fmt::Formatter, use std::fmt::{..., Formatter, ...}
fmt_import_output=$(rg -n --no-heading 'use\s+std::fmt::\{[^}]*(Formatter|Result)[^}]*\}|use\s+std::fmt::(Formatter|Result)\b' crates --type rust 2> /dev/null || true)

if [[ -n "$fmt_import_output" ]]; then
  echo
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"
      echo -e "${RED}Error:${NC} Do not import Formatter/Result from std::fmt in $file:$line_num"
      echo "  ${line_content:0:100}"
      echo "  Use std::fmt::Formatter and std::fmt::Result directly instead"
      FMT_VIOLATIONS=$((FMT_VIOLATIONS + 1))
    fi
  done <<< "$fmt_import_output"
fi

# Check 3: fmt::Formatter and fmt::Result should be std::fmt::Formatter and std::fmt::Result
# This catches the pattern where someone does `use std::fmt;` then uses `fmt::Formatter`
fmt_shorthand_output=$(rg -n --no-heading '\bfmt::(Formatter|Result)\b' crates --type rust 2> /dev/null | grep -v 'std::fmt::' | grep -v '::core::fmt::' | grep -v 'core::fmt::' || true)

if [[ -n "$fmt_shorthand_output" ]]; then
  echo
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"
      echo -e "${RED}Error:${NC} Use std::fmt::Formatter/Result instead of fmt::Formatter/Result in $file:$line_num"
      echo "  ${line_content:0:100}"
      FMT_VIOLATIONS=$((FMT_VIOLATIONS + 1))
    fi
  done <<< "$fmt_shorthand_output"
fi

# Check 4: Bare Formatter< usage (not preceded by std::fmt:: or core::fmt:: or fmt::)
# This catches direct usage like `f: &mut Formatter<'_>` without qualification
# Excludes fmt::Formatter which is caught by Check 3
bare_formatter_output=$(rg -n --no-heading '\bFormatter<' crates --type rust 2> /dev/null | grep -v 'std::fmt::Formatter' | grep -v '::core::fmt::Formatter' | grep -v 'core::fmt::Formatter' | grep -v 'fmt::Formatter' | grep -v '/generated/' || true)

if [[ -n "$bare_formatter_output" ]]; then
  echo
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"
      echo -e "${RED}Error:${NC} Use std::fmt::Formatter instead of bare Formatter in $file:$line_num"
      echo "  ${line_content:0:100}"
      FMT_VIOLATIONS=$((FMT_VIOLATIONS + 1))
    fi
  done <<< "$bare_formatter_output"
fi

# Check 5: debug_struct should use stringify! macro, not string literals
# Catches: debug_struct("TypeName") - should be debug_struct(stringify!(TypeName))
debug_struct_output=$(rg -n --no-heading 'debug_struct\("[A-Z]' crates --type rust 2> /dev/null || true)

if [[ -n "$debug_struct_output" ]]; then
  echo
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      line_content="${BASH_REMATCH[3]}"
      echo -e "${RED}Error:${NC} Use stringify! macro with debug_struct in $file:$line_num"
      echo "  ${line_content:0:100}"
      echo "  Use: debug_struct(stringify!(TypeName)) instead of debug_struct(\"TypeName\")"
      FMT_VIOLATIONS=$((FMT_VIOLATIONS + 1))
    fi
  done <<< "$debug_struct_output"
fi

if [ $FMT_VIOLATIONS -gt 0 ]; then
  echo
  echo -e "${RED}Found $FMT_VIOLATIONS std::fmt convention violation(s)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC}"
  echo "  - Import Debug/Display and use as: impl Debug for MyType, impl Display for MyType"
  echo "  - Do NOT import Formatter or Result, use fully qualified:"
  echo "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result"
  echo "  - Use stringify! with debug_struct: f.debug_struct(stringify!(MyType))"
else
  echo "std::fmt conventions are valid"
fi

# Check for ", got" phrasing in error messages
echo "Checking for ', got' phrasing..."

# Search for ", got" followed by space, punctuation, or end-of-line
got_output=$(rg -n ', got[[:space:][:punct:]]|, got$' \
  crates python examples \
  --type rust --type py \
  --glob '!docs/**' \
  2> /dev/null || true)

if [[ -n "$got_output" ]]; then
  GOT_VIOLATIONS=$(echo "$got_output" | wc -l | tr -d ' ')
  echo
  echo "$got_output" | head -20
  echo
  echo -e "${RED}Found $GOT_VIOLATIONS ', got' phrasing violation(s)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC} Use more descriptive alternatives:"
  echo "  - ', was' for type mismatches"
  echo "  - ', received' for input validation"
  echo "  - ', found' for search/lookup results"
  echo "  See: docs/developer_guide/coding_standards.md#terminology-and-phrasing"
else
  GOT_VIOLATIONS=0
  echo "No ', got' phrasing found"
fi

# Check for fully qualified macro calls
echo "Checking for fully qualified macro calls..."

MACRO_VIOLATIONS=0
macro_output=$(rg -n --no-heading \
  'nautilus_(common|trading)::(nautilus_actor|nautilus_strategy)!' \
  crates examples \
  --type rust \
  2> /dev/null || true)

if [[ -n "$macro_output" ]]; then
  # Filter out use statements
  while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
      line_content="${BASH_REMATCH[3]}"
      trimmed="${line_content#"${line_content%%[![:space:]]*}"}"

      if [[ "$trimmed" =~ ^use[[:space:]] ]] || [[ "$trimmed" =~ ^pub[[:space:]]+use[[:space:]] ]]; then
        continue
      fi

      file="${BASH_REMATCH[1]}"
      line_num="${BASH_REMATCH[2]}"
      echo -e "${RED}Error:${NC} Fully qualified macro call in $file:$line_num"
      echo "  ${trimmed:0:100}"
      echo "  Import the macro and call it unqualified: nautilus_actor!(...) or nautilus_strategy!(...)"
      echo
      MACRO_VIOLATIONS=$((MACRO_VIOLATIONS + 1))
    fi
  done <<< "$macro_output"

  if [ $MACRO_VIOLATIONS -gt 0 ]; then
    echo -e "${RED}Found $MACRO_VIOLATIONS fully qualified macro call(s)${NC}"
    echo
    echo -e "${YELLOW}To fix:${NC} Add 'use nautilus_common::nautilus_actor;' or"
    echo "'use nautilus_trading::nautilus_strategy;' and call the macro unqualified"
  fi
else
  echo "No fully qualified macro calls found"
fi

# Check that the exposed Python submodule set is unchanged
echo "Checking Python submodule registrations..."

# Every module the extension exposes to Python, as "<python name> <wrap_pymodule! target>" pairs.
# Each entry becomes an importable `nautilus_trader.<name>` package, so adding, removing, renaming,
# or repointing one changes the public API. Internal crates (nautilus-system and other engine
# plumbing) must not appear here: register a single class into an existing submodule instead, as
# nautilus-system does for `Controller` in the trading module.
EXPECTED_PYO3_MODULES="analysis nautilus_analysis::python::analysis
architect_ax nautilus_architect_ax::python::architect_ax
backtest nautilus_backtest::python::backtest
betfair nautilus_betfair::python::betfair
binance nautilus_binance::python::binance
bitmex nautilus_bitmex::python::bitmex
blockchain nautilus_blockchain::python::blockchain
bybit nautilus_bybit::python::bybit
coinbase nautilus_coinbase::python::coinbase
common nautilus_common::python::common
core nautilus_core::python::core
cryptography nautilus_cryptography::python::cryptography
data nautilus_data::python::data
databento nautilus_databento::python::databento
deribit nautilus_deribit::python::deribit
derive nautilus_derive::python::derive
dydx nautilus_dydx::python::dydx
execution nautilus_execution::python::execution
hyperliquid nautilus_hyperliquid::python::hyperliquid
indicators nautilus_indicators::python::indicators
infrastructure nautilus_infrastructure::python::infrastructure
interactive_brokers nautilus_interactive_brokers::python::interactive_brokers
kraken nautilus_kraken::python::kraken
lighter nautilus_lighter::python::lighter
live nautilus_live::python::live
model nautilus_model::python::model
network nautilus_network::python::network
okx nautilus_okx::python::okx
persistence nautilus_persistence::python::persistence
polymarket nautilus_polymarket::python::polymarket
portfolio nautilus_portfolio::python::portfolio
risk nautilus_risk::python::risk
sandbox nautilus_sandbox::python::sandbox
serialization nautilus_serialization::python::serialization
tardis nautilus_tardis::python::tardis
testkit nautilus_testkit::python::testkit
trading nautilus_trading::python::trading"

PYO3_LIB="crates/pyo3/src/lib.rs"
MODULE_VIOLATIONS=0

if [ -f "$PYO3_LIB" ]; then
  # Strip line comments so documentation mentions of the macro are not read as registrations, then
  # pair each `let n = "<name>"` with the `wrap_pymodule!` target that follows it
  actual_pairs=$(sed 's://.*::' "$PYO3_LIB" | awk '
    match($0, /let n = "[a-z_][a-z0-9_]*"/) {
      s = substr($0, RSTART, RLENGTH); gsub(/let n = "|"/, "", s); name = s; next
    }
    match($0, /wrap_pymodule!\([A-Za-z0-9_:]+\)/) {
      t = substr($0, RSTART, RLENGTH); gsub(/wrap_pymodule!\(|\)/, "", t)
      print name " " t; name = ""
    }
  ' | sort)

  expected_pairs=$(echo "$EXPECTED_PYO3_MODULES" | sed '/^$/d' | sort)

  added=$(comm -13 <(echo "$expected_pairs") <(echo "$actual_pairs") || true)
  removed=$(comm -23 <(echo "$expected_pairs") <(echo "$actual_pairs") || true)

  if [[ -n "$added" ]]; then
    while IFS= read -r pair; do
      [ -z "$pair" ] && continue
      echo -e "${RED}Error:${NC} Unexpected Python submodule registration in $PYO3_LIB: $pair"
      MODULE_VIOLATIONS=$((MODULE_VIOLATIONS + 1))
    done <<< "$added"
  fi
  if [[ -n "$removed" ]]; then
    while IFS= read -r pair; do
      [ -z "$pair" ] && continue
      echo -e "${RED}Error:${NC} Python submodule registration missing from $PYO3_LIB: $pair"
      MODULE_VIOLATIONS=$((MODULE_VIOLATIONS + 1))
    done <<< "$removed"
  fi

  # A registration must expose its target under that target's own name, so a pair swap between two
  # blocks fails even if the allowlist were regenerated from the swapped source
  while IFS= read -r pair; do
    [ -z "$pair" ] && continue
    pair_name=${pair%% *}
    pair_target=${pair##* }
    if [ "${pair_target##*::}" != "$pair_name" ]; then
      echo -e "${RED}Error:${NC} Python submodule '$pair_name' registers a mismatched target: $pair_target"
      MODULE_VIOLATIONS=$((MODULE_VIOLATIONS + 1))
    fi
  done <<< "$actual_pairs"

  # Anything that fails to parse must fail the check rather than silently shrink the compared set
  wrap_count=$(sed 's://.*::' "$PYO3_LIB" | rg -c 'wrap_pymodule!' 2> /dev/null || true)
  pair_count=$(echo "$actual_pairs" | sed '/^$/d' | wc -l | tr -d ' ')
  wrap_count=${wrap_count:-0}

  if [ "$wrap_count" -ne "$pair_count" ]; then
    echo -e "${RED}Error:${NC} Python submodule registrations in $PYO3_LIB did not parse cleanly"
    echo "  wrap_pymodule! calls: $wrap_count, parsed name/target pairs: $pair_count"
    echo "  Every registration must be one let n = \"<name>\" followed by one wrap_pymodule!(<path>)"
    MODULE_VIOLATIONS=$((MODULE_VIOLATIONS + 1))
  fi

  if [ $MODULE_VIOLATIONS -gt 0 ]; then
    echo
    echo -e "${RED}Found $MODULE_VIOLATIONS Python submodule registration issue(s)${NC}"
    echo
    echo -e "${YELLOW}To fix:${NC} Each submodule is public API surface, so this is a deliberate"
    echo "decision, not a refactor side effect. Do not expose an internal crate as a module;"
    echo "register its class into an existing submodule instead. If the change is intended,"
    echo "update EXPECTED_PYO3_MODULES in this script."
  else
    echo "Python submodule registrations are unchanged"
  fi
else
  echo "Python submodule registrations skipped ($PYO3_LIB not found)"
fi

# Exit with error if any violations found
if [ $VIOLATIONS -gt 0 ] || [ $BANNER_VIOLATIONS -gt 0 ] || [ $FMT_VIOLATIONS -gt 0 ] || [ "$GOT_VIOLATIONS" -gt 0 ] || [ $MACRO_VIOLATIONS -gt 0 ] || [ $MODULE_VIOLATIONS -gt 0 ]; then
  exit 1
fi

echo
echo "All Nautilus conventions are valid"
exit 0
