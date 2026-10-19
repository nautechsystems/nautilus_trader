#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
plan_script="$repo_root/scripts/ci/plan-nightly-merge.bash"
merge_script="$repo_root/scripts/ci/merge-nightly.bash"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT
origin="$case_root/origin.git"
seed="$case_root/seed"
runner="$case_root/runner"
fake_bin="$case_root/bin"
response_dir="$case_root/responses"
real_git=$(command -v git)

git init -q --bare "$origin"
git --git-dir="$origin" symbolic-ref HEAD refs/heads/develop
git -c init.defaultBranch=develop init -q "$seed"
git -C "$seed" config user.email "script-tests@example.com"
git -C "$seed" config user.name "Script Tests"
git -C "$seed" config commit.gpgsign false
printf 'base\n' > "$seed/history.txt"
git -C "$seed" add history.txt
git -C "$seed" commit -qm "Base"
git -C "$seed" remote add origin "$origin"
git -C "$seed" push -q -u origin develop
git -C "$seed" branch nightly
git -C "$seed" push -q origin nightly

printf 'older successful build\n' >> "$seed/history.txt"
git -C "$seed" commit -qam "Older successful build"
older_sha=$(git -C "$seed" rev-parse HEAD)
git -C "$seed" push -q origin develop

printf 'latest successful build\n' >> "$seed/history.txt"
git -C "$seed" commit -qam "Latest successful build"
latest_sha=$(git -C "$seed" rev-parse HEAD)
git -C "$seed" push -q origin develop

printf 'current build in progress\n' >> "$seed/history.txt"
git -C "$seed" commit -qam "Current build in progress"
current_sha=$(git -C "$seed" rev-parse HEAD)
git -C "$seed" push -q origin develop

git clone -q "$origin" "$runner"
git -C "$runner" config user.email "script-tests@example.com"
git -C "$runner" config user.name "Script Tests"
git -C "$runner" config commit.gpgsign false

mkdir -p "$fake_bin" "$response_dir"
# shellcheck disable=SC2016
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'url=' \
  'for arg in "$@"; do url=$arg; done' \
  'printf "%s\n" "$url" >> "$CURL_LOG"' \
  'sha=${url#*head_sha=}' \
  'sha=${sha%%&*}' \
  'response="$CURL_RESPONSE_DIR/$sha.json"' \
  'if [[ "$sha" == "$url" || ! -f "$response" ]]; then' \
  '  echo "No response for URL: $url" >&2' \
  '  exit 1' \
  'fi' \
  'cat "$response"' \
  > "$fake_bin/curl"
chmod +x "$fake_bin/curl"
# shellcheck disable=SC2016
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'if [[ "${FAIL_REV_LIST:-}" == "true" && "$1" == "rev-list" ]]; then' \
  '  exit 2' \
  'fi' \
  'exec "$REAL_GIT" "$@"' \
  > "$fake_bin/git"
chmod +x "$fake_bin/git"

write_response() {
  local sha="$1"
  local status="$2"
  local conclusion="$3"
  local run_number="$4"

  jq -n \
    --arg sha "$sha" \
    --arg status "$status" \
    --argjson conclusion "$conclusion" \
    --argjson run_number "$run_number" \
    '{
      total_count: 1,
      workflow_runs: [{
        id: $run_number,
        name: "build",
        head_branch: "develop",
        head_sha: $sha,
        event: "push",
        status: $status,
        conclusion: $conclusion,
        run_number: $run_number,
        created_at: "2026-08-27T00:00:00Z",
        updated_at: "2026-08-27T00:01:00Z"
      }]
    }' > "$response_dir/$sha.json"
}

run_plan() {
  : > "$case_root/github-output"
  : > "$case_root/curl-log"

  set +e
  (
    cd "$runner"
    PATH="$fake_bin:$PATH" \
      CURL_LOG="$case_root/curl-log" \
      CURL_RESPONSE_DIR="$response_dir" \
      FAIL_REV_LIST="${FAIL_REV_LIST:-}" \
      GITHUB_OUTPUT="$case_root/github-output" \
      NIGHTLY_TOKEN=test-token \
      REAL_GIT="$real_git" \
      bash "$plan_script"
  ) > "$case_root/stdout" 2> "$case_root/stderr"
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

expect_output() {
  local expected="$1"
  local actual

  actual=$(cat "$case_root/github-output")
  if [[ "$actual" != "$expected" ]]; then
    printf 'Expected output:\n%s\nActual output:\n%s\n' "$expected" "$actual"
    exit 1
  fi
}

write_response "$current_sha" in_progress null 30
write_response "$latest_sha" completed '"success"' 29
jq '
  .workflow_runs[0] as $run
  | .total_count = 2
  | .workflow_runs = [
      ($run | .id = 28 | .event = "workflow_dispatch" | .run_number = 28),
      $run
    ]
' "$response_dir/$latest_sha.json" > "$case_root/multiple-runs.json"
mv "$case_root/multiple-runs.json" "$response_dir/$latest_sha.json"

run_plan
expect_status 0
expect_output $'sha='"$latest_sha"$'\nhas_changes=true'
grep -Fq '"id": 29' "$case_root/stdout"
if grep -Fq '"id": 28' "$case_root/stdout"; then
  echo "Planner logged an irrelevant successful workflow run" >&2
  exit 1
fi
grep -Fxq "https://api.github.com/repos/nautechsystems/nautilus_trader/actions/workflows/build.yml/runs?head_sha=$current_sha&per_page=100" "$case_root/curl-log"
grep -Fxq "https://api.github.com/repos/nautechsystems/nautilus_trader/actions/workflows/build.yml/runs?head_sha=$latest_sha&per_page=100" "$case_root/curl-log"
if grep -Fq "$older_sha" "$case_root/curl-log"; then
  echo "Planner queried a commit older than the latest successful build" >&2
  exit 1
fi
if grep -Fq 'branch=develop' "$case_root/curl-log"; then
  echo "Planner used the capped branch workflow search" >&2
  exit 1
fi

(
  cd "$runner"
  bash "$merge_script" "$latest_sha"
)
remote_nightly=$(git --git-dir="$origin" rev-parse refs/heads/nightly)
if [[ "$remote_nightly" != "$latest_sha" ]]; then
  echo "Expected nightly at $latest_sha, was $remote_nightly" >&2
  exit 1
fi

if (cd "$runner" && bash "$merge_script" invalid) > /dev/null 2>&1; then
  echo "Expected an invalid develop SHA to fail" >&2
  exit 1
fi

run_plan
expect_status 0
expect_output $'sha='"$latest_sha"$'\nhas_changes=false'
if grep -Fq "$latest_sha" "$case_root/curl-log"; then
  echo "Planner queried the current nightly commit" >&2
  exit 1
fi

jq '.total_count = 2' "$response_dir/$current_sha.json" > "$case_root/incomplete.json"
mv "$case_root/incomplete.json" "$response_dir/$current_sha.json"
run_plan
expect_status 2
write_response "$current_sha" in_progress null 30

FAIL_REV_LIST=true run_plan
expect_status 1
grep -Fq 'Failed to enumerate develop commits after nightly' "$case_root/stderr"

git --git-dir="$origin" update-ref refs/heads/nightly "$current_sha"
run_plan
expect_status 0
expect_output $'sha='"$current_sha"$'\nhas_changes=false'
if [[ -s "$case_root/curl-log" ]]; then
  echo "Planner queried GitHub when nightly matched develop" >&2
  exit 1
fi

git -C "$seed" switch --detach -q "$current_sha"
printf 'nightly ahead\n' >> "$seed/history.txt"
git -C "$seed" commit -qam "Nightly ahead"
ahead_sha=$(git -C "$seed" rev-parse HEAD)
git -C "$seed" push -q origin HEAD:nightly
run_plan
expect_status 0
expect_output $'sha='"$current_sha"$'\nhas_changes=false'
if [[ -s "$case_root/curl-log" ]]; then
  echo "Planner queried GitHub when nightly was ahead of develop" >&2
  exit 1
fi

git -C "$seed" switch -q develop
printf 'develop divergence\n' >> "$seed/history.txt"
git -C "$seed" commit -qam "Develop divergence"
diverged_develop=$(git -C "$seed" rev-parse HEAD)
git -C "$seed" push -q origin develop
diverged_nightly=$ahead_sha
run_plan
expect_status 1
if [[ -s "$case_root/curl-log" ]]; then
  echo "Planner queried GitHub after detecting branch divergence" >&2
  exit 1
fi

merge_runner="$case_root/merge-runner"
git clone -q "$origin" "$merge_runner"
git -C "$merge_runner" config user.email "script-tests@example.com"
git -C "$merge_runner" config user.name "Script Tests"
git -C "$merge_runner" config commit.gpgsign false
if (cd "$merge_runner" && bash "$merge_script" "$diverged_develop") > /dev/null 2>&1; then
  echo "Expected a non-fast-forward nightly merge to fail" >&2
  exit 1
fi
remote_nightly=$(git --git-dir="$origin" rev-parse refs/heads/nightly)
if [[ "$remote_nightly" != "$diverged_nightly" ]]; then
  echo "Expected divergent nightly to remain at $diverged_nightly, was $remote_nightly" >&2
  exit 1
fi

echo "Nightly merge workflow tests passed"
