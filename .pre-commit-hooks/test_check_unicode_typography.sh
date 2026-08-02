#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_unicode_typography.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

write_codepoint() {
  local path="$1"
  local codepoint="$2"
  local suffix="${3:-}"

  mkdir -p "$(dirname "$path")"
  perl -CSDA -e 'print chr(hex($ARGV[0])), $ARGV[1], "\n"' "$codepoint" "$suffix" > "$path"
}

run_hook() {
  local case_dir="$1"
  shift

  (cd "$case_dir" && bash "$HOOK" "$@") > "$case_dir/output.txt" 2>&1
}

expect_failure() {
  local case_dir="$1"
  local file="$2"
  local pattern="$3"

  if run_hook "$case_dir" "$file"; then
    echo "Expected Unicode typography hook to fail for $file"
    cat "$case_dir/output.txt"
    exit 1
  fi

  grep -Fq "$pattern" "$case_dir/output.txt"
}

expect_success() {
  local case_dir="$1"
  shift

  if ! run_hook "$case_dir" "$@"; then
    echo "Expected Unicode typography hook to pass"
    cat "$case_dir/output.txt"
    exit 1
  fi
}

while IFS='|' read -r codepoint name; do
  case_dir="$CASE_ROOT/reject-$codepoint"
  file="sample.txt"
  write_codepoint "$case_dir/$file" "$codepoint"
  expect_failure "$case_dir" "$file" "$name"
done << 'CASES'
2013|U+2013 EN DASH
2014|U+2014 EM DASH
2018|U+2018 LEFT SINGLE QUOTATION MARK
2019|U+2019 RIGHT SINGLE QUOTATION MARK
201C|U+201C LEFT DOUBLE QUOTATION MARK
201D|U+201D RIGHT DOUBLE QUOTATION MARK
2705|U+2705 WHITE HEAVY CHECK MARK
274C|U+274C CROSS MARK
CASES

batch_case="$CASE_ROOT/reject-batch-with-exact-line"
mkdir -p "$batch_case"
printf '%s\n' first second > "$batch_case/first.txt"
printf '%s\n' first > "$batch_case/second.txt"
perl -CSDA -e 'print chr(hex($ARGV[0])), "\n"' 2014 >> "$batch_case/second.txt"
if run_hook "$batch_case" first.txt second.txt; then
  echo "Expected Unicode typography hook to fail for second.txt"
  cat "$batch_case/output.txt"
  exit 1
fi
grep -Fq "second.txt:2: U+2014 EM DASH" "$batch_case/output.txt"

allowed_case="$CASE_ROOT/allow-approved-codepoints"
mkdir -p "$allowed_case"
perl -CSDA -e 'print join(" ", map { chr hex } @ARGV), "\n"' 2011 2713 2717 > "$allowed_case/sample.txt"
expect_success "$allowed_case" sample.txt

fixture_case="$CASE_ROOT/allow-unicode-fixture"
write_codepoint "$fixture_case/sample.rs" 2014 " // unicode-typography: allow"
expect_success "$fixture_case" sample.rs

excluded_case="$CASE_ROOT/allow-excluded-paths"
excluded_files=(
  CLA.md
  RELEASES.md
  patches/upstream/README.md
  tests/integration/sample/resources/payload.json
  tests/test_data/payload.csv
)
for file in "${excluded_files[@]}"; do
  write_codepoint "$excluded_case/$file" 2014
done
expect_success "$excluded_case" "${excluded_files[@]}"

authored_resource_case="$CASE_ROOT/reject-authored-resource"
authored_resource_file="tests/integration/sample/resources/__init__.py"
write_codepoint "$authored_resource_case/$authored_resource_file" 2014
expect_failure "$authored_resource_case" "$authored_resource_file" "U+2014 EM DASH"

ascii_case="$CASE_ROOT/allow-ascii"
mkdir -p "$ascii_case"
printf '%s\n' "ASCII typography" > "$ascii_case/sample.txt"
expect_success "$ascii_case" sample.txt

echo "Unicode typography hook tests passed"
