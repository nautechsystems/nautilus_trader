#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
SCRIPT="$REPO_ROOT/scripts/ci/check-nightly-merge-status.bash"
WORKFLOW="$REPO_ROOT/.github/workflows/nightly-merge.yml"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

create_repo() {
  local name="$1"
  local repo="$CASE_ROOT/$name"

  git -c init.defaultBranch=develop init -q "$repo"
  git -C "$repo" config user.email "script-tests@example.com"
  git -C "$repo" config user.name "Script Tests"
  git -C "$repo" config commit.gpgsign false
  printf '%s\n' 'base' > "$repo/history.txt"
  git -C "$repo" add history.txt
  git -C "$repo" commit -qm "Initial state"
  printf '%s' "$repo"
}

commit_change() {
  local repo="$1"
  local message="$2"

  printf '%s\n' "$message" >> "$repo/history.txt"
  git -C "$repo" add history.txt
  git -C "$repo" commit -qm "$message"
  COMMIT_SHA=$(git -C "$repo" rev-parse HEAD)
}

run_check() {
  local repo="$1"
  local build_sha="$2"
  local nightly_sha="$3"
  local develop_sha="$4"

  git -C "$repo" update-ref refs/remotes/origin/nightly "$nightly_sha"
  git -C "$repo" update-ref refs/remotes/origin/develop "$develop_sha"
  jq -n --arg sha "$build_sha" '{
    workflow_runs: [{
      name: "build",
      head_branch: "develop",
      event: "push",
      status: "completed",
      conclusion: "success",
      head_sha: $sha
    }]
  }' > "$repo/workflow-runs.json"

  run_script "$repo"
}

run_script() {
  local repo="$1"

  RUN_OUTPUT="$repo/github-output.txt"
  RUN_STDOUT="$repo/stdout.txt"
  RUN_STDERR="$repo/stderr.txt"
  : > "$RUN_OUTPUT"

  set +e
  (
    cd "$repo"
    GITHUB_OUTPUT="$RUN_OUTPUT" bash "$SCRIPT" workflow-runs.json
  ) > "$RUN_STDOUT" 2> "$RUN_STDERR"
  RUN_STATUS=$?
  set -e
}

expect_status() {
  local expected="$1"

  if [[ "$RUN_STATUS" -ne "$expected" ]]; then
    echo "Expected status $expected, was $RUN_STATUS"
    cat "$RUN_STDOUT" "$RUN_STDERR"
    exit 1
  fi
}

expect_output() {
  local expected="$1"
  local actual

  actual=$(cat "$RUN_OUTPUT")
  if [[ "$actual" != "$expected" ]]; then
    printf 'Expected output:\n%s\nActual output:\n%s\n' "$expected" "$actual"
    exit 1
  fi
}

expect_log() {
  local expected="$1"

  if ! grep -Fq "$expected" "$RUN_STDOUT" "$RUN_STDERR"; then
    echo "Expected log text: $expected"
    cat "$RUN_STDOUT" "$RUN_STDERR"
    exit 1
  fi
}

advance_repo=$(create_repo "advance")
commit_change "$advance_repo" "Nightly"
advance_nightly="$COMMIT_SHA"
commit_change "$advance_repo" "Successful build"
advance_build="$COMMIT_SHA"
commit_change "$advance_repo" "Current develop"
run_check "$advance_repo" "$advance_build" "$advance_nightly" "$COMMIT_SHA"
expect_status 0
expect_output $'sha='"$advance_build"$'\nhas_changes=true'
expect_log "Develop has new successful changes to merge"

stale_repo=$(create_repo "stale")
commit_change "$stale_repo" "Successful build"
stale_build="$COMMIT_SHA"
commit_change "$stale_repo" "Nightly"
stale_nightly="$COMMIT_SHA"
commit_change "$stale_repo" "Current develop"
run_check "$stale_repo" "$stale_build" "$stale_nightly" "$COMMIT_SHA"
expect_status 0
expect_output $'sha='"$stale_build"$'\nhas_changes=false'
expect_log "stale or out-of-order build status"

equal_repo=$(create_repo "equal")
commit_change "$equal_repo" "Successful build"
equal_build="$COMMIT_SHA"
commit_change "$equal_repo" "Current develop"
run_check "$equal_repo" "$equal_build" "$equal_build" "$COMMIT_SHA"
expect_status 0
expect_output $'sha='"$equal_build"$'\nhas_changes=false'
expect_log "Nightly is already at the last successful develop commit"

inconsistent_repo=$(create_repo "inconsistent")
inconsistent_base=$(git -C "$inconsistent_repo" rev-parse HEAD)
commit_change "$inconsistent_repo" "Detached successful build"
inconsistent_build="$COMMIT_SHA"
git -C "$inconsistent_repo" switch --detach -q "$inconsistent_base"
commit_change "$inconsistent_repo" "Nightly"
inconsistent_nightly="$COMMIT_SHA"
commit_change "$inconsistent_repo" "Current develop"
run_check "$inconsistent_repo" "$inconsistent_build" "$inconsistent_nightly" "$COMMIT_SHA"
expect_status 0
expect_output $'sha='"$inconsistent_build"$'\nhas_changes=false'
expect_log "outside nightly's fast-forward path"

off_develop_repo=$(create_repo "off-develop")
commit_change "$off_develop_repo" "Nightly"
off_develop_nightly="$COMMIT_SHA"
commit_change "$off_develop_repo" "Successful build"
off_develop_build="$COMMIT_SHA"
git -C "$off_develop_repo" switch --detach -q "$off_develop_nightly"
commit_change "$off_develop_repo" "Current develop"
run_check "$off_develop_repo" "$off_develop_build" "$off_develop_nightly" "$COMMIT_SHA"
expect_status 0
expect_output $'sha='"$off_develop_build"$'\nhas_changes=false'
expect_log "outside the current develop history"

diverged_repo=$(create_repo "diverged")
diverged_base=$(git -C "$diverged_repo" rev-parse HEAD)
commit_change "$diverged_repo" "Successful build"
diverged_build="$COMMIT_SHA"
git -C "$diverged_repo" switch --detach -q "$diverged_base"
commit_change "$diverged_repo" "Nightly"
run_check "$diverged_repo" "$diverged_build" "$COMMIT_SHA" "$diverged_build"
expect_status 1
expect_output ""
expect_log "Nightly has diverged from current origin/develop"

missing_repo=$(create_repo "missing")
missing_nightly=$(git -C "$missing_repo" rev-parse HEAD)
commit_change "$missing_repo" "Current develop"
run_check "$missing_repo" "0000000000000000000000000000000000000000" "$missing_nightly" "$COMMIT_SHA"
expect_status 0
expect_output $'sha=0000000000000000000000000000000000000000\nhas_changes=false'
expect_log "build commit absent from the fetched develop history"

missing_diverged_repo=$(create_repo "missing-diverged")
missing_diverged_base=$(git -C "$missing_diverged_repo" rev-parse HEAD)
commit_change "$missing_diverged_repo" "Current develop"
missing_diverged_develop="$COMMIT_SHA"
git -C "$missing_diverged_repo" switch --detach -q "$missing_diverged_base"
commit_change "$missing_diverged_repo" "Nightly"
run_check \
  "$missing_diverged_repo" \
  "0000000000000000000000000000000000000000" \
  "$COMMIT_SHA" \
  "$missing_diverged_develop"
expect_status 1
expect_output ""
expect_log "Nightly has diverged from current origin/develop"

ahead_repo=$(create_repo "ahead")
commit_change "$ahead_repo" "Current develop"
ahead_develop="$COMMIT_SHA"
commit_change "$ahead_repo" "Nightly"
run_check "$ahead_repo" "$ahead_develop" "$COMMIT_SHA" "$ahead_develop"
expect_status 0
expect_output $'sha='"$ahead_develop"$'\nhas_changes=false'
expect_log "Nightly is ahead of current origin/develop"

empty_response_repo=$(create_repo "empty-response")
empty_response_sha=$(git -C "$empty_response_repo" rev-parse HEAD)
git -C "$empty_response_repo" update-ref refs/remotes/origin/nightly "$empty_response_sha"
git -C "$empty_response_repo" update-ref refs/remotes/origin/develop "$empty_response_sha"
printf '%s\n' '{"workflow_runs":[]}' > "$empty_response_repo/workflow-runs.json"
run_script "$empty_response_repo"
expect_status 1
expect_output ""
expect_log "Expected one successful develop build workflow"

invalid_sha_repo=$(create_repo "invalid-sha")
invalid_sha_head=$(git -C "$invalid_sha_repo" rev-parse HEAD)
run_check "$invalid_sha_repo" "invalid" "$invalid_sha_head" "$invalid_sha_head"
expect_status 1
expect_output ""
expect_log "Successful develop build has an invalid head SHA"

grep -Fq 'actions/workflows/build.yml/runs' "$WORKFLOW"
grep -Fq '?branch=develop&event=push&status=success&per_page=1' "$WORKFLOW"
grep -Fq 'Accept: application/vnd.github+json' "$WORKFLOW"
grep -Fq 'X-GitHub-Api-Version: 2022-11-28' "$WORKFLOW"
grep -Fq -- '--fail-with-body' "$WORKFLOW"

echo "Nightly merge status tests passed"
