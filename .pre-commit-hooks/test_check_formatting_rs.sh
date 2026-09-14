#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for Rust formatting hook tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
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
  git -C "$(dirname "$path")" add -- "$(basename "$path")"
}

create_case() {
  local case_dir="$1"

  mkdir -p "$case_dir"/{crates/common/src,tests,examples,docs}
  git -C "$case_dir" init -q
  git -C "$case_dir" config user.name "Formatting test"
  git -C "$case_dir" config user.email "formatting@example.invalid"
  git -C "$case_dir" config commit.gpgsign false
}

run_hook() {
  local case_dir="$1"

  (
    unset CHANGED_BASE_SHA
    cd "$case_dir" && bash "$HOOK"
  ) > "$case_dir/output.txt" 2>&1
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

for control_flow in match for while loop spawn; do
  control_flow_case="$CASE_ROOT/reject-missing-blank-$control_flow"
  create_case "$control_flow_case"

  case "$control_flow" in
    match)
      write_rs "$control_flow_case/crates/common/src/lib.rs" \
        'pub fn check(state: State) {' \
        '    prepare();' \
        '    match state {' \
        '        State::Ready => run(),' \
        '    }' \
        '}'
      ;;
    for)
      write_rs "$control_flow_case/crates/common/src/lib.rs" \
        'pub fn check(items: &[Item]) {' \
        '    prepare();' \
        '    for item in items {' \
        '        consume(item);' \
        '    }' \
        '}'
      ;;
    while)
      write_rs "$control_flow_case/crates/common/src/lib.rs" \
        'pub fn check(active: bool) {' \
        '    prepare();' \
        '    while active {' \
        '        run();' \
        '    }' \
        '}'
      ;;
    loop)
      write_rs "$control_flow_case/crates/common/src/lib.rs" \
        'pub fn check() {' \
        '    prepare();' \
        '    loop {' \
        '        run();' \
        '    }' \
        '}'
      ;;
    spawn)
      write_rs "$control_flow_case/crates/common/src/lib.rs" \
        'pub fn check() {' \
        '    prepare();' \
        '    tokio::spawn(async {});' \
        '}'
      ;;
  esac

  expect_failure "$control_flow_case" "Missing blank line above .${control_flow}."
done

valid_control_flow_case="$CASE_ROOT/allow-first-control-flow-statements"
create_case "$valid_control_flow_case"
write_rs "$valid_control_flow_case/crates/common/src/lib.rs" \
  'pub fn select(state: State) {' \
  '    match state {' \
  '        State::Ready => run(),' \
  '    }' \
  '}' \
  '' \
  'pub fn visit(items: &[Item]) {' \
  '    for item in items {' \
  '        consume(item);' \
  '    }' \
  '}' \
  '' \
  'pub fn poll(active: bool) {' \
  '    while active {' \
  '        run();' \
  '    }' \
  '}' \
  '' \
  'pub fn repeat() {' \
  '    loop {' \
  '        run();' \
  '    }' \
  '}' \
  '' \
  'pub fn start() {' \
  '    tokio::spawn(async {});' \
  '}'
expect_success "$valid_control_flow_case"

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
  '#[cfg(all(feature = "python", any(test, feature = "test-support")))]' \
  'mod cfg_nested;' \
  '#[cfg(feature = "python")]' \
  'pub mod cfg_public;' \
  '#[cfg(any(test, feature = "test-support"))]' \
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

scope_case="$CASE_ROOT/changed-lines"
create_case "$scope_case"
write_rs "$scope_case/crates/common/src/lib.rs" \
  'fn existing() {' \
  '    prepare();' \
  '    if ready { run(); }' \
  '}' \
  '' \
  'fn changed() {' \
  '    prepare();' \
  '' \
  '    if enabled { run(); }' \
  '}'
git -C "$scope_case" commit -qm Baseline
expect_success "$scope_case"

# An unrelated edit in the same file must not expose the old violation
printf '\n// Changed comment\n' >> "$scope_case/crates/common/src/lib.rs"
expect_success "$scope_case"

# Removing the separator must report the newly adjacent boundary
awk 'NR != 8' "$scope_case/crates/common/src/lib.rs" > "$scope_case/edited"
mv "$scope_case/edited" "$scope_case/crates/common/src/lib.rs"
expect_failure "$scope_case" 'crates/common/src/lib.rs:8'
violation_count=$(rg -c 'Missing blank line above' "$scope_case/output.txt")
[[ "$violation_count" -eq 1 ]]
git -C "$scope_case" add -- crates/common/src/lib.rs
expect_failure "$scope_case" 'crates/common/src/lib.rs:8'

base=$(git -C "$scope_case" rev-parse HEAD)
git -C "$scope_case" commit -qm Changed
expect_success "$scope_case"
if (cd "$scope_case" && CHANGED_BASE_SHA="$base" bash "$HOOK") > "$scope_case/output.txt" 2>&1; then
  echo "Expected the base-ref diff to report the committed violation"
  exit 1
fi
rg -q 'crates/common/src/lib.rs:8' "$scope_case/output.txt"

module_scope_case="$CASE_ROOT/changed-module-boundary"
create_case "$module_scope_case"
write_rs "$module_scope_case/crates/common/src/mod.rs" \
  'pub mod alpha;' \
  'pub mod beta;'
git -C "$module_scope_case" commit -qm Baseline
write_rs "$module_scope_case/crates/common/src/mod.rs" \
  'pub mod zeta;' \
  'pub mod beta;'
expect_failure "$module_scope_case" 'Module .*beta.* is not alphabetized'

lookahead_case="$CASE_ROOT/changed-exemption-input"
create_case "$lookahead_case"
write_rs "$lookahead_case/crates/common/src/lib.rs" \
  'fn check() {' \
  '    prepare(value);' \
  '    if ready {' \
  '        consume(value);' \
  '    }' \
  '}'
git -C "$lookahead_case" commit -qm Baseline
write_rs "$lookahead_case/crates/common/src/lib.rs" \
  'fn check() {' \
  '    prepare(value);' \
  '    if ready {' \
  '        consume(other);' \
  '    }' \
  '}'
expect_failure "$lookahead_case" 'crates/common/src/lib.rs:3'

forward_case="$CASE_ROOT/changed-forward-scan"
create_case "$forward_case"
write_rs "$forward_case/crates/common/src/lib.rs" \
  'fn check() {' \
  '    prepare();' \
  '    if ready' \
  '        && enabled' \
  '        && active =>' \
  '    {' \
  '        run();' \
  '    }' \
  '}'
git -C "$forward_case" commit -qm Baseline
write_rs "$forward_case/crates/common/src/lib.rs" \
  'fn check() {' \
  '    prepare();' \
  '    if ready' \
  '        && enabled' \
  '        && active' \
  '    {' \
  '        run();' \
  '    }' \
  '}'
expect_failure "$forward_case" 'crates/common/src/lib.rs:3'

fallback_case="$CASE_ROOT/unavailable-ci-base"
create_case "$fallback_case"
write_rs "$fallback_case/crates/common/src/lib.rs" 'fn check() {}'
git -C "$fallback_case" commit -qm Baseline
for unavailable in 0000000000000000000000000000000000000000 missing-ref; do
  (cd "$fallback_case" && CHANGED_BASE_SHA="$unavailable" bash "$HOOK") > "$fallback_case/output.txt" 2>&1
  rg -q 'checking all tracked Rust files' "$fallback_case/output.txt"
done
write_rs "$fallback_case/crates/common/src/lib.rs" \
  'fn check() {' \
  '    prepare();' \
  '    if ready { run(); }' \
  '}'
git -C "$fallback_case" commit -qm Changed
if (cd "$fallback_case" && CHANGED_BASE_SHA=missing-ref bash "$HOOK") > "$fallback_case/output.txt" 2>&1; then
  echo "Expected the unavailable-base fallback to detect a committed violation"
  exit 1
fi
rg -q 'crates/common/src/lib.rs:3' "$fallback_case/output.txt"

echo "Rust formatting hook tests passed"
