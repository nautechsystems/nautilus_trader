#!/usr/bin/env bash

set -euo pipefail

for command in cargo rg; do
  if ! command -v "$command" &> /dev/null; then
    echo "ERROR: $command is required for workspace test coverage script tests" >&2
    exit 1
  fi
done

REPO_ROOT=$(git rev-parse --show-toplevel)
CHECKER="$REPO_ROOT/scripts/ci/check-workspace-test-coverage.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

write_makefile() {
  local case_dir="$1"
  local core_crates="$2"
  local adapter_crates="$3"
  local no_test_crates="$4"

  printf '%s\n' \
    "CORE_CRATES := $core_crates" \
    "ADAPTER_CRATES := $adapter_crates" \
    "NO_TEST_CRATES := $no_test_crates" > "$case_dir/Makefile"
}

create_case() {
  local case_dir="$CASE_ROOT/$1"
  shift

  local members=(crates/alpha crates/beta crates/gamma "$@")
  local member

  mkdir -p "$case_dir/scripts/ci"
  cp "$CHECKER" "$case_dir/scripts/ci/"
  cp "$REPO_ROOT/rust-toolchain.toml" "$case_dir/"

  {
    printf '%s\n' '[workspace]' 'resolver = "3"' 'members = ['
    printf '  "%s",\n' "${members[@]}"
    printf '%s\n' ']'
  } > "$case_dir/Cargo.toml"

  for member in "${members[@]}"; do
    mkdir -p "$case_dir/$member/src"
    printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2024"\n' \
      "${member##*/}" > "$case_dir/$member/Cargo.toml"
    : > "$case_dir/$member/src/lib.rs"
  done

  write_makefile "$case_dir" "alpha" "beta" "gamma"
  printf '%s\n' "$case_dir"
}

run_checker() {
  local case_dir="$1"

  RUN_STATUS=0
  CARGO_NET_OFFLINE=true bash "$case_dir/scripts/ci/check-workspace-test-coverage.sh" \
    > "$case_dir/stdout.txt" 2> "$case_dir/stderr.txt" || RUN_STATUS=$?
}

expect_success() {
  local case_dir="$1"
  local member_count="$2"
  local output

  run_checker "$case_dir"
  if [ "$RUN_STATUS" -ne 0 ]; then
    echo "Expected workspace coverage check to pass in $case_dir"
    cat "$case_dir/stdout.txt" "$case_dir/stderr.txt"
    exit 1
  fi

  output=$(cat "$case_dir/stdout.txt")
  if [ "$output" != "Workspace test coverage is complete ($member_count workspace members)" ]; then
    echo "Unexpected success output in $case_dir: $output"
    exit 1
  fi
}

expect_failure() {
  local case_dir="$1"
  local reason="$2"

  run_checker "$case_dir"
  if [ "$RUN_STATUS" -ne 1 ]; then
    echo "Expected workspace coverage check to fail with status 1 in $case_dir"
    cat "$case_dir/stdout.txt" "$case_dir/stderr.txt"
    exit 1
  fi

  if ! rg -Fq -- "$reason" "$case_dir/stderr.txt"; then
    echo "Expected failure reason not found: $reason"
    cat "$case_dir/stderr.txt"
    exit 1
  fi
}

valid_case=$(create_case "valid")
expect_success "$valid_case" 3

duplicate_case=$(create_case "duplicate")
write_makefile "$duplicate_case" "alpha" "beta alpha" "gamma"
expect_failure "$duplicate_case" "workspace crates listed more than once: alpha"

missing_case=$(create_case "missing")
write_makefile "$missing_case" "alpha" "beta" ""
expect_failure "$missing_case" "workspace crates missing from inventories: gamma"

unknown_case=$(create_case "unknown")
write_makefile "$unknown_case" "alpha unknown" "beta" "gamma"
expect_failure "$unknown_case" "inventory entries not found in the workspace: unknown"

test_target_case=$(create_case "test-target")
mkdir -p "$test_target_case/crates/gamma/tests"
: > "$test_target_case/crates/gamma/tests/gamma-tests.rs"
expect_failure "$test_target_case" "no-test crate gamma defines test targets: gamma-tests"

rust_test_case=$(create_case "rust-test")
printf '%s\n' \
  '#[test]' \
  'fn rejects_no_test_inventory() {}' > "$rust_test_case/crates/gamma/src/lib.rs"
expect_failure "$rust_test_case" "no-test crate gamma contains Rust tests: crates/gamma/src/lib.rs"

nested_case=$(create_case "nested-and-cache" "crates/gamma/nested")
mkdir -p "$nested_case/crates/gamma/cache/src"
: > "$nested_case/crates/gamma/cache/CACHEDIR.TAG"
printf '%s\n' '#[test]' 'fn nested_test() {}' > "$nested_case/crates/gamma/nested/src/lib.rs"
printf '%s\n' '#[test]' 'fn cached_test() {}' > "$nested_case/crates/gamma/cache/src/lib.rs"
write_makefile "$nested_case" "alpha nested" "beta" "gamma"
expect_success "$nested_case" 4

echo "Workspace test coverage script tests passed"
