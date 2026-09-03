#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for DST convention hook tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_dst_conventions.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

create_case() {
  local case_dir="$1"
  local crate

  for crate in \
    analysis backtest common core cryptography data execution indicators live \
    model network persistence portfolio risk serialization system trading; do
    mkdir -p "$case_dir/crates/$crate/src"
  done

  mkdir -p \
    "$case_dir/crates/live/src/execution" \
    "$case_dir/crates/execution/src/matching_engine"
  : > "$case_dir/crates/live/src/execution/manager.rs"
  : > "$case_dir/crates/execution/src/matching_engine/mod.rs"
}

run_hook() {
  local case_dir="$1"

  set +e
  (cd "$case_dir" && bash "$HOOK") > "$case_dir/output.txt" 2>&1
  RUN_STATUS=$?
  set -e
}

# Reports carry ANSI color between the rule and the path, so compare plain text
strip_color() {
  local esc
  esc=$(printf '\033')
  sed "s/${esc}\[[0-9;]*m//g" "$1"
}

valid_case="$CASE_ROOT/valid"
create_case "$valid_case"
printf '%s\n' 'pub fn deterministic() {}' > "$valid_case/crates/common/src/lib.rs"
run_hook "$valid_case"
if [ "$RUN_STATUS" -ne 0 ]; then
  echo "Expected DST convention hook to pass valid input"
  cat "$valid_case/output.txt"
  exit 1
fi
rg -Fq "All DST conventions are valid" "$valid_case/output.txt"

invalid_case="$CASE_ROOT/invalid"
create_case "$invalid_case"
printf '%s\n' \
  'pub async fn nondeterministic() {' \
  '    let _now = std::time::Instant::now();' \
  '    let _rng = rand::rng();' \
  '    tokio::select! {' \
  '        _ = ready() => {}' \
  '    }' \
  '    std::thread::spawn(run);' \
  '    let _stream = tokio::net::TcpStream::connect("localhost:1").await;' \
  '    tokio::time::sleep(delay).await;' \
  '}' > "$invalid_case/crates/common/src/lib.rs"
printf '%s\n' \
  'use ahash::AHashMap;' \
  'pub type Orders = AHashMap<u64, u64>;' > "$invalid_case/crates/live/src/execution/manager.rs"
run_hook "$invalid_case"
if [ "$RUN_STATUS" -ne 1 ]; then
  echo "Expected DST convention hook to reject violations"
  cat "$invalid_case/output.txt"
  exit 1
fi

for rule in 1 2 3 4 5 6 7; do
  if ! rg -Fq "Error (rule$rule):" "$invalid_case/output.txt"; then
    echo "Expected a rule$rule violation"
    cat "$invalid_case/output.txt"
    exit 1
  fi
done

# Rule 5 searches one explicit file, where ripgrep omits the path, so assert
# the reported location rather than detection alone.
strip_color "$invalid_case/output.txt" > "$invalid_case/plain.txt"
for expected in \
  "Error (rule5): crates/live/src/execution/manager.rs:1" \
  "Error (rule5): crates/live/src/execution/manager.rs:2"; do
  if ! rg -Fq "$expected" "$invalid_case/plain.txt"; then
    echo "Expected a violation reported as: $expected"
    cat "$invalid_case/output.txt"
    exit 1
  fi
done

# Bare clock reads are resolved from per-file import facts and the inline
# `#[cfg(test)]` boundary rather than from the matched line, so cover both
# sides of each decision and two hits in one file.
bare_case="$CASE_ROOT/bare"
create_case "$bare_case"
printf '%s\n' \
  'use std::time::Instant;' \
  'pub fn before_tests() { let _ = Instant::now(); }' \
  '#[cfg(test)]' \
  'mod tests {' \
  '    pub fn after_tests() { let _ = Instant::now(); }' \
  '}' > "$bare_case/crates/core/src/lib.rs"
printf '%s\n' \
  'use std::time::SystemTime;' \
  'pub fn first() { let _ = SystemTime::now(); }' \
  'pub fn second() { let _ = SystemTime::now(); }' > "$bare_case/crates/model/src/lib.rs"
printf '%s\n' \
  'use tokio::time::Instant;' \
  'pub fn tokio_clock() { let _ = Instant::now(); }' > "$bare_case/crates/data/src/lib.rs"
run_hook "$bare_case"
if [ "$RUN_STATUS" -ne 1 ]; then
  echo "Expected DST convention hook to reject bare clock reads"
  cat "$bare_case/output.txt"
  exit 1
fi

strip_color "$bare_case/output.txt" > "$bare_case/plain.txt"

for expected in \
  "Error (rule1): crates/core/src/lib.rs:2" \
  "Error (rule1): crates/model/src/lib.rs:2" \
  "Error (rule1): crates/model/src/lib.rs:3"; do
  if ! rg -Fq "$expected" "$bare_case/plain.txt"; then
    echo "Expected a violation reported as: $expected"
    cat "$bare_case/output.txt"
    exit 1
  fi
done

for unexpected in \
  "Error (rule1): crates/core/src/lib.rs:5" \
  "Error (rule1): crates/data/src/lib.rs"; do
  if rg -Fq "$unexpected" "$bare_case/plain.txt"; then
    echo "Did not expect a violation reported as: $unexpected"
    cat "$bare_case/output.txt"
    exit 1
  fi
done

# Renamed clock imports reach rule 1 through a ripgrep file filter that has to
# stay a superset of the alias extraction it feeds. No file in the tree carries
# such an import, so this case is the only thing holding the two regexes together.
alias_case="$CASE_ROOT/jiff-alias"
create_case "$alias_case"
printf '%s\n' \
  'use jiff::{Timestamp as Ts};' \
  'pub fn renamed() { let _ = Ts::now(); }' > "$alias_case/crates/core/src/lib.rs"
run_hook "$alias_case"
if [ "$RUN_STATUS" -ne 1 ]; then
  echo "Expected DST convention hook to reject a renamed jiff clock read"
  cat "$alias_case/output.txt"
  exit 1
fi

# The alias scan also searches one explicit file, so assert the reported
# location and content rather than detection alone.
strip_color "$alias_case/output.txt" > "$alias_case/plain.txt"
for expected in \
  "Error (rule1): crates/core/src/lib.rs:2" \
  "Found: pub fn renamed() { let _ = Ts::now(); }" \
  "Found 1 DST convention violation(s)"; do
  if ! rg -Fq "$expected" "$alias_case/plain.txt"; then
    echo "Expected the renamed jiff clock read reported as: $expected"
    cat "$alias_case/output.txt"
    exit 1
  fi
done

echo "DST convention hook tests passed"
