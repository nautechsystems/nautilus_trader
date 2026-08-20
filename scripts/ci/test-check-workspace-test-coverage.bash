#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for workspace test coverage script tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
CHECKER="$REPO_ROOT/scripts/ci/check-workspace-test-coverage.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

mkdir -p "$CASE_ROOT/bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  "if [ \"\$*\" != \"metadata --no-deps --format-version 1\" ]; then" \
  "  echo \"Unexpected cargo arguments: \$*\" >&2" \
  '  exit 2' \
  'fi' \
  '' \
  "cat \"\$MOCK_CARGO_METADATA\"" > "$CASE_ROOT/bin/cargo"
chmod +x "$CASE_ROOT/bin/cargo"

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

write_metadata() {
  local case_dir="$1"
  local mode="$2"
  local gamma_target_kind="lib"
  local gamma_target_name="gamma"

  if [ "$mode" = "test-target" ]; then
    gamma_target_kind="test"
    gamma_target_name="gamma-tests"
  fi

  if [ "$mode" = "nested" ]; then
    printf '%s\n' \
      '{' \
      '  "workspace_members": ["alpha", "beta", "gamma", "nested"],' \
      '  "packages": [' \
      "    {\"id\": \"alpha\", \"name\": \"alpha\", \"manifest_path\": \"$case_dir/crates/alpha/Cargo.toml\", \"targets\": [{\"name\": \"alpha\", \"kind\": [\"lib\"]}]}," \
      "    {\"id\": \"beta\", \"name\": \"beta\", \"manifest_path\": \"$case_dir/crates/beta/Cargo.toml\", \"targets\": [{\"name\": \"beta\", \"kind\": [\"lib\"]}]}," \
      "    {\"id\": \"gamma\", \"name\": \"gamma\", \"manifest_path\": \"$case_dir/crates/gamma/Cargo.toml\", \"targets\": [{\"name\": \"gamma\", \"kind\": [\"lib\"]}]}," \
      "    {\"id\": \"nested\", \"name\": \"nested\", \"manifest_path\": \"$case_dir/crates/gamma/nested/Cargo.toml\", \"targets\": [{\"name\": \"nested\", \"kind\": [\"lib\"]}]}" \
      '  ]' \
      '}' > "$case_dir/metadata.json"
  else
    printf '%s\n' \
      '{' \
      '  "workspace_members": ["alpha", "beta", "gamma"],' \
      '  "packages": [' \
      "    {\"id\": \"alpha\", \"name\": \"alpha\", \"manifest_path\": \"$case_dir/crates/alpha/Cargo.toml\", \"targets\": [{\"name\": \"alpha\", \"kind\": [\"lib\"]}]}," \
      "    {\"id\": \"beta\", \"name\": \"beta\", \"manifest_path\": \"$case_dir/crates/beta/Cargo.toml\", \"targets\": [{\"name\": \"beta\", \"kind\": [\"lib\"]}]}," \
      "    {\"id\": \"gamma\", \"name\": \"gamma\", \"manifest_path\": \"$case_dir/crates/gamma/Cargo.toml\", \"targets\": [{\"name\": \"$gamma_target_name\", \"kind\": [\"$gamma_target_kind\"]}]}" \
      '  ]' \
      '}' > "$case_dir/metadata.json"
  fi
}

create_case() {
  local name="$1"
  local mode="${2:-default}"
  local case_dir="$CASE_ROOT/$name"

  mkdir -p "$case_dir"
  case_dir=$(cd "$case_dir" && pwd -P)
  mkdir -p "$case_dir/scripts/ci" "$case_dir/crates"/{alpha,beta,gamma}/src
  cp "$CHECKER" "$case_dir/scripts/ci/check-workspace-test-coverage.sh"
  : > "$case_dir/crates/alpha/Cargo.toml"
  : > "$case_dir/crates/beta/Cargo.toml"
  : > "$case_dir/crates/gamma/Cargo.toml"
  write_makefile "$case_dir" "alpha" "beta" "gamma"
  write_metadata "$case_dir" "$mode"

  printf '%s' "$case_dir"
}

run_checker() {
  local case_dir="$1"

  set +e
  PATH="$CASE_ROOT/bin:$PATH" \
    MOCK_CARGO_METADATA="$case_dir/metadata.json" \
    bash "$case_dir/scripts/ci/check-workspace-test-coverage.sh" \
    > "$case_dir/stdout.txt" 2> "$case_dir/stderr.txt"
  RUN_STATUS=$?
  set -e
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

test_target_case=$(create_case "test-target" "test-target")
expect_failure "$test_target_case" "no-test crate gamma defines test targets: gamma-tests"

rust_test_case=$(create_case "rust-test")
printf '%s\n' \
  '#[test]' \
  'fn rejects_no_test_inventory() {}' > "$rust_test_case/crates/gamma/src/lib.rs"
expect_failure "$rust_test_case" "no-test crate gamma contains Rust tests: crates/gamma/src/lib.rs"

nested_case=$(create_case "nested-and-cache" "nested")
mkdir -p "$nested_case/crates/gamma/nested/src" "$nested_case/crates/gamma/cache/src"
: > "$nested_case/crates/gamma/nested/Cargo.toml"
: > "$nested_case/crates/gamma/cache/CACHEDIR.TAG"
printf '%s\n' '#[test]' 'fn nested_test() {}' > "$nested_case/crates/gamma/nested/src/lib.rs"
printf '%s\n' '#[test]' 'fn cached_test() {}' > "$nested_case/crates/gamma/cache/src/lib.rs"
write_makefile "$nested_case" "alpha nested" "beta" "gamma"
expect_success "$nested_case" 4

echo "Workspace test coverage script tests passed"
