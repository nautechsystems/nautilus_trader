#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping logging convention hook tests"
  exit 0
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_logging_conventions.sh"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

write_rs() {
  local path="$1"
  shift

  mkdir -p "$(dirname "$path")"
  printf '%s\n' "$@" > "$path"
}

run_hook() {
  local case_dir="$1"
  local output="$case_dir/output.txt"

  (cd "$case_dir" && bash "$HOOK") > "$output" 2>&1
}

expect_failure() {
  local case_dir="$1"
  local pattern="$2"

  if run_hook "$case_dir"; then
    echo "Expected logging convention hook to fail in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi

  rg -q "$pattern" "$case_dir/output.txt"
}

expect_success() {
  local case_dir="$1"

  if ! run_hook "$case_dir"; then
    echo "Expected logging convention hook to pass in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi
}

reject_direct_case="$TMP_DIR/reject-direct-production-output"
write_rs "$reject_direct_case/crates/common/src/lib.rs" \
  'pub fn direct_output() {' \
  '    println!("leak");' \
  '}'
expect_failure "$reject_direct_case" "Direct stdout/stderr macro"

reject_after_string_case="$TMP_DIR/reject-output-after-string"
write_rs "$reject_after_string_case/crates/common/src/lib.rs" \
  'pub fn output_after_string_match(value: &str) {' \
  '    match value {' \
  '        "quit" => eprintln!("bye"),' \
  '        _ => {}' \
  '    }' \
  '}'
expect_failure "$reject_after_string_case" "Direct stdout/stderr macro"

reject_not_test_case="$TMP_DIR/reject-not-test-output"
write_rs "$reject_not_test_case/crates/common/src/lib.rs" \
  '#[cfg(not(test))]' \
  'pub fn output_under_not_test() {' \
  '    print!("still production");' \
  '}'
expect_failure "$reject_not_test_case" "Direct stdout/stderr macro"

reject_direct_exit_case="$TMP_DIR/reject-direct-process-exit"
write_rs "$reject_direct_exit_case/crates/common/src/lib.rs" \
  'pub fn terminate() {' \
  '    std::process::exit(1);' \
  '}'
expect_failure "$reject_direct_exit_case" "Process exit in production library code"

reject_process_exit_case="$TMP_DIR/reject-process-module-exit"
write_rs "$reject_process_exit_case/crates/common/src/lib.rs" \
  'use std::process;' \
  '' \
  'pub fn terminate() {' \
  '    process::exit(1);' \
  '}'
expect_failure "$reject_process_exit_case" "Process exit in production library code"

reject_imported_exit_case="$TMP_DIR/reject-imported-process-exit"
write_rs "$reject_imported_exit_case/crates/common/src/lib.rs" \
  'use std::process::{' \
  '    Command,' \
  '    exit,' \
  '};' \
  '' \
  'pub fn terminate() {' \
  '    exit(1);' \
  '}'
expect_failure "$reject_imported_exit_case" "Process exit in production library code"

reject_single_imported_exit_case="$TMP_DIR/reject-single-imported-process-exit"
write_rs "$reject_single_imported_exit_case/crates/common/src/lib.rs" \
  'use std::process::exit;' \
  '' \
  'pub fn terminate() {' \
  '    exit(1);' \
  '}'
expect_failure "$reject_single_imported_exit_case" "Process exit in production library code"

allow_method_exit_case="$TMP_DIR/allow-imported-method-exit"
write_rs "$allow_method_exit_case/crates/common/src/lib.rs" \
  'use std::process::exit;' \
  '' \
  'struct Handle;' \
  '' \
  'impl Handle {' \
  '    fn exit(&self) {}' \
  '}' \
  '' \
  'pub fn close(handle: Handle) {' \
  '    handle.exit();' \
  '}'
expect_success "$allow_method_exit_case"

allow_case="$TMP_DIR/allow-intentional-output"
write_rs "$allow_case/crates/common/src/lib.rs" \
  'pub fn literal_text() {' \
  '    let _text = "println!(not a macro)";' \
  '    let _exit_text = "std::process::exit(1)";' \
  '    let exit = || ();' \
  '    exit();' \
  '}' \
  '' \
  '// std::process::exit(1);' \
  '' \
  'use std::process::exit as process_exit;' \
  '' \
  '#[cfg(all(test, not(all(feature = "simulation", madsim))))]' \
  'mod output_cases {' \
  '    fn prints_in_tests() {' \
  '        println!("test output");' \
  '        std::process::exit(1);' \
  '    }' \
  '}'
write_rs "$allow_case/crates/common/src/bin/tool.rs" \
  'fn main() {' \
  '    std::process::exit(0);' \
  '}'
write_rs "$allow_case/crates/common/src/examples/demo.rs" \
  'pub fn example_output() {' \
  '    println!("example output");' \
  '    std::process::exit(0);' \
  '}'
write_rs "$allow_case/crates/common/src/benches/demo.rs" \
  'pub fn bench_tool() {' \
  '    std::process::exit(0);' \
  '}'
write_rs "$allow_case/crates/common/src/logging/writer.rs" \
  'pub fn fallback() {' \
  '    eprintln!("logging fallback");' \
  '}'
write_rs "$allow_case/crates/model/src/identifiers/mod.rs" \
  'pub fn interned_string_stats() {' \
  '    ustr::string_cache_iter().for_each(|s| println!("{s}"));' \
  '}'
expect_success "$allow_case"

echo "Logging convention hook tests passed"
