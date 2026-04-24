#!/usr/bin/env bash
# Enforces deterministic simulation testing (DST) path bans in the in-scope crates.
#
# Rules (all applied to production code in the 16 in-scope crates):
#   1. No direct std::time::Instant::now() or std::time::SystemTime::now() reads
#   2. No raw RNG imports (rand::thread_rng, fastrand::, getrandom::, OsRng)
#   3. No unbiased tokio::select! (must have `biased;` as first token in block)
#   4. No raw thread spawning (std::thread::spawn, std::thread::Builder::spawn,
#      tokio::task::spawn_blocking) without cfg gating
#   5. No AHashMap / AHashSet in crates/live/src/manager.rs or
#      crates/execution/src/matching_engine/engine.rs
#
# Use '// dst-ok' inline comment to allow specific exceptions.
# Test modules (files under tests/, matching *_tests.rs, or lines inside an
# inline `#[cfg(test)]` module) are excluded.

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping DST convention checks"
  exit 0
fi

RED='\033[0;31m'
NC='\033[0m'

VIOLATIONS=0
ALLOW_MARKER="dst-ok"

# The 16 in-scope crates per phase3 upstream closure plan.
IN_SCOPE_CRATES=(
  "analysis" "common" "core" "cryptography" "data" "execution"
  "indicators" "live" "model" "network" "persistence" "portfolio"
  "risk" "serialization" "system" "trading"
)

# Rule-1 L-dispositioned sites from the codebase audit: log timing, progress
# reporting, and audit-only uses that do not affect DST-path state.
RULE1_ALLOWLIST=(
  "crates/common/src/cache/mod.rs"
  "crates/model/src/defi/reporting.rs"
)

# Build ripgrep --glob patterns for in-scope crates.
GLOBS=()
for c in "${IN_SCOPE_CRATES[@]}"; do
  GLOBS+=(--glob "crates/$c/src/**/*.rs")
done

# Normalize Windows backslash paths to POSIX so path matching works under
# Git Bash / MSYS2. Callers must pass results through this before matching.
normalize_path() {
  printf '%s' "${1//\\//}"
}

# Skip test infrastructure and non-DST-path bindings within in-scope crates.
# Python bindings and FFI live behind their own feature gates and are not part
# of the simulation path, so DST bans do not apply there.
is_test_path() {
  local file
  file=$(normalize_path "$1")
  [[ "$file" =~ /tests/ ]] && return 0
  [[ "$file" =~ _test\.rs$ ]] && return 0
  [[ "$file" =~ _tests\.rs$ ]] && return 0
  [[ "$file" =~ /python/ ]] && return 0
  [[ "$file" =~ /ffi/ ]] && return 0
  return 1
}

# Skip rustdoc example lines like `/// let x = std::time::Instant::now();`
is_doc_comment() {
  local content="$1"
  [[ "$content" =~ ^[[:space:]]*/// ]] && return 0
  [[ "$content" =~ ^[[:space:]]*//! ]] && return 0
  return 1
}

# Return 0 if the given line number falls after an inline `#[cfg(test)]`
# attribute in the same file. Inline test modules live at the bottom of many
# Rust source files; violations beyond that boundary are test-only.
is_in_test_module() {
  local file="$1"
  local line_num="$2"

  local cfg_test_line
  cfg_test_line=$(rg -n '^\s*#\[cfg\(test\)\]' "$file" 2> /dev/null |
    head -1 | cut -d: -f1)

  [[ -z "$cfg_test_line" ]] && return 1
  [[ "$line_num" -ge "$cfg_test_line" ]] && return 0
  return 1
}

is_in_rule1_allowlist() {
  local file
  file=$(normalize_path "$1")
  local entry
  for entry in "${RULE1_ALLOWLIST[@]}"; do
    [[ "$file" == "$entry" ]] && return 0
  done
  return 1
}

# Detect whether a file imports Instant / SystemTime from std::time. Bare
# `Instant::now()` / `SystemTime::now()` calls are only flagged when the
# enclosing file actually pulls the type in from std::time. Covers every
# in-repo shape:
#   - `use std::time::Instant;`
#   - `use std::time::{Duration, Instant};`
#   - `use std::{time::Instant, ...};`                    (sibling brace)
#   - `use std::{thread, time::{Duration, Instant}};`     (nested brace)
#   - `use std::{..., time::SystemTime, ...};`            (sibling brace)
# The multi-line flag (-U) handles use statements that wrap onto multiple
# lines, which several in-scope crates do.
file_imports_std_instant() {
  rg -qU 'use\s+std::[^;]*\btime::(Instant\b|\{[^}]*\bInstant\b)' \
    "$1" 2> /dev/null
}

file_imports_std_system_time() {
  rg -qU 'use\s+std::[^;]*\btime::(SystemTime\b|\{[^}]*\bSystemTime\b)' \
    "$1" 2> /dev/null
}

# Return 0 if any of the 15 lines preceding `line_num` in `file` carry a cfg
# attribute that excludes madsim or restricts to test builds.
has_preceding_dst_cfg() {
  local file="$1"
  local line_num="$2"
  local start_line=$((line_num - 15))
  ((start_line < 1)) && start_line=1

  sed -n "${start_line},$((line_num - 1))p" "$file" 2> /dev/null |
    grep -qE '#\[cfg\(not\(all\(feature[[:space:]]*=[[:space:]]*"simulation"[[:space:]]*,[[:space:]]*madsim\)\)\)\]|#\[cfg\(test\)\]|#\[cfg\(not\(madsim\)\)\]'
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

################################################################################
# Rule 1: direct std::time clock reads
################################################################################

echo "Checking direct std::time clock reads..."

check_rule1_hit() {
  local file="$1"
  local line_num="$2"
  local content="$3"

  [[ -z "$file" ]] && return
  local norm_file
  norm_file=$(normalize_path "$file")
  is_test_path "$norm_file" && return
  is_in_test_module "$file" "$line_num" && return
  is_doc_comment "$content" && return
  [[ "$content" =~ $ALLOW_MARKER ]] && return
  is_in_rule1_allowlist "$norm_file" && return

  # Allowlist: the wall-clock seam definition site in core::time.
  if [[ "$norm_file" == "crates/core/src/time.rs" ]] &&
    [[ "$content" =~ SystemTime::now ]]; then
    return
  fi

  report "rule1" "$norm_file" "$line_num" "$content" \
    "Route through nautilus_core::time::duration_since_unix_epoch or a DST seam"
}

# Fully-qualified reads are caught everywhere.
while IFS=: read -r file line_num content; do
  check_rule1_hit "$file" "$line_num" "$content"
done < <(rg -n --no-heading \
  'std::time::Instant::now\(\)|std::time::SystemTime::now\(\)' \
  "${GLOBS[@]}" --type rust 2> /dev/null || true)

# Bare `Instant::now()` counts only when the file imports std::time::Instant.
while IFS=: read -r file line_num content; do
  [[ -z "$file" ]] && continue
  file_imports_std_instant "$file" || continue
  [[ "$content" =~ (tokio|madsim|dst)::time::Instant ]] && continue
  check_rule1_hit "$file" "$line_num" "$content"
done < <(rg -n --no-heading \
  '\bInstant::now\(\)' \
  "${GLOBS[@]}" --type rust 2> /dev/null || true)

# Bare `SystemTime::now()` counts only when the file imports std::time::SystemTime.
while IFS=: read -r file line_num content; do
  [[ -z "$file" ]] && continue
  file_imports_std_system_time "$file" || continue
  [[ "$content" =~ madsim::time::SystemTime ]] && continue
  check_rule1_hit "$file" "$line_num" "$content"
done < <(rg -n --no-heading \
  '\bSystemTime::now\(\)' \
  "${GLOBS[@]}" --type rust 2> /dev/null || true)

################################################################################
# Rule 2: raw RNG imports
################################################################################

echo "Checking raw RNG usage..."

while IFS=: read -r file line_num content; do
  [[ -z "$file" ]] && continue
  is_test_path "$file" && continue
  is_in_test_module "$file" "$line_num" && continue
  is_doc_comment "$content" && continue
  [[ "$content" =~ $ALLOW_MARKER ]] && continue

  report "rule2" "$file" "$line_num" "$content" \
    "Route RNG through a seeded source; madsim::rand under cfg(madsim)"
done < <(rg -n --no-heading \
  'rand::thread_rng|fastrand::|getrandom::|\bOsRng\b' \
  "${GLOBS[@]}" --type rust 2> /dev/null || true)

################################################################################
# Rule 3: unbiased tokio::select!
################################################################################

echo "Checking tokio::select! biased; discipline..."

while IFS=: read -r file line_num content; do
  [[ -z "$file" ]] && continue
  is_test_path "$file" && continue
  is_in_test_module "$file" "$line_num" && continue
  is_doc_comment "$content" && continue
  [[ "$content" =~ $ALLOW_MARKER ]] && continue

  # Check the three lines after the select! opening for `biased;`.
  next_window=$(sed -n "$((line_num + 1)),$((line_num + 3))p" "$file" 2> /dev/null)
  if echo "$next_window" | grep -q 'biased;'; then
    continue
  fi

  report "rule3" "$file" "$line_num" "$content" \
    "Add 'biased;' as the first token inside the select! block"
done < <(rg -n --no-heading \
  'tokio::select!\s*\{' \
  "${GLOBS[@]}" --type rust 2> /dev/null || true)

################################################################################
# Rule 4: raw thread spawning outside cfg(test) and cfg(not(madsim))
################################################################################

echo "Checking raw thread spawning..."

while IFS=: read -r file line_num content; do
  [[ -z "$file" ]] && continue
  is_test_path "$file" && continue
  is_in_test_module "$file" "$line_num" && continue
  is_doc_comment "$content" && continue
  [[ "$content" =~ $ALLOW_MARKER ]] && continue

  has_preceding_dst_cfg "$file" "$line_num" && continue

  report "rule4" "$file" "$line_num" "$content" \
    "Wrap the spawn in #[cfg(not(all(feature = \"simulation\", madsim)))] or add '// dst-ok'"
done < <(rg -n --no-heading \
  'std::thread::spawn\b|std::thread::Builder::new\(\)|tokio::task::spawn_blocking' \
  "${GLOBS[@]}" --type rust 2> /dev/null || true)

################################################################################
# Rule 5: AHashMap / AHashSet in reconciliation manager and matching engine
################################################################################

RULE5_FILES=(
  "crates/live/src/manager.rs"
  "crates/execution/src/matching_engine/engine.rs"
)

for rule5_file in "${RULE5_FILES[@]}"; do
  echo "Checking AHashMap / AHashSet in $rule5_file..."
  [[ -f "$rule5_file" ]] || continue

  while IFS=: read -r file line_num content; do
    [[ -z "$file" ]] && continue
    is_doc_comment "$content" && continue
    [[ "$content" =~ $ALLOW_MARKER ]] && continue

    report "rule5" "$file" "$line_num" "$content" \
      "Use IndexMap / IndexSet for deterministic iteration order"
  done < <(rg -n --no-heading '\bAHash(Map|Set)\b' "$rule5_file" 2> /dev/null || true)
done

################################################################################
# Summary
################################################################################

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS DST convention violation(s)${NC}"
  echo
  echo "Add '// dst-ok' inline comment to allow specific exceptions"
  exit 1
fi

echo "✓ All DST conventions are valid"
exit 0
