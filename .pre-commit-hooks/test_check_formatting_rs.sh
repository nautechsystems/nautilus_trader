#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping Rust formatting hook tests"
  exit 0
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_formatting_rs.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

write_rs() {
  local path="$1"
  shift

  mkdir -p "$(dirname "$path")"
  printf '%s\n' "$@" > "$path"
}

create_case() {
  local case_dir="$1"

  mkdir -p "$case_dir"/{crates/common/src,tests,examples,docs}
}

run_hook() {
  local case_dir="$1"

  (cd "$case_dir" && bash "$HOOK") > "$case_dir/output.txt" 2>&1
}

expect_failure() {
  local case_dir="$1"
  local pattern="$2"

  if run_hook "$case_dir"; then
    echo "Expected Rust formatting hook to fail in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi

  rg -q "$pattern" "$case_dir/output.txt"
}

expect_success() {
  local case_dir="$1"

  if ! run_hook "$case_dir"; then
    echo "Expected Rust formatting hook to pass in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi
}

match_guard_and_if_case="$CASE_ROOT/allow-match-guard-reject-missing-blank"
create_case "$match_guard_and_if_case"
write_rs "$match_guard_and_if_case/crates/common/src/lib.rs" \
  'pub fn map_status(status: Status, filled_qty: Quantity, reason: &str) -> Status {' \
  '    match status {' \
  '        Status::Canceled' \
  '            if filled_qty.is_zero()' \
  '                && due_post_only(reason) =>' \
  '        {' \
  '            Status::Rejected' \
  '        }' \
  '        status => status,' \
  '    }' \
  '}' \
  '' \
  'pub fn check_ready(ready: bool, enabled: bool) {' \
  '    prepare();' \
  '    if ready' \
  '        && enabled' \
  '    {' \
  '        run();' \
  '    }' \
  '}'
expect_failure "$match_guard_and_if_case" "crates/common/src/lib.rs:15"

violation_count=$(rg -c "Missing blank line above" "$match_guard_and_if_case/output.txt")
if [ "$violation_count" -ne 1 ]; then
  echo "Expected exactly one missing-blank violation"
  cat "$match_guard_and_if_case/output.txt"
  exit 1
fi

valid_modules_case="$CASE_ROOT/allow-valid-module-sections"
create_case "$valid_modules_case"
write_rs "$valid_modules_case/crates/common/src/mod.rs" \
  '#[macro_use]' \
  'mod macros;' \
  '' \
  'pub mod alpha;' \
  '#[path = "zeta.rs"]' \
  'pub mod zeta;' \
  '' \
  'pub(crate) mod crate_api;' \
  'pub(super) mod parent_api;' \
  'pub(in crate::common) mod scoped_api;' \
  '' \
  '#[cfg(all(feature = "python", any(test, feature = "stubs")))]' \
  'mod cfg_nested;' \
  '#[cfg(feature = "python")]' \
  'pub mod cfg_public;' \
  '#[cfg(any(test, feature = "stubs"))]' \
  'mod cfg_stubs;' \
  '' \
  'mod internal;' \
  '' \
  '#[cfg(all(test, feature = "python"))]' \
  'mod python_tests;' \
  '#[cfg(all(feature = "python", test))]' \
  'mod reversed_tests;' \
  '#[cfg(test)]' \
  'mod tests;' \
  '' \
  'mod inline {' \
  '    pub fn run() {}' \
  '}'
expect_success "$valid_modules_case"

wrong_section_case="$CASE_ROOT/reject-module-section-order"
create_case "$wrong_section_case"
write_rs "$wrong_section_case/crates/common/src/mod.rs" \
  'mod internal;' \
  '' \
  'pub mod public;'
expect_failure "$wrong_section_case" "Module .*public.* is in the wrong section"

missing_blank_case="$CASE_ROOT/reject-missing-module-section-blank"
create_case "$missing_blank_case"
write_rs "$missing_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  'pub(crate) mod crate_api;'
expect_failure "$missing_blank_case" "Expected one blank line before restricted module"

extra_blank_case="$CASE_ROOT/reject-extra-module-section-blank"
create_case "$extra_blank_case"
write_rs "$extra_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '' \
  'pub(crate) mod crate_api;'
expect_failure "$extra_blank_case" "Expected one blank line before restricted module .* found 2"

commented_boundary_case="$CASE_ROOT/allow-commented-module-section-boundary"
create_case "$commented_boundary_case"
write_rs "$commented_boundary_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '// Internal implementation' \
  '' \
  'mod internal;'
expect_success "$commented_boundary_case"

extra_before_comment_case="$CASE_ROOT/reject-extra-blank-before-module-section-comment"
create_case "$extra_before_comment_case"
write_rs "$extra_before_comment_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '' \
  '// Internal implementation' \
  'mod internal;'
expect_failure "$extra_before_comment_case" "Expected one blank line before private module .* found 2"

extra_after_comment_case="$CASE_ROOT/reject-extra-blank-after-module-section-comment"
create_case "$extra_after_comment_case"
write_rs "$extra_after_comment_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '// Internal implementation' \
  '' \
  '' \
  'mod internal;'
expect_failure "$extra_after_comment_case" "Expected one blank line before private module .* found 2"

extra_commented_blank_case="$CASE_ROOT/reject-extra-commented-module-section-blank"
create_case "$extra_commented_blank_case"
write_rs "$extra_commented_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '' \
  '// Internal implementation' \
  '' \
  '' \
  'mod internal;'
expect_failure "$extra_commented_blank_case" "Expected one blank line before private module .* found 4"

comment_without_blank_case="$CASE_ROOT/reject-comment-without-module-section-blank"
create_case "$comment_without_blank_case"
write_rs "$comment_without_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '// Internal implementation' \
  'mod internal;'
expect_failure "$comment_without_blank_case" "Expected one blank line before private module .* found 0"

alphabetical_case="$CASE_ROOT/reject-module-alphabetical-order"
create_case "$alphabetical_case"
write_rs "$alphabetical_case/crates/common/src/mod.rs" \
  'pub mod zeta;' \
  'pub mod alpha;'
expect_failure "$alphabetical_case" "Module .*alpha.* is not alphabetized"

test_section_case="$CASE_ROOT/reject-test-module-before-private"
create_case "$test_section_case"
write_rs "$test_section_case/crates/common/src/mod.rs" \
  '#[cfg(test)]' \
  'mod tests;' \
  '' \
  'mod internal;'
expect_failure "$test_section_case" "Module .*internal.* is in the wrong section"

echo "Rust formatting hook tests passed"
