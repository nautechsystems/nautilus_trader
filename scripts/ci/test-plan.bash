#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
SCRIPT="$REPO_ROOT/scripts/ci/plan.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

create_repo() {
  local name="$1"
  local repo="$CASE_ROOT/$name"

  mkdir -p "$repo/docs"
  git -c init.defaultBranch=develop init -q "$repo"
  git -C "$repo" config user.email "script-tests@example.com"
  git -C "$repo" config user.name "Script Tests"
  git -C "$repo" config commit.gpgsign false
  printf '%s\n' '# Project' > "$repo/README.md"
  printf '%s\n' '# Guide' > "$repo/docs/guide.md"
  git -C "$repo" add README.md docs/guide.md
  git -C "$repo" commit -qm "Initial state"

  printf '%s' "$repo"
}

commit_changes() {
  local repo="$1"
  local message="$2"

  git -C "$repo" add -A
  git -C "$repo" commit -qm "$message"
}

run_plan() {
  local repo="$1"
  local event_name="$2"
  local base_ref="$3"
  local before_sha="$4"

  RUN_OUTPUT="$repo/github-output.txt"
  RUN_STDOUT="$repo/stdout.txt"
  RUN_STDERR="$repo/stderr.txt"
  : > "$RUN_OUTPUT"

  set +e
  (
    cd "$repo"
    EVENT_NAME="$event_name" \
      BASE_REF="$base_ref" \
      BEFORE_SHA="$before_sha" \
      GITHUB_OUTPUT="$RUN_OUTPUT" \
      bash "$SCRIPT"
  ) > "$RUN_STDOUT" 2> "$RUN_STDERR"
  RUN_STATUS=$?
  set -e
}

expect_status() {
  local expected="$1"

  if [ "$RUN_STATUS" -ne "$expected" ]; then
    echo "Expected plan.sh status $expected, got $RUN_STATUS"
    cat "$RUN_STDOUT" "$RUN_STDERR"
    exit 1
  fi
}

expect_text() {
  local path="$1"
  local expected="$2"
  local actual

  actual=$(cat "$path")
  if [ "$actual" != "$expected" ]; then
    echo "Unexpected contents in $path"
    printf 'Expected:\n%s\nActual:\n%s\n' "$expected" "$actual"
    exit 1
  fi
}

docs_repo=$(create_repo "docs-only")
docs_base=$(git -C "$docs_repo" rev-parse HEAD)
printf '%s\n' '# Updated guide' > "$docs_repo/docs/guide.md"
commit_changes "$docs_repo" "Update docs"
run_plan "$docs_repo" "push" "" "$docs_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=false\nrun_rust_tests=false\ngithub_changed=false'
expect_text "$RUN_STDOUT" "Docs-only changes: skipping build and test jobs"

delete_repo=$(create_repo "deleted-readme")
delete_base=$(git -C "$delete_repo" rev-parse HEAD)
git -C "$delete_repo" rm -q README.md
commit_changes "$delete_repo" "Remove README"
run_plan "$delete_repo" "push" "" "$delete_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=false\ngithub_changed=false'
expect_text "$RUN_STDOUT" "Python-only changes: skipping Rust tests"

python_repo=$(create_repo "python-only")
python_base=$(git -C "$python_repo" rev-parse HEAD)
mkdir -p "$python_repo/python"
printf '%s\n' 'VALUE = 1' > "$python_repo/python/example.py"
commit_changes "$python_repo" "Add Python code"
run_plan "$python_repo" "push" "" "$python_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=false\ngithub_changed=false'
expect_text "$RUN_STDOUT" "Python-only changes: skipping Rust tests"

rust_repo=$(create_repo "rust-change")
rust_base=$(git -C "$rust_repo" rev-parse HEAD)
mkdir -p "$rust_repo/crates/core/src"
printf '%s\n' 'pub fn run() {}' > "$rust_repo/crates/core/src/lib.rs"
commit_changes "$rust_repo" "Add Rust code"
run_plan "$rust_repo" "push" "" "$rust_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=true\ngithub_changed=false'
expect_text "$RUN_STDOUT" "Rust changes detected: running all jobs"

new_branch_repo=$(create_repo "new-branch")
run_plan "$new_branch_repo" "push" "" "0000000000000000000000000000000000000000"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=true\ngithub_changed=true'
expect_text "$RUN_STDOUT" "New branch push: running all jobs"

missing_base_repo=$(create_repo "missing-push-base")
run_plan "$missing_base_repo" "push" "" "1111111111111111111111111111111111111111"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=true\ngithub_changed=true'
expect_text "$RUN_STDOUT" "Push base SHA not found: running all jobs"

missing_ref_repo=$(create_repo "missing-pr-ref")
run_plan "$missing_ref_repo" "pull_request" "" ""
expect_status 1
expect_text "$RUN_OUTPUT" ""
expect_text "$RUN_STDERR" "::error::BASE_REF is required for pull_request events"

pr_repo=$(create_repo "pull-request")
pr_base=$(git -C "$pr_repo" rev-parse HEAD)
git -C "$pr_repo" update-ref refs/remotes/origin/develop "$pr_base"
git -C "$pr_repo" switch -qc feature
printf '%s\n' '# Feature guide' > "$pr_repo/docs/guide.md"
commit_changes "$pr_repo" "Update feature docs"
run_plan "$pr_repo" "pull_request" "develop" ""
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=false\nrun_rust_tests=false\ngithub_changed=false'
expect_text "$RUN_STDOUT" "Docs-only changes: skipping build and test jobs"

missing_merge_base_repo=$(create_repo "missing-merge-base")
run_plan "$missing_merge_base_repo" "pull_request" "unknown" ""
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=true\ngithub_changed=true'
expect_text "$RUN_STDOUT" "Failed to compute merge-base against origin/unknown: running all jobs"

# A scheduled workflow is out of scope for the build jobs but its action pins
# still need verifying, so the pin signal must survive the out-of-scope filter.
scheduled_repo=$(create_repo "scheduled-workflow")
scheduled_base=$(git -C "$scheduled_repo" rev-parse HEAD)
mkdir -p "$scheduled_repo/.github/workflows"
printf '%s\n' 'name: nightly' > "$scheduled_repo/.github/workflows/nightly-tests.yml"
commit_changes "$scheduled_repo" "Add scheduled workflow"
run_plan "$scheduled_repo" "push" "" "$scheduled_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=false\nrun_rust_tests=false\ngithub_changed=true'
expect_text "$RUN_STDOUT" "Docs-only changes: skipping build and test jobs"

composite_repo=$(create_repo "composite-action")
composite_base=$(git -C "$composite_repo" rev-parse HEAD)
mkdir -p "$composite_repo/.github/actions/common-setup"
printf '%s\n' 'name: common-setup' > "$composite_repo/.github/actions/common-setup/action.yml"
commit_changes "$composite_repo" "Add composite action"
run_plan "$composite_repo" "push" "" "$composite_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=true\ngithub_changed=true'
expect_text "$RUN_STDOUT" "Rust changes detected: running all jobs"

checker_repo=$(create_repo "pin-checker")
checker_base=$(git -C "$checker_repo" rev-parse HEAD)
mkdir -p "$checker_repo/scripts/ci"
printf '%s\n' '# checker' > "$checker_repo/scripts/ci/check-github-action-shas.sh"
commit_changes "$checker_repo" "Edit the pin checker"
run_plan "$checker_repo" "push" "" "$checker_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=false\ngithub_changed=true'
expect_text "$RUN_STDOUT" "Python-only changes: skipping Rust tests"

# The shared feature definition decides what every Rust gate compiles, so a
# change to it alone must still select the Rust lanes.
features_repo=$(create_repo "cargo-features")
features_base=$(git -C "$features_repo" rev-parse HEAD)
mkdir -p "$features_repo/scripts"
printf '%s\n' 'echo arrow' > "$features_repo/scripts/cargo-features.bash"
commit_changes "$features_repo" "Edit the shared feature list"
run_plan "$features_repo" "push" "" "$features_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=true\ngithub_changed=false'
expect_text "$RUN_STDOUT" "Rust changes detected: running all jobs"

# Rename detection reports only the destination, which would hide a moved gate
# input from the exact-path rules above.
rename_repo=$(create_repo "renamed-pin-checker")
mkdir -p "$rename_repo/scripts/ci"
printf '%s\n' '# checker' > "$rename_repo/scripts/ci/check-github-action-shas.sh"
commit_changes "$rename_repo" "Add the pin checker"
rename_base=$(git -C "$rename_repo" rev-parse HEAD)
git -C "$rename_repo" mv scripts/ci/check-github-action-shas.sh scripts/ci/check-action-pins.sh
commit_changes "$rename_repo" "Rename the pin checker"
run_plan "$rename_repo" "push" "" "$rename_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=true\nrun_rust_tests=false\ngithub_changed=true'
expect_text "$RUN_STDOUT" "Python-only changes: skipping Rust tests"

# The Makefile recipe owns the pin checker's invocation and file globs, so a
# Makefile-only change must still select the pin check even though the build
# jobs treat it as out of scope.
makefile_repo=$(create_repo "makefile-pin-recipe")
makefile_base=$(git -C "$makefile_repo" rev-parse HEAD)
printf '%s\n' 'check-github-action-pins:' > "$makefile_repo/Makefile"
commit_changes "$makefile_repo" "Edit the pin-check recipe"
run_plan "$makefile_repo" "push" "" "$makefile_base"
expect_status 0
expect_text "$RUN_OUTPUT" $'run_tests=false\nrun_rust_tests=false\ngithub_changed=true'
expect_text "$RUN_STDOUT" "Docs-only changes: skipping build and test jobs"

echo "CI plan script tests passed"
