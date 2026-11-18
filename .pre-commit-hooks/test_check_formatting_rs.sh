#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for Rust formatting hook tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/scripts/check-rust-formatting.bash"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

write_rs() {
  local path="$1"
  shift

  mkdir -p "$(dirname "$path")"
  printf '%s\n' "$@" > "$path"
}

run_hook() {
  local case_dir="$1"

  (cd "$case_dir" && bash "$HOOK" crates/common/src/*.rs) > "$case_dir/output.txt" 2>&1
}

expect_failure() {
  local case_dir="$1"
  local pattern="$2"

  local status=0
  run_hook "$case_dir" || status=$?
  if [[ "$status" -ne 1 ]]; then
    echo "Expected a Rust formatting violation in $case_dir, received exit status $status"
    cat "$case_dir/output.txt"
    exit 1
  fi

  if ! rg -q "$pattern" "$case_dir/output.txt"; then
    echo "Expected diagnostic matching $pattern in $case_dir"
    cat "$case_dir/output.txt"
    exit 1
  fi
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

for control_flow in if for; do
  for continuation in first later absent; do
    wrapped_case="$CASE_ROOT/wrapped-$control_flow-$continuation"

    case "$continuation" in
      first)
        first_body='        consume(expected);'
        next_body='        work();'
        ;;
      later)
        first_body='        work();'
        next_body='        consume(expected);'
        ;;
      absent)
        first_body='        work();'
        next_body='        consume(unrelated);'
        ;;
    esac

    case "$control_flow" in
      if)
        write_rs "$wrapped_case/crates/common/src/lib.rs" \
          'fn check() {' \
          '    let expected = prepare();' \
          '    if current_start_date' \
          '        .zip(current_end_date)' \
          '        .is_some_and(|(start, end)| end <= start)' \
          '    {' \
          "$first_body" \
          "$next_body" \
          '    }' \
          '}'
        ;;
      for)
        write_rs "$wrapped_case/crates/common/src/lib.rs" \
          'fn check() {' \
          '    let expected = prepare();' \
          '    for contract in contracts' \
          '        .iter()' \
          '        .filter(|contract| contract.security_type == SecurityType::Spread)' \
          '    {' \
          "$first_body" \
          "$next_body" \
          '    }' \
          '}'
        ;;
    esac

    if [ "$continuation" = first ]; then
      expect_success "$wrapped_case"
    else
      expect_failure "$wrapped_case" "Missing blank line above .${control_flow}."
    fi
  done
done

valid_control_flow_case="$CASE_ROOT/allow-first-control-flow-statements"
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

for module_file in mod.rs lib.rs component.rs; do
  valid_modules_case="$CASE_ROOT/allow-valid-module-sections-$module_file"
  write_rs "$valid_modules_case/crates/common/src/$module_file" \
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
    'mod internal;' \
    '' \
    '#[cfg(feature = "python")]' \
    'pub mod cfg_public;' \
    '' \
    '#[cfg(feature = "python")]' \
    'pub(crate) mod cfg_crate;' \
    '#[cfg(feature = "python")]' \
    'pub(super) mod cfg_parent;' \
    '' \
    '#[cfg(all(feature = "python", any(test, feature = "test-support")))]' \
    'mod cfg_nested;' \
    '#[cfg(any(test, feature = "test-support"))]' \
    'mod cfg_stubs;' \
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

  gated_first_case="$CASE_ROOT/reject-gated-before-unconditional-$module_file"
  write_rs "$gated_first_case/crates/common/src/$module_file" \
    '#[cfg(feature = "python")]' \
    'pub mod optional;' \
    '' \
    'mod internal;'
  expect_failure "$gated_first_case" 'Module .*internal.* is in the wrong section'
done

for visibility in public restricted private; do
  unconditional_case="$CASE_ROOT/reject-unconditional-after-gated-$visibility"
  case "$visibility" in
    public) declaration='pub mod unconditional;' ;;
    restricted) declaration='pub(crate) mod unconditional;' ;;
    private) declaration='mod unconditional;' ;;
  esac
  write_rs "$unconditional_case/crates/common/src/mod.rs" \
    '#[cfg(feature = "python")]' \
    'mod optional;' \
    '' \
    "$declaration"
  expect_failure "$unconditional_case" 'Module .*unconditional.* is in the wrong section'
done

for declaration in 'pub mod public;' 'pub(crate) mod restricted;'; do
  gated_visibility_case="$CASE_ROOT/reject-gated-visibility-$declaration"
  write_rs "$gated_visibility_case/crates/common/src/mod.rs" \
    '#[cfg(feature = "python")]' \
    'mod internal;' \
    '' \
    '#[cfg(feature = "python")]' \
    "$declaration"
  expect_failure "$gated_visibility_case" 'Module .* is in the wrong section'
done

for declaration in 'pub mod beta;' 'pub(crate) mod beta;' 'mod beta;'; do
  gated_alphabetical_case="$CASE_ROOT/reject-gated-alphabetical-$declaration"
  write_rs "$gated_alphabetical_case/crates/common/src/mod.rs" \
    '#[cfg(feature = "python")]' \
    "${declaration/beta/zeta}" \
    '#[cfg(feature = "python")]' \
    "$declaration"
  expect_failure "$gated_alphabetical_case" 'Module .*beta.* is not alphabetized'
done

wrong_section_case="$CASE_ROOT/reject-module-section-order"
write_rs "$wrong_section_case/crates/common/src/mod.rs" \
  'mod internal;' \
  '' \
  'pub mod public;'
expect_failure "$wrong_section_case" "Module .*public.* is in the wrong section"

missing_blank_case="$CASE_ROOT/reject-missing-module-section-blank"
write_rs "$missing_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  'pub(crate) mod crate_api;'
expect_failure "$missing_blank_case" \
  "^crates/common/src/mod\.rs:2: Expected one blank line before restricted module \`crate_api\`$"

extra_blank_case="$CASE_ROOT/reject-extra-module-section-blank"
write_rs "$extra_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '' \
  'pub(crate) mod crate_api;'
expect_failure "$extra_blank_case" \
  "^crates/common/src/mod\.rs:4: Expected one blank line before restricted module \`crate_api\`$"

commented_boundary_case="$CASE_ROOT/allow-commented-module-section-boundary"
write_rs "$commented_boundary_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '// Internal implementation' \
  '' \
  'mod internal;'
expect_success "$commented_boundary_case"

extra_before_comment_case="$CASE_ROOT/reject-extra-blank-before-module-section-comment"
write_rs "$extra_before_comment_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '' \
  '// Internal implementation' \
  'mod internal;'
expect_failure "$extra_before_comment_case" \
  "^crates/common/src/mod\.rs:5: Expected one blank line before private module \`internal\`$"

extra_after_comment_case="$CASE_ROOT/reject-extra-blank-after-module-section-comment"
write_rs "$extra_after_comment_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '// Internal implementation' \
  '' \
  '' \
  'mod internal;'
expect_failure "$extra_after_comment_case" \
  "^crates/common/src/mod\.rs:5: Expected one blank line before private module \`internal\`$"

extra_commented_blank_case="$CASE_ROOT/reject-extra-commented-module-section-blank"
write_rs "$extra_commented_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '' \
  '' \
  '// Internal implementation' \
  '' \
  '' \
  'mod internal;'
expect_failure "$extra_commented_blank_case" \
  "^crates/common/src/mod\.rs:7: Expected one blank line before private module \`internal\`$"

comment_without_blank_case="$CASE_ROOT/reject-comment-without-module-section-blank"
write_rs "$comment_without_blank_case/crates/common/src/mod.rs" \
  'pub mod public;' \
  '// Internal implementation' \
  'mod internal;'
expect_failure "$comment_without_blank_case" \
  "^crates/common/src/mod\.rs:3: Expected one blank line before private module \`internal\`$"

alphabetical_case="$CASE_ROOT/reject-module-alphabetical-order"
write_rs "$alphabetical_case/crates/common/src/mod.rs" \
  'pub mod zeta;' \
  'pub mod alpha;'
expect_failure "$alphabetical_case" "Module .*alpha.* is not alphabetized"

test_section_case="$CASE_ROOT/reject-test-module-before-private"
write_rs "$test_section_case/crates/common/src/mod.rs" \
  '#[cfg(test)]' \
  'mod tests;' \
  '' \
  'mod internal;'
expect_failure "$test_section_case" "Module .*internal.* is in the wrong section"

module_scope_case="$CASE_ROOT/changed-module-boundary"
write_rs "$module_scope_case/crates/common/src/mod.rs" \
  'pub mod alpha;' \
  'pub mod beta;'
expect_success "$module_scope_case"
write_rs "$module_scope_case/crates/common/src/mod.rs" \
  'pub mod zeta;' \
  'pub mod beta;'
expect_failure "$module_scope_case" 'Module .*beta.* is not alphabetized'

lookahead_case="$CASE_ROOT/changed-exemption-input"
write_rs "$lookahead_case/crates/common/src/lib.rs" \
  'fn check() {' \
  '    prepare(value);' \
  '    if ready {' \
  '        consume(value);' \
  '    }' \
  '}'
expect_success "$lookahead_case"
write_rs "$lookahead_case/crates/common/src/lib.rs" \
  'fn check() {' \
  '    prepare(value);' \
  '    if ready {' \
  '        consume(other);' \
  '    }' \
  '}'
expect_failure "$lookahead_case" "^crates/common/src/lib\.rs:3: Missing blank line above \`if\`$"

precommit_case="$CASE_ROOT/precommit"
mkdir -p "$precommit_case/bin"
cat > "$precommit_case/bin/prek" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$PRECOMMIT_LOG"
exit "${PRECOMMIT_EXIT:-0}"
EOF
chmod +x "$precommit_case/bin/prek"

run_precommit() {
  PATH="$precommit_case/bin:$PATH" PRECOMMIT_LOG="$precommit_case/prek.txt" \
    make --no-print-directory -s -C "$REPO_ROOT" -o check-cargo-cooldown pre-commit
}

run_precommit
printf '%s\n' run --all-files > "$precommit_case/expected.txt"
cmp "$precommit_case/expected.txt" "$precommit_case/prek.txt"

status=0
rm "$precommit_case/prek.txt"
PRECOMMIT_EXIT=19 run_precommit > "$precommit_case/output.txt" 2>&1 || status=$?
[[ "$status" -eq 2 ]]
cmp "$precommit_case/expected.txt" "$precommit_case/prek.txt"

echo "Rust formatting hook tests passed"
