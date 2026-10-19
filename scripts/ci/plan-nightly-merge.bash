#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
token=${NIGHTLY_TOKEN:?NIGHTLY_TOKEN is required}
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"
workflow_runs=$(mktemp)
trap 'rm -f "$workflow_runs"' EXIT

write_outputs() {
  printf 'sha=%s\nhas_changes=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"
}

is_ancestor() {
  local ancestor="$1"
  local descendant="$2"
  local status

  if git merge-base --is-ancestor "$ancestor" "$descendant"; then
    return 0
  else
    status=$?
  fi

  if [[ "$status" -eq 1 ]]; then
    return 1
  fi

  echo "ERROR: Failed to compare commits $ancestor and $descendant" >&2
  exit "$status"
}

if ! git ls-remote --exit-code --heads origin nightly; then
  echo "ERROR: nightly branch does not exist" >&2
  exit 1
fi

git fetch origin \
  +refs/heads/nightly:refs/remotes/origin/nightly \
  +refs/heads/develop:refs/remotes/origin/develop

nightly_sha=$(git rev-parse --verify 'refs/remotes/origin/nightly^{commit}')
develop_sha=$(git rev-parse --verify 'refs/remotes/origin/develop^{commit}')
echo "Current nightly HEAD: $nightly_sha"
echo "Current develop HEAD: $develop_sha"

if ! is_ancestor "$nightly_sha" "$develop_sha"; then
  if is_ancestor "$develop_sha" "$nightly_sha"; then
    echo "::warning::Nightly is ahead of current origin/develop; leaving nightly unchanged"
    write_outputs "$develop_sha" false
    exit 0
  fi

  echo "ERROR: Nightly has diverged from current origin/develop" >&2
  exit 1
fi

if [[ "$nightly_sha" == "$develop_sha" ]]; then
  echo "Nightly is already at current origin/develop"
  write_outputs "$develop_sha" false
  exit 0
fi

selected_sha=
if ! candidate_shas=$(git rev-list --first-parent "$nightly_sha..$develop_sha"); then
  echo "ERROR: Failed to enumerate develop commits after nightly" >&2
  exit 1
fi

for candidate_sha in $candidate_shas; do
  url="https://api.github.com/repos/nautechsystems/nautilus_trader/actions/workflows/build.yml/runs"
  url="${url}?head_sha=${candidate_sha}&per_page=100"
  echo "Checking build workflow for develop commit $candidate_sha"
  if ! curl -sS -L \
    --retry 5 --retry-delay 2 --retry-all-errors \
    --connect-timeout 5 --max-time 60 --fail-with-body \
    -H "Accept: application/vnd.github+json" \
    -H "Authorization: Bearer ${token}" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    "$url" > "$workflow_runs"; then
    echo "ERROR: Failed to fetch build workflows for develop commit $candidate_sha" >&2
    exit 1
  fi

  if bash "$script_dir/check-nightly-merge-status.bash" "$workflow_runs" "$candidate_sha"; then
    selected_sha=$candidate_sha
    echo "Latest successful develop build:"
    jq '
      .workflow_runs
      | map(select(
          .name == "build"
          and .head_branch == "develop"
          and .event == "push"
          and .status == "completed"
          and .conclusion == "success"
        ))
      | first
      | {id, run_number, head_sha, created_at, updated_at}
    ' "$workflow_runs"
    break
  else
    status=$?
  fi

  if [[ "$status" -ne 1 ]]; then
    exit "$status"
  fi
done

if [[ -z "$selected_sha" ]]; then
  echo "No new successful develop build to merge"
  write_outputs "$nightly_sha" false
  exit 0
fi

echo "Develop commit $selected_sha has a successful build to merge"
write_outputs "$selected_sha" true
