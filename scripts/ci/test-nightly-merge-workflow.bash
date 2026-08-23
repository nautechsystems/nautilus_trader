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

printf 'successful develop build\n' >> "$seed/history.txt"
git -C "$seed" commit -qam "Successful develop build"
develop_sha=$(git -C "$seed" rev-parse HEAD)
git -C "$seed" push -q origin develop

git clone -q "$origin" "$runner"
git -C "$runner" config user.email "script-tests@example.com"
git -C "$runner" config user.name "Script Tests"
git -C "$runner" config commit.gpgsign false

response="$case_root/workflow-runs.json"
jq -n --arg sha "$develop_sha" '{
  total_count: 1,
  workflow_runs: [{
    name: "build",
    head_branch: "develop",
    event: "push",
    status: "completed",
    conclusion: "success",
    created_at: "2026-08-23T00:00:00Z",
    updated_at: "2026-08-23T00:01:00Z",
    run_number: 1,
    head_sha: $sha
  }]
}' > "$response"

mkdir -p "$fake_bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  "printf '%s\\n' \"\$*\" >> \"\$CURL_LOG\"" \
  "cat \"\$CURL_RESPONSE\"" \
  > "$fake_bin/curl"
chmod +x "$fake_bin/curl"

output="$case_root/github-output"
curl_log="$case_root/curl-log"
(
  cd "$runner"
  PATH="$fake_bin:$PATH" \
    CURL_LOG="$curl_log" \
    CURL_RESPONSE="$response" \
    GITHUB_OUTPUT="$output" \
    NIGHTLY_TOKEN=test-token \
    bash "$plan_script"
)

grep -Fxq "sha=$develop_sha" "$output"
grep -Fxq 'has_changes=true' "$output"
grep -Fq -- '--retry-all-errors' "$curl_log"
grep -Fq 'actions/workflows/build.yml/runs?branch=develop&event=push&per_page=100' "$curl_log"

(
  cd "$runner"
  bash "$merge_script" "$develop_sha"
)
remote_nightly=$(git --git-dir="$origin" rev-parse refs/heads/nightly)
if [ "$remote_nightly" != "$develop_sha" ]; then
  echo "Expected nightly at $develop_sha, found $remote_nightly" >&2
  exit 1
fi

if (cd "$runner" && bash "$merge_script" invalid) > /dev/null 2>&1; then
  echo "Expected an invalid develop SHA to fail" >&2
  exit 1
fi

printf 'nightly divergence\n' >> "$runner/history.txt"
git -C "$runner" commit -qam "Nightly divergence"
diverged_nightly=$(git -C "$runner" rev-parse HEAD)
git -C "$runner" push -q origin nightly

git -C "$runner" checkout -q -B develop origin/develop
printf 'develop divergence\n' >> "$runner/history.txt"
git -C "$runner" commit -qam "Develop divergence"
diverged_develop=$(git -C "$runner" rev-parse HEAD)
git -C "$runner" push -q origin develop

if (cd "$runner" && bash "$merge_script" "$diverged_develop") > /dev/null 2>&1; then
  echo "Expected a non-fast-forward nightly merge to fail" >&2
  exit 1
fi
remote_nightly=$(git --git-dir="$origin" rev-parse refs/heads/nightly)
if [ "$remote_nightly" != "$diverged_nightly" ]; then
  echo "Expected divergent nightly to remain at $diverged_nightly, found $remote_nightly" >&2
  exit 1
fi

echo "Nightly merge workflow tests passed"
