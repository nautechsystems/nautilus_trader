#!/usr/bin/env bash
# Enforces logging conventions:
# 1. Logging macros (trace, debug, info, warn, error) must be fully qualified
#    Use log::debug!(...) or tracing::info!(...) instead of importing the macros
#    Other imports from log/tracing crates are allowed (Level, LevelFilter, etc.)
#    Handles both single-line and multi-line use statements (e.g., rustfmt-wrapped imports)
#    Handles all visibility modifiers: pub, pub(crate), pub(super), pub(self), pub(in path)
# 2. Log messages must not end with a terminating period
#    Use '// log-period-ok' comment on or within 3 lines above to allow exceptions
# 3. Production library code must not write directly to stdout or stderr.
#    Build scripts, bins, examples, benches, tests, CLI/adapters, and testkit code are out of scope.

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

echo "Checking for direct stdout/stderr macros in production library code..."

OUTPUT_VIOLATIONS=0

PRODUCTION_OUTPUT_CRATES=(
  "analysis" "backtest" "common" "core" "cryptography" "data" "execution"
  "indicators" "infrastructure" "live" "model" "network" "persistence"
  "portfolio" "risk" "serialization" "system" "trading"
)

OUTPUT_GLOBS=()
for c in "${PRODUCTION_OUTPUT_CRATES[@]}"; do
  OUTPUT_GLOBS+=(--glob "crates/$c/src/**/*.rs")
done

normalize_path() {
  printf '%s' "${1//\\//}"
}

is_direct_output_test_path() {
  local file
  file=$(normalize_path "$1")
  [[ "$file" =~ /examples/ ]] && return 0
  [[ "$file" =~ /tests/ ]] && return 0
  [[ "$file" =~ tests\.rs$ ]] && return 0
  [[ "$file" =~ _test\.rs$ ]] && return 0
  [[ "$file" =~ _tests\.rs$ ]] && return 0
  [[ "$file" =~ /test_.*\.rs$ ]] && return 0
  return 1
}

is_comment_line() {
  local content="$1"
  [[ "$content" =~ ^[[:space:]]*// ]] && return 0
  [[ "$content" =~ ^[[:space:]]*/\* ]] && return 0
  [[ "$content" =~ ^[[:space:]]*\* ]] && return 0
  return 1
}

has_test_cfg_for_module() {
  local file="$1"
  local mod_line="$2"

  awk -v target="$mod_line" '
    NR < target {
      lines[NR] = $0
      next
    }
    NR == target {
      for (i = target - 1; i >= 1; i--) {
        line = lines[i]
        if (line ~ /^[[:space:]]*$/ || line ~ /^[[:space:]]*\/\//) {
          continue
        }
        if (line ~ /^[[:space:]]*#\[/) {
          if (line ~ /#\[cfg\([^]]*not[[:space:]]*\([[:space:]]*test[[:space:]]*\)/) {
            blocked = 1
          }
          if (line ~ /#\[cfg\([^]]*test[^]]*\)\]/) {
            found = 1
          }
          continue
        }
        break
      }
      exit found && !blocked ? 0 : 1
    }
  ' "$file"
}

line_is_inside_module() {
  local file="$1"
  local mod_line="$2"
  local line_num="$3"

  awk -v start="$mod_line" -v target="$line_num" '
    function update_depth(s,    i, c, pair) {
      for (i = 1; i <= length(s); i++) {
        c = substr(s, i, 1)
        pair = substr(s, i, 2)

        if (in_block_comment) {
          if (pair == "*/") {
            in_block_comment = 0
            i++
          }
          continue
        }
        if (in_string) {
          if (escaped) {
            escaped = 0
          } else if (c == "\\") {
            escaped = 1
          } else if (c == "\"") {
            in_string = 0
          }
          continue
        }
        if (in_char) {
          if (escaped) {
            escaped = 0
          } else if (c == "\\") {
            escaped = 1
          } else if (c == sprintf("%c", 39)) {
            in_char = 0
          }
          continue
        }

        if (pair == "//") {
          break
        }
        if (pair == "/*") {
          in_block_comment = 1
          i++
          continue
        }
        if (c == "\"") {
          in_string = 1
          continue
        }
        if (c == sprintf("%c", 39)) {
          in_char = 1
          continue
        }
        if (c == "{") {
          depth++
        } else if (c == "}") {
          depth--
        }
      }
    }

    NR < start { next }
    NR > target { exit }
    {
      update_depth($0)
      if (NR == target && depth > 0) {
        found = 1
        exit
      }
      if (NR > start && depth <= 0) {
        exit
      }
    }
    END { exit found ? 0 : 1 }
  ' "$file"
}

line_has_direct_output_macro() {
  local content="$1"

  awk -v line="$content" '
    function is_ident_char(c) {
      return c ~ /[[:alnum:]_]/
    }
    BEGIN {
      for (i = 1; i <= length(line); i++) {
        c = substr(line, i, 1)
        pair = substr(line, i, 2)

        if (in_string) {
          if (escaped) {
            escaped = 0
          } else if (c == "\\") {
            escaped = 1
          } else if (c == "\"") {
            in_string = 0
          }
          continue
        }
        if (in_char) {
          if (escaped) {
            escaped = 0
          } else if (c == "\\") {
            escaped = 1
          } else if (c == sprintf("%c", 39)) {
            in_char = 0
          }
          continue
        }

        if (pair == "//") {
          exit 1
        }
        if (c == "\"") {
          in_string = 1
          continue
        }
        if (c == sprintf("%c", 39)) {
          in_char = 1
          continue
        }

        rest = substr(line, i)
        if (rest ~ /^(e?println|e?print)![[:space:]]*\(/) {
          previous = i == 1 ? "" : substr(line, i - 1, 1)
          exit is_ident_char(previous) ? 1 : 0
        }
      }
      exit 1
    }
  '
}

is_in_inline_test_module() {
  local file="$1"
  local line_num="$2"
  local mod_line

  while IFS=: read -r mod_line _; do
    [[ -z "$mod_line" ]] && continue
    ((line_num < mod_line)) && continue
    has_test_cfg_for_module "$file" "$mod_line" || continue
    line_is_inside_module "$file" "$mod_line" "$line_num" && return 0
  done < <(rg -n '^\s*(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+[[:alnum:]_]+[[:space:]]*\{' "$file" 2> /dev/null || true)
  return 1
}

is_allowed_direct_output() {
  local file
  file=$(normalize_path "$1")
  local content="$2"

  case "$file" in
    crates/common/src/logging/logger.rs | crates/common/src/logging/writer.rs)
      return 0
      ;;
    crates/model/src/identifiers/mod.rs)
      [[ "$content" == *'println!("{s}")'* ]] && return 0
      ;;
  esac

  return 1
}

direct_output=$(rg -n --no-heading \
  '\b(e?println|e?print)!\s*\(' \
  "${OUTPUT_GLOBS[@]}" --type rust 2> /dev/null || true)

if [[ -n "$direct_output" ]]; then
  while IFS=: read -r file line_num content; do
    [[ -z "$file" ]] && continue

    norm_file=$(normalize_path "$file")
    is_direct_output_test_path "$norm_file" && continue
    is_in_inline_test_module "$file" "$line_num" && continue
    is_comment_line "$content" && continue
    line_has_direct_output_macro "$content" || continue
    is_allowed_direct_output "$norm_file" "$content" && continue

    trimmed="${content#"${content%%[![:space:]]*}"}"
    echo -e "${RED}Error:${NC} Direct stdout/stderr macro in $norm_file:$line_num"
    echo "  Found: ${trimmed:0:100}"
    echo "  Use log:: or tracing:: macros in production library code."
    echo
    OUTPUT_VIOLATIONS=$((OUTPUT_VIOLATIONS + 1))
  done <<< "$direct_output"
fi

if [ $OUTPUT_VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $OUTPUT_VIOLATIONS direct stdout/stderr macro violation(s)${NC}"
  echo
else
  echo "✓ No direct stdout/stderr macros in production library code"
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
if [ $VIOLATIONS -gt 0 ] || [ $OUTPUT_VIOLATIONS -gt 0 ] || [ $PERIOD_VIOLATIONS -gt 0 ]; then
  exit 1
fi

echo
echo "✓ All logging conventions are valid"
exit 0
