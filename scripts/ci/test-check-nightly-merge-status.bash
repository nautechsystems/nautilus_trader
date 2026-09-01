#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
script="$repo_root/scripts/ci/check-nightly-merge-status.bash"
plan_script="$repo_root/scripts/ci/plan-nightly-merge.bash"
workflow="$repo_root/.github/workflows/nightly-merge.yml"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT
develop_sha=$(git rev-parse HEAD)
other_sha=0000000000000000000000000000000000000000

run_check() {
  local response="$1"
  local sha="${2:-$develop_sha}"

  set +e
  bash "$script" "$response" "$sha" > "$case_root/stdout" 2> "$case_root/stderr"
  run_status=$?
  set -e
}

expect_status() {
  local expected="$1"

  if [[ "$run_status" -ne "$expected" ]]; then
    echo "Expected status $expected, was $run_status"
    cat "$case_root/stdout" "$case_root/stderr"
    exit 1
  fi
}

response="$case_root/workflow-runs.json"
jq -n --arg sha "$develop_sha" '{
  total_count: 1,
  workflow_runs: [{
    name: "build",
    head_branch: "develop",
    head_sha: $sha,
    event: "push",
    status: "completed",
    conclusion: "success",
    run_number: 10
  }]
}' > "$response"
run_check "$response"
expect_status 0

jq '.workflow_runs[0].conclusion = "failure"' "$response" > "$case_root/failure.json"
run_check "$case_root/failure.json"
expect_status 1

jq '
  .workflow_runs[0].status = "in_progress"
  | .workflow_runs[0].conclusion = null
' "$response" > "$case_root/in-progress.json"
run_check "$case_root/in-progress.json"
expect_status 1

jq '
  .total_count = 2
  | .workflow_runs[0].conclusion = "failure"
  | .workflow_runs += [(.workflow_runs[0] | .conclusion = "success" | .run_number = 11)]
' "$response" > "$case_root/mixed.json"
run_check "$case_root/mixed.json"
expect_status 0

printf '%s\n' '{"total_count":0,"workflow_runs":[]}' > "$case_root/empty.json"
run_check "$case_root/empty.json"
expect_status 1

jq '.total_count = 2' "$response" > "$case_root/incomplete.json"
run_check "$case_root/incomplete.json"
expect_status 2

jq --arg sha "$other_sha" '.workflow_runs[0].head_sha = $sha' "$response" > "$case_root/wrong-sha.json"
run_check "$case_root/wrong-sha.json"
expect_status 2

jq '.workflow_runs[0].head_branch = "nightly"' "$response" > "$case_root/wrong-branch.json"
run_check "$case_root/wrong-branch.json"
expect_status 1

jq 'del(.workflow_runs[0].conclusion)' "$response" > "$case_root/missing-conclusion.json"
run_check "$case_root/missing-conclusion.json"
expect_status 2

jq '.workflow_runs[0].status = "in_progress"' "$response" > "$case_root/inconsistent-status.json"
run_check "$case_root/inconsistent-status.json"
expect_status 1

run_check "$response" invalid
expect_status 2

printf '%s\n' '{' > "$case_root/malformed.json"
run_check "$case_root/malformed.json"
expect_status 2

grep -Fq 'actions/workflows/build.yml/runs' "$plan_script"
grep -Fq "?head_sha=\${candidate_sha}&per_page=100" "$plan_script"
if grep -Fq 'branch=develop' "$plan_script"; then
  echo "Nightly merge planner must not use the capped branch workflow search" >&2
  exit 1
fi
grep -Fq -- '--retry-all-errors' "$plan_script"
grep -Fq 'Accept: application/vnd.github+json' "$plan_script"
grep -Fq 'X-GitHub-Api-Version: 2022-11-28' "$plan_script"
grep -Fq -- '--fail-with-body' "$plan_script"
grep -Fq 'run: bash scripts/ci/plan-nightly-merge.bash' "$workflow"
grep -Fq "run: bash scripts/ci/merge-nightly.bash \"\$DEVELOP_SHA\"" "$workflow"

echo "Nightly merge status tests passed"
