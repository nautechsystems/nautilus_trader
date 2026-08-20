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

echo "DST convention hook tests passed"
